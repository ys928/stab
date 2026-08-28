//! Mutable client runtime and link lifecycle manager.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::Serialize;
use tokio::{sync::RwLock, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::info;
use uuid::Uuid;

use crate::config::{Link, StabConfig};

/// Snapshot of mutable client settings.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeConfigView {
    /// control port used when dialing the server
    pub control_port: u16,
    /// whether a secret is configured (hashed value is never exposed)
    pub has_secret: bool,
    /// reconnect attempts; `-1` infinite, `0` none
    pub retry: i32,
    /// reconnect interval in seconds
    pub retry_interval: u64,
}

/// Mutable settings read by link tasks.
#[derive(Debug)]
pub struct ClientRuntime {
    secret: RwLock<Option<String>>,
    control_port: RwLock<u16>,
    retry: RwLock<i32>,
    retry_interval: RwLock<u64>,
}

impl ClientRuntime {
    /// Seed from static startup config.
    pub fn from_cfg(cfg: &StabConfig) -> Arc<Self> {
        Arc::new(Self {
            secret: RwLock::new(cfg.secret.clone()),
            control_port: RwLock::new(cfg.port),
            retry: RwLock::new(cfg.retry),
            retry_interval: RwLock::new(cfg.retry_interval),
        })
    }

    /// Current secret (hashed).
    pub async fn secret(&self) -> Option<String> {
        self.secret.read().await.clone()
    }

    /// Control port for the stab server.
    pub async fn control_port(&self) -> u16 {
        *self.control_port.read().await
    }

    /// Retry policy.
    pub async fn retry(&self) -> i32 {
        *self.retry.read().await
    }

    /// Retry interval in seconds.
    pub async fn retry_interval(&self) -> u64 {
        *self.retry_interval.read().await
    }

    /// Apply partial updates. Returns whether links must be restarted
    /// (`secret` or `control_port` changed).
    pub async fn set_config(
        &self,
        secret: Option<Option<String>>,
        control_port: Option<u16>,
        retry: Option<i32>,
        retry_interval: Option<u64>,
    ) -> bool {
        let mut restart = false;
        if let Some(s) = secret {
            *self.secret.write().await = s;
            restart = true;
        }
        if let Some(p) = control_port {
            let mut port = self.control_port.write().await;
            if *port != p {
                *port = p;
                restart = true;
            }
        }
        if let Some(r) = retry {
            *self.retry.write().await = r;
        }
        if let Some(i) = retry_interval {
            *self.retry_interval.write().await = i;
        }
        restart
    }

    /// Public status view.
    pub async fn view(&self) -> RuntimeConfigView {
        RuntimeConfigView {
            control_port: *self.control_port.read().await,
            has_secret: self.secret.read().await.is_some(),
            retry: *self.retry.read().await,
            retry_interval: *self.retry_interval.read().await,
        }
    }
}

/// Lifecycle state of a managed link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    /// dialing / handshaking
    Connecting,
    /// control channel established
    Up,
    /// waiting to reconnect
    Retrying,
    /// cancelled or finished without retry
    Stopped,
}

/// Serializable link info for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    /// stable id
    pub link_id: String,
    /// local host
    pub local_host: String,
    /// local port
    pub local_port: u16,
    /// remote host
    pub remote_host: String,
    /// remote port request (0 = server assigns)
    pub remote_port: u16,
    /// current status
    pub status: LinkStatus,
    /// last error message, if any
    pub last_error: Option<String>,
}

struct LinkEntry {
    link: Arc<Link>,
    status: Arc<Mutex<LinkStatus>>,
    last_error: Arc<Mutex<Option<String>>>,
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

/// Spawns and stops tunnel link tasks.
pub struct LinkManager {
    runtime: Arc<ClientRuntime>,
    links: Mutex<HashMap<String, LinkEntry>>,
    /// factory for the per-link async worker (injected to avoid a cycle with local)
    runner: Arc<dyn Fn(LinkRunArgs) -> JoinHandle<()> + Send + Sync>,
}

/// Arguments passed to each link worker.
pub struct LinkRunArgs {
    /// link definition
    pub link: Arc<Link>,
    /// shared runtime
    pub runtime: Arc<ClientRuntime>,
    /// cancel signal
    pub cancel: CancellationToken,
    /// status cell
    pub status: Arc<Mutex<LinkStatus>>,
    /// last error cell
    pub last_error: Arc<Mutex<Option<String>>>,
}

impl LinkManager {
    /// Create a manager; `runner` typically wraps [`crate::local::run_link_task`].
    pub fn new(
        runtime: Arc<ClientRuntime>,
        runner: Arc<dyn Fn(LinkRunArgs) -> JoinHandle<()> + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            links: Mutex::new(HashMap::new()),
            runner,
        })
    }

    /// Add and start a link. Returns the new `link_id`.
    pub fn add(&self, link: Arc<Link>) -> String {
        let link_id = Uuid::new_v4().to_string();
        self.spawn_entry(link_id.clone(), link);
        link_id
    }

    /// Stop and remove one link.
    pub async fn remove(&self, link_id: &str) -> bool {
        let entry = {
            let mut map = self.links.lock().unwrap();
            map.remove(link_id)
        };
        let Some(entry) = entry else {
            return false;
        };
        entry.cancel.cancel();
        let _ = entry.handle.await;
        if let Ok(mut s) = entry.status.lock() {
            *s = LinkStatus::Stopped;
        }
        true
    }

    /// Stop and remove every link.
    pub async fn clear(&self) {
        let entries: Vec<LinkEntry> = {
            let mut map = self.links.lock().unwrap();
            map.drain().map(|(_, e)| e).collect()
        };
        for entry in entries {
            entry.cancel.cancel();
            let _ = entry.handle.await;
        }
    }

    /// Cancel all links and start them again with the same ids (after secret/port change).
    pub async fn restart_all(&self) {
        let snapshot: Vec<(String, Arc<Link>)> = {
            let map = self.links.lock().unwrap();
            map.iter()
                .map(|(id, e)| (id.clone(), e.link.clone()))
                .collect()
        };
        self.clear().await;
        for (id, link) in snapshot {
            self.spawn_entry(id, link);
        }
    }

    /// List current links.
    pub fn list(&self) -> Vec<LinkView> {
        let map = self.links.lock().unwrap();
        map.iter()
            .map(|(id, e)| LinkView {
                link_id: id.clone(),
                local_host: e.link.local.host.clone(),
                local_port: e.link.local.port,
                remote_host: e.link.remote.host.clone(),
                remote_port: e.link.remote.port,
                status: *e.status.lock().unwrap(),
                last_error: e.last_error.lock().unwrap().clone(),
            })
            .collect()
    }

    /// Wait until every link task has finished (non-API mode).
    pub async fn join_all(&self) {
        loop {
            let entry = {
                let mut map = self.links.lock().unwrap();
                let key = map.keys().next().cloned();
                key.and_then(|k| map.remove(&k))
            };
            match entry {
                Some(entry) => {
                    let _ = entry.handle.await;
                }
                None => break,
            }
        }
    }

    fn spawn_entry(&self, link_id: String, link: Arc<Link>) {
        let cancel = CancellationToken::new();
        let status = Arc::new(Mutex::new(LinkStatus::Connecting));
        let last_error = Arc::new(Mutex::new(None));
        let args = LinkRunArgs {
            link: link.clone(),
            runtime: self.runtime.clone(),
            cancel: cancel.clone(),
            status: status.clone(),
            last_error: last_error.clone(),
        };
        let handle = (self.runner)(args);
        info!("link {} started", link_id);
        let mut map = self.links.lock().unwrap();
        map.insert(
            link_id,
            LinkEntry {
                link,
                status,
                last_error,
                cancel,
                handle,
            },
        );
    }
}

/// Helper used by link tasks to update status.
pub fn set_status(status: &Mutex<LinkStatus>, value: LinkStatus) {
    if let Ok(mut s) = status.lock() {
        *s = value;
    }
}

/// Helper used by link tasks to record the last error.
pub fn set_last_error(last_error: &Mutex<Option<String>>, msg: Option<String>) {
    if let Ok(mut e) = last_error.lock() {
        *e = msg;
    }
}
