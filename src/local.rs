//! the local module code

use std::{sync::Arc, time::Duration};

use log::{error, info, trace, warn};
use tokio::{
    net::TcpStream,
    task::JoinHandle,
    time::{sleep, timeout},
};
use tracing::{trace_span, Instrument};
use uuid::Uuid;

use anyhow::{anyhow, bail, Context, Result};

use crate::{
    config::{Link, G_CFG},
    share::{proxy, FrameStream, M, NETWORK_TIMEOUT},
};

/// run local
pub async fn run() {
    let links = &G_CFG.get().unwrap().links;
    let port = G_CFG.get().unwrap().port;
    let mut joins: Vec<JoinHandle<()>> = Vec::new();
    for link in links {
        let join = tokio::spawn(
            async move {
                let _ = create_link(link.clone(), port)
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
async fn create_link(link: Arc<Link>, port: u16) -> Result<()> {
    let stream = connect_with_timeout(&link.remote.host, port).await?;

    let mut frame_stream = FrameStream::new(stream);

    let _ = auth(&mut frame_stream).await?;

    let _ = init_port(&mut frame_stream, &link).await?;

    let (mut frame_sender, mut frame_receiver) = frame_stream.split();

    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(3)).await;
            if let Err(e) = frame_sender.send(&M::Heartbeat).await {
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
            M::InitPort(_) => info!("unexpected init"),
            M::Auth(_) => warn!("unexpected auth"),
            M::Heartbeat => trace!("server >> heartbeat"),
            M::Error(e) => {
                return Err(anyhow!("{}", e));
            }
            M::Connect(port) => {
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

/// authentication info to server
async fn auth(frame_stream: &mut FrameStream) -> Result<()> {
    let secret = &G_CFG.get().unwrap().secret;

    let Some(secret) = secret else {
        return Ok(());
    };

    frame_stream.send(&M::Auth(secret.clone())).await?;

    let msg = frame_stream.recv_timeout().await?;
    match msg {
        M::Auth(_) => Ok(()),
        M::Error(e) => Err(anyhow!("{}", e)),
        _ => Err(anyhow!("unexpect msg")),
    }
}

/// send and recv InitPort message with server
async fn init_port(frame_stream: &mut FrameStream, link: &Arc<Link>) -> Result<()> {
    frame_stream.send(&M::InitPort(link.remote.port)).await?;
    let msg = frame_stream.recv_timeout().await?;
    match msg {
        M::InitPort(port) => {
            info!(
                "{}:{} link to {}:{}",
                link.local.host, link.local.port, link.remote.host, port
            );
            Ok(())
        }
        M::Error(e) => Err(anyhow!("{}", e)),
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
    let mut frame_stream = FrameStream::new(stream);

    auth(&mut frame_stream).await?;

    frame_stream.send(&M::Connect(port)).await?;

    let local = connect_with_timeout(&link.local.host, link.local.port).await?;

    proxy(local, frame_stream.stream()).await?;

    Ok(())
}
