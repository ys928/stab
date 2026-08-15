//! the local module code

use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use tokio::{
    net::TcpStream,
    task::JoinHandle,
    time::{sleep, timeout},
};
use tracing::{error, info, trace, trace_span, warn, Instrument};
use uuid::Uuid;

use crate::{
    config::{Link, G_CFG},
    share::{proxy, proxy_with_prepend, FrameStream, Msg, NETWORK_TIMEOUT},
};

/// run local
pub async fn run() {
    let links = &G_CFG.get().unwrap().links;
    let port = G_CFG.get().unwrap().port;
    let mut joins: Vec<JoinHandle<()>> = Vec::new();
    for link in links {
        let join = tokio::spawn(
            async move {
                run_link_with_retry(link.clone(), port).await;
            }
            .instrument(trace_span!("conn", id = Uuid::new_v4().to_string())),
        );
        joins.push(join);
    }
    for join in joins {
        let _ = join.await.map_err(|e| error!("{}", e));
    }
}

/// Keep reconnecting according to retry settings.
///
/// `retry = -1` means retry forever; `retry = 0` means never reconnect.
async fn run_link_with_retry(link: Arc<Link>, port: u16) {
    let cfg = G_CFG.get().unwrap();
    let max_retry = cfg.retry;
    let interval = cfg.retry_interval;
    let mut attempt: i32 = 0;

    loop {
        match create_link(link.clone(), port).await {
            Ok(()) => {
                warn!("{:?}: link closed", link);
            }
            Err(e) => {
                error!("{:?}:{}", link, e);
            }
        }

        if max_retry == 0 {
            break;
        }

        attempt = attempt.saturating_add(1);
        if max_retry > 0 && attempt > max_retry {
            error!("{:?}: exceeded retry limit ({})", link, max_retry);
            break;
        }

        let label = if max_retry < 0 {
            format!("attempt {attempt}, infinite")
        } else {
            format!("attempt {attempt}/{max_retry}")
        };
        warn!("{:?}: reconnecting in {}s ({})", link, interval, label);
        sleep(Duration::from_secs(interval)).await;
    }
}

/// begin a connect
async fn create_link(link: Arc<Link>, port: u16) -> Result<()> {
    let stream = connect_with_timeout(&link.remote.host, port).await?;

    let mut frame_stream = FrameStream::new(stream);

    let _ = init_port(&mut frame_stream, &link).await?;

    let (mut frame_sender, mut frame_receiver) = frame_stream.split();

    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(3)).await;
            if let Err(e) = frame_sender.send(&Msg::Heartbeat).await {
                error!("{}", e);
                break;
            }
        }
    });

    loop {
        let msg = frame_receiver.recv().await;
        let Ok(msg) = msg else {
            bail!("{:?}", msg.unwrap_err());
        };

        match msg {
            Msg::InitPort(_, _) => info!("unexpected init"),
            Msg::Heartbeat => trace!("server >> heartbeat"),
            Msg::Start => info!("unexpected start on control link"),
            Msg::Error(e) => {
                return Err(anyhow!("{}", e));
            }
            Msg::Connect(port, _) => {
                let link = link.clone();
                tokio::spawn(async move {
                    info!("new connection");
                    match handle_proxy_connection(port, &link).await {
                        Ok(_) => info!("connection exited"),
                        Err(err) => warn!("connection exited with error {}", err),
                    }
                });
            }
        }
    }
}

/// send and recv InitPort message with server
async fn init_port(frame_stream: &mut FrameStream, link: &Arc<Link>) -> Result<()> {
    let secret = &G_CFG.get().unwrap().secret;

    frame_stream
        .send(&Msg::InitPort(link.remote.port, secret.clone()))
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
async fn handle_proxy_connection(port: u16, link: &Link) -> Result<()> {
    let stream = connect_with_timeout(&link.remote.host, G_CFG.get().unwrap().port).await?;
    let _ = stream.set_nodelay(true);
    let mut frame_stream = FrameStream::new(stream);

    let secret = &G_CFG.get().unwrap().secret;

    frame_stream
        .send(&Msg::Connect(port, secret.clone()))
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
