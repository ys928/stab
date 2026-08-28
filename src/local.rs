//! the local module code

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use tokio::{
    net::TcpStream,
    sync::oneshot,
    time::{sleep, timeout},
};
use tracing::{error, info, trace, trace_span, warn, Instrument};
use uuid::Uuid;

use crate::{
    config::{Link, G_CFG},
    runtime::{
        set_last_error, set_status, ClientRuntime, LinkManager, LinkRunArgs, LinkStatus,
    },
    share::{
        proxy, proxy_with_prepend, FrameStream, Msg, HEARTBEAT_INTERVAL_CLIENT, HEARTBEAT_TIMEOUT,
        NETWORK_TIMEOUT,
    },
};

/// run local
pub async fn run() {
    let cfg = G_CFG.get().unwrap();
    let runtime = ClientRuntime::from_cfg(cfg);
    let runner = Arc::new(|args: LinkRunArgs| {
        tokio::spawn(
            async move {
                run_link_task(args).await;
            }
            .instrument(trace_span!("conn", id = Uuid::new_v4().to_string())),
        )
    });
    let manager = LinkManager::new(runtime.clone(), runner);

    if cfg.api {
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let api_runtime = runtime.clone();
        let api_manager = manager.clone();
        let token = cfg
            .api_token
            .clone()
            .expect("api_token must be set when --api");
        let path = cfg.api_path.clone();
        tokio::spawn(async move {
            if let Err(e) =
                crate::api::run(api_runtime, api_manager, token, path, shutdown_tx).await
            {
                error!("api server exited: {}", e);
            }
        });

        for link in &cfg.links {
            manager.add(link.clone());
        }

        let _ = shutdown_rx.await;
        manager.clear().await;
    } else {
        for link in &cfg.links {
            manager.add(link.clone());
        }
        manager.join_all().await;
    }
}

/// Entry point for a managed link task (used by [`LinkManager`]).
pub async fn run_link_task(args: LinkRunArgs) {
    run_link_with_retry(
        args.link,
        args.runtime,
        args.cancel,
        args.status,
        args.last_error,
    )
    .await;
}

/// Keep reconnecting according to retry settings.
///
/// `retry = -1` means retry forever; `retry = 0` means never reconnect.
async fn run_link_with_retry(
    link: Arc<Link>,
    runtime: Arc<ClientRuntime>,
    cancel: tokio_util::sync::CancellationToken,
    status: Arc<Mutex<LinkStatus>>,
    last_error: Arc<Mutex<Option<String>>>,
) {
    let mut attempt: i32 = 0;

    loop {
        if cancel.is_cancelled() {
            set_status(&status, LinkStatus::Stopped);
            break;
        }

        set_status(&status, LinkStatus::Connecting);
        match create_link(link.clone(), runtime.clone(), cancel.clone(), &status).await {
            Ok(()) => {
                warn!("{:?}: link closed", link);
                set_last_error(&last_error, None);
            }
            Err(e) => {
                error!("{:?}:{}", link, e);
                set_last_error(&last_error, Some(e.to_string()));
            }
        }

        if cancel.is_cancelled() {
            set_status(&status, LinkStatus::Stopped);
            break;
        }

        let max_retry = runtime.retry().await;
        let interval = runtime.retry_interval().await;

        if max_retry == 0 {
            set_status(&status, LinkStatus::Stopped);
            break;
        }

        attempt = attempt.saturating_add(1);
        if max_retry > 0 && attempt > max_retry {
            error!("{:?}: exceeded retry limit ({})", link, max_retry);
            set_status(&status, LinkStatus::Stopped);
            break;
        }

        let label = if max_retry < 0 {
            format!("attempt {attempt}, infinite")
        } else {
            format!("attempt {attempt}/{max_retry}")
        };
        warn!("{:?}: reconnecting in {}s ({})", link, interval, label);
        set_status(&status, LinkStatus::Retrying);

        tokio::select! {
            _ = cancel.cancelled() => {
                set_status(&status, LinkStatus::Stopped);
                break;
            }
            _ = sleep(Duration::from_secs(interval)) => {}
        }
    }
}

/// begin a connect
async fn create_link(
    link: Arc<Link>,
    runtime: Arc<ClientRuntime>,
    cancel: tokio_util::sync::CancellationToken,
    status: &Mutex<LinkStatus>,
) -> Result<()> {
    let port = runtime.control_port().await;
    let stream = connect_with_timeout(&link.remote.host, port).await?;

    let mut frame_stream = FrameStream::new(stream);

    let _ = init_port(&mut frame_stream, &link, &runtime).await?;
    set_status(status, LinkStatus::Up);

    let (mut frame_sender, mut frame_receiver) = frame_stream.split();

    let last_server_msg = Arc::new(Mutex::new(Instant::now()));
    let (dead_tx, mut dead_rx) = oneshot::channel::<String>();

    let hb_cancel = cancel.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = hb_cancel.cancelled() => break,
                _ = sleep(HEARTBEAT_INTERVAL_CLIENT) => {
                    if let Err(e) = frame_sender.send(&Msg::Heartbeat).await {
                        let _ = dead_tx.send(format!("control send failed: {e}"));
                        break;
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                bail!("cancelled");
            }
            reason = &mut dead_rx => {
                let reason = reason.unwrap_or_else(|_| "control send task ended".into());
                bail!("{reason}");
            }
            _ = sleep(Duration::from_secs(5)) => {
                let elapsed = last_server_msg.lock().unwrap().elapsed();
                if elapsed > HEARTBEAT_TIMEOUT {
                    bail!(
                        "no server heartbeat for {:.0}s",
                        elapsed.as_secs_f64()
                    );
                }
            }
            msg = frame_receiver.recv() => {
                let Ok(msg) = msg else {
                    bail!("control recv closed: {:?}", msg.unwrap_err());
                };
                *last_server_msg.lock().unwrap() = Instant::now();

                match msg {
                    Msg::InitPort(_, _) => info!("unexpected init"),
                    Msg::Heartbeat => trace!("server >> heartbeat"),
                    Msg::Start => info!("unexpected start on control link"),
                    Msg::Error(e) => {
                        return Err(anyhow!("{}", e));
                    }
                    Msg::Connect(port, _) => {
                        let link = link.clone();
                        let runtime = runtime.clone();
                        tokio::spawn(async move {
                            info!("new connection");
                            match handle_proxy_connection(port, &link, &runtime).await {
                                Ok(_) => info!("connection exited"),
                                Err(err) => warn!("connection exited with error {}", err),
                            }
                        });
                    }
                }
            }
        }
    }
}

/// send and recv InitPort message with server
async fn init_port(
    frame_stream: &mut FrameStream,
    link: &Arc<Link>,
    runtime: &ClientRuntime,
) -> Result<()> {
    let secret = runtime.secret().await;

    frame_stream
        .send(&Msg::InitPort(link.remote.port, secret))
        .await?;
    let msg = frame_stream.recv_timeout().await?;
    match msg {
        Msg::InitPort(port, _) => {
            info!(
                "{}:{} link to {}:{}",
                link.local.host, link.local.port, link.remote.host, port
            );
            Ok(())
        }
        Msg::Error(e) => Err(anyhow!("{}", e)),
        _ => Err(anyhow!("unexpect msg")),
    }
}

/// create a TcpStream from to:port
async fn connect_with_timeout(addr: &str, port: u16) -> Result<TcpStream> {
    let conn = timeout(NETWORK_TIMEOUT, TcpStream::connect((addr, port)))
        .await
        .context(format!("{}:{}", addr, port))??;
    Ok(conn)
}

/// deal connection from server proxy port
async fn handle_proxy_connection(
    port: u16,
    link: &Link,
    runtime: &ClientRuntime,
) -> Result<()> {
    let control_port = runtime.control_port().await;
    let stream = connect_with_timeout(&link.remote.host, control_port).await?;
    let _ = stream.set_nodelay(true);
    let mut frame_stream = FrameStream::new(stream);

    let secret = runtime.secret().await;

    frame_stream
        .send(&Msg::Connect(port, secret))
        .await?;

    // Wait until the server pairs a real client. Connecting to the local
    // target earlier leaves idle SSH (etc.) sessions that get killed, and the
    // tunnel then closes as soon as a user connects.
    //
    // Bound the wait so NAT-killed work connections do not leak tasks forever
    // when the server never sends Start.
    let msg = timeout(Duration::from_secs(60), frame_stream.recv())
        .await
        .context("timeout waiting for Start")??;
    match msg {
        Msg::Start => {}
        Msg::Error(e) => return Err(anyhow!("{}", e)),
        other => return Err(anyhow!("unexpected msg before start: {:?}", other)),
    }

    let (tunnel, head) = frame_stream.into_tcp_stream();
    let local = connect_with_timeout(&link.local.host, link.local.port).await?;
    let _ = local.set_nodelay(true);

    if head.is_empty() {
        proxy(local, tunnel).await?;
    } else {
        // Rare: bytes already buffered from the tunnel toward local.
        proxy_with_prepend(local, tunnel, &head).await?;
    }

    Ok(())
}
