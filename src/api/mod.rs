//! Local IPC control API for embedding the client.

mod ipc;

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::{
    config::{hash_secret, parse_link, Link},
    runtime::{ClientRuntime, LinkManager, LinkView, RuntimeConfigView},
};

pub use ipc::run_ipc;

/// Start the IPC control server and announce the endpoint on stdout.
pub async fn run(
    runtime: Arc<ClientRuntime>,
    manager: Arc<LinkManager>,
    token: String,
    api_path: Option<String>,
    shutdown_tx: oneshot::Sender<()>,
) -> Result<()> {
    let path = ipc::default_or_path(api_path)?;
    let state = ApiState {
        runtime,
        manager,
        token: token.clone(),
        shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
    };
    run_ipc(path, state, token).await
}

/// Shared API state for one client process.
pub struct ApiState {
    runtime: Arc<ClientRuntime>,
    manager: Arc<LinkManager>,
    token: String,
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

/// Incoming JSON-RPC style request.
#[derive(Debug, Deserialize)]
pub struct ApiRequest {
    id: Value,
    method: String,
    token: Option<String>,
    #[serde(default)]
    params: Value,
}

/// Outgoing response.
#[derive(Debug, Serialize)]
pub struct ApiResponse {
    id: Value,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SetConfigParams {
    secret: Option<String>,
    /// when true, clear the secret
    #[serde(default)]
    clear_secret: bool,
    control_port: Option<u16>,
    retry: Option<i32>,
    retry_interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AddLinkParams {
    /// full link string, e.g. `127.0.0.1:8000=server.com:0`
    link: Option<String>,
    local: Option<String>,
    remote: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LinkIdParams {
    link_id: String,
}

#[derive(Debug, Serialize)]
struct StatusResult {
    config: RuntimeConfigView,
    links: Vec<LinkView>,
}

/// Handle one request; returns the response to write back.
pub async fn dispatch(state: &ApiState, req: ApiRequest) -> ApiResponse {
    let id = req.id.clone();
    if req.token.as_deref() != Some(state.token.as_str()) {
        return ApiResponse {
            id,
            ok: false,
            result: None,
            error: Some("unauthorized".into()),
        };
    }

    match req.method.as_str() {
        "ping" => ok(id, json!("pong")),
        "get_status" => {
            let config = state.runtime.view().await;
            let links = state.manager.list();
            ok(id, json!(StatusResult { config, links }))
        }
        "set_config" => match serde_json::from_value::<SetConfigParams>(req.params) {
            Ok(p) => {
                let secret_update = if p.clear_secret {
                    Some(None)
                } else {
                    p.secret.map(|s| Some(hash_secret(s)))
                };
                let restart = state
                    .runtime
                    .set_config(secret_update, p.control_port, p.retry, p.retry_interval)
                    .await;
                if restart {
                    state.manager.restart_all().await;
                }
                ok(id, json!({"restarted": restart}))
            }
            Err(e) => err(id, e.to_string()),
        },
        "add_link" => match serde_json::from_value::<AddLinkParams>(req.params) {
            Ok(p) => match build_link(p) {
                Ok(link) => {
                    let link_id = state.manager.add(Arc::new(link));
                    ok(id, json!({"link_id": link_id}))
                }
                Err(e) => err(id, e),
            },
            Err(e) => err(id, e.to_string()),
        },
        "remove_link" => match serde_json::from_value::<LinkIdParams>(req.params) {
            Ok(p) => {
                let removed = state.manager.remove(&p.link_id).await;
                if removed {
                    ok(id, json!({"removed": true}))
                } else {
                    err(id, "link not found".into())
                }
            }
            Err(e) => err(id, e.to_string()),
        },
        "list_links" => ok(id, json!(state.manager.list())),
        "clear_links" => {
            state.manager.clear().await;
            ok(id, json!({"cleared": true}))
        }
        "shutdown" => {
            if let Some(tx) = state.shutdown_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            ok(id, json!({"shutdown": true}))
        }
        other => err(id, format!("unknown method: {other}")),
    }
}

fn build_link(p: AddLinkParams) -> Result<Link, String> {
    if let Some(raw) = p.link {
        return parse_link(&raw, None).map_err(|e| e.to_string());
    }
    let local = p.local.ok_or_else(|| "missing local".to_string())?;
    let remote = p.remote.ok_or_else(|| "missing remote".to_string())?;
    let raw = format!("{local}={remote}");
    parse_link(&raw, None).map_err(|e| e.to_string())
}

fn ok(id: Value, result: Value) -> ApiResponse {
    ApiResponse {
        id,
        ok: true,
        result: Some(result),
        error: None,
    }
}

fn err(id: Value, error: String) -> ApiResponse {
    ApiResponse {
        id,
        ok: false,
        result: None,
        error: Some(error),
    }
}
