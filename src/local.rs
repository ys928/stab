//! the local module code

use log::{error, info, trace, warn};
use tokio::{net::TcpStream, task::JoinHandle, time::timeout};
use tracing::{debug, trace_span, Instrument};
use uuid::Uuid;

use anyhow::{anyhow, Context, Result};

use crate::{
    config::{Link, G_CFG},
    share::{proxy, FrameStream, Message, NETWORK_TIMEOUT},
};

/// run local
pub async fn run() {
    let links = &G_CFG.get().unwrap().links;
    let port = G_CFG.get().unwrap().port;
    let mut joins: Vec<JoinHandle<()>> = Vec::new();
    for link in links.iter() {
        let join = tokio::spawn(
            async move {
                let _ = create_link(link, port)
                    .await
                    .map_err(|e| error!("{:?}:{}", link, e));
            }
            .instrument(trace_span!("conn", id = Uuid::new_v4().to_string())),
        );
        joins.push(join);
    }
    for join in joins {
        let _ = join.await.map_err(|e| error!("{}", e));
    }
}

/// begin a connect
async fn create_link(link: &Link, port: u16) -> Result<()> {
    let stream = connect_with_timeout(&link.remote.host, port).await?;

    let mut frame_stream = FrameStream::new(stream);

    let _ = auth(&mut frame_stream).await?;

    let _ = init_port(&mut frame_stream, link).await?;

    loop {
        // sure connection is established
        frame_stream
            .send(&Message::Heartbeat)
            .await
            .context("heartbeat failed")?;

        let msg = frame_stream.recv_timeout().await;
        let Ok(msg) = msg else {
            debug!("{:?}", msg.unwrap_err());
            continue;
        };

        match msg {
            Message::InitPort(_) => info!("unexpected init"),
            Message::Auth(_) => warn!("unexpected auth"),
            Message::Heartbeat => trace!("server >> heartbeat"),
            Message::Error(e) => {
                return Err(anyhow!("{}", e));
            }
            Message::Connect(id) => {
                let link = link.clone();
                tokio::spawn(async move {
                    info!("new connection");
                    match handle_proxy_connection(id, &link).await {
                        Ok(_) => info!("connection exited"),
                        Err(err) => warn!("connection exited with error {}", err),
                    }
                });
            }
        }
    }
}

/// authentication info to server
async fn auth(frame_stream: &mut FrameStream) -> Result<()> {
    let secret = &G_CFG.get().unwrap().secret;
    if secret.is_none() {
        return Ok(());
    }
    let secret = secret.as_ref().unwrap();

    frame_stream.send(&Message::Auth(secret.clone())).await?;

    let msg = frame_stream.recv_timeout().await?;
    match msg {
        Message::Auth(_) => Ok(()),
        Message::Error(e) => Err(anyhow!("{}", e)),
        _ => Err(anyhow!("unexpect msg")),
    }
}

/// send and recv InitPort message with server
async fn init_port(frame_stream: &mut FrameStream, link: &Link) -> Result<()> {
    frame_stream
        .send(&Message::InitPort(link.remote.port))
        .await?;
    let msg = frame_stream.recv_timeout().await?;
    match msg {
        Message::InitPort(port) => {
            info!(
                "{}:{} link to {}:{}",
                link.local.host, link.local.port, link.remote.host, port
            );
            Ok(())
        }
        Message::Error(e) => Err(anyhow!("{}", e)),
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
async fn handle_proxy_connection(id: Uuid, link: &Link) -> Result<()> {
    let stream = connect_with_timeout(&link.remote.host, G_CFG.get().unwrap().port).await?;
    let mut frame_stream = FrameStream::new(stream);

    auth(&mut frame_stream).await?;

    frame_stream.send(&Message::Connect(id)).await?;

    let local = connect_with_timeout(&link.local.host, link.local.port).await?;

    proxy(local, frame_stream.stream()).await?;

    Ok(())
}
