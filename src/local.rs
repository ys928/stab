//! the local module code

use std::io::{Error, ErrorKind};

use log::{error, info, trace, warn};
use tokio::{net::TcpStream, task::JoinHandle, time::timeout};
use uuid::Uuid;

use crate::{
    config::{Link, G_CFG},
    share::{proxy, FrameStream, Message, NETWORK_TIMEOUT},
};

/// run client
pub async fn run() {
    let links = &G_CFG.get().unwrap().links;
    let port = G_CFG.get().unwrap().port;
    let mut joins: Vec<JoinHandle<Result<(), Error>>> = Vec::new();
    for link in links.iter() {
        let join = tokio::spawn(create_link(link, port));
        joins.push(join);
    }
    for join in joins {
        let ret = join.await;
        if ret.is_err() {
            error!("{}", ret.unwrap_err());
        }
    }
}

/// begin a connect
async fn create_link(link: &Link, port: u16) -> Result<(), Error> {
    let stream = connect_with_timeout(&link.remote_host, port).await?;

    let mut frame_stream = FrameStream::new(stream);

    let _ = auth(&mut frame_stream).await?;

    let _ = init_port(&mut frame_stream, link).await?;

    loop {
        // sure connection is established
        let ret = frame_stream.send(&Message::Heartbeat).await;

        if ret.is_err() {
            return Err(Error::new(ErrorKind::ConnectionAborted, "heartbeat failed"));
        }

        let msg = frame_stream.recv_timeout().await;
        if msg.is_err() {
            continue;
        }

        match msg.unwrap() {
            Message::InitPort(_) => info!("unexpected init"),
            Message::Auth(_) => warn!("unexpected auth"),
            Message::Heartbeat => trace!("server check heartbeat"),
            Message::Error(e) => {
                return Err(Error::new(ErrorKind::Other, e));
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
async fn auth(frame_stream: &mut FrameStream) -> Result<(), Error> {
    let secret = &G_CFG.get().unwrap().secret;
    if secret.is_none() {
        return Ok(());
    }
    let secret = secret.as_ref().unwrap();

    frame_stream.send(&Message::Auth(secret.clone())).await?;

    let msg = frame_stream.recv_timeout().await?;
    match msg {
        Message::Auth(_) => Ok(()),
        Message::Error(e) => Err(Error::new(ErrorKind::PermissionDenied, e)),
        _ => Err(Error::new(ErrorKind::InvalidData, "unexpect msg")),
    }
}

/// send and recv InitPort message with server
async fn init_port(frame_stream: &mut FrameStream, link: &Link) -> Result<(), Error> {
    frame_stream
        .send(&Message::InitPort(link.remote_port))
        .await?;
    let msg = frame_stream.recv_timeout().await?;
    match msg {
        Message::InitPort(port) => {
            info!(
                "{}:{} link to {}:{}",
                link.local_host, link.local_port, link.remote_host, port
            );
            Ok(())
        }
        Message::Error(e) => Err(Error::new(ErrorKind::Other, e)),
        _ => Err(Error::new(ErrorKind::InvalidData, "unexpect msg")),
    }
}

/// create a TcpStream from to:port
async fn connect_with_timeout(addr: &str, port: u16) -> Result<TcpStream, Error> {
    let ret = timeout(NETWORK_TIMEOUT, TcpStream::connect((addr, port))).await;
    if ret.is_err() {
        return Err(Error::new(ErrorKind::TimedOut, "timeout"));
    }
    ret.unwrap()
}

/// deal connection from server proxy port
async fn handle_proxy_connection(id: Uuid, link: &Link) -> Result<(), Error> {
    let stream = connect_with_timeout(&link.remote_host, G_CFG.get().unwrap().port).await?;
    let mut frame_stream = FrameStream::new(stream);

    auth(&mut frame_stream).await?;

    frame_stream.send(&Message::Connect(id)).await?;

    let local = connect_with_timeout(&link.local_host, link.local_port).await?;

    proxy(local, frame_stream.stream()).await?;

    Ok(())
}
