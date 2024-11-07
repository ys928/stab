//! the server mode code

use anyhow::{anyhow, Context, Result};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use crate::config::G_CFG;
use crate::control::CtlConns;
use crate::data_conn::DataConns;
use crate::share::{proxy, FrameStream, Message, NETWORK_TIMEOUT};
use chrono::Local;
use log::{error, info, trace, warn};
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout};
use tracing::{debug, debug_span, Instrument};
use uuid::Uuid;

/// connection information
#[derive(Debug, Deserialize, Serialize)]
pub struct CtlConInfo {
    /// server port
    pub port: u16,
    /// src address
    pub src: String,
    /// begin time
    pub time: String,
    /// transmission data size
    pub data: u64,
}

/// Links for transferring data.
#[derive(Debug)]
pub struct DataConn {
    /// Subordinate Port
    pub port: u16,
    /// connection
    pub stream: TcpStream,
}

/// Concurrent map of IDs to incoming connections.
static DATA_CONNS: OnceLock<DataConns> = OnceLock::new();

/// All control connect
pub static CTL_CONNS: OnceLock<CtlConns> = OnceLock::new();

/// current port number
static PORT_IDX: AtomicU16 = AtomicU16::new(0);

/// Start the server, listening for new control connections.
pub async fn run() {
    CTL_CONNS.set(CtlConns::new()).unwrap();

    DATA_CONNS.set(DataConns::new()).unwrap();

    let addr = format!("0.0.0.0:{}", G_CFG.get().unwrap().port);

    let control_listener = TcpListener::bind(&addr).await;

    let Ok(control_listener) = control_listener else {
        error!("{}", control_listener.unwrap_err());
        return;
    };

    info!("server listening {}", addr);

    loop {
        let ret = control_listener.accept().await;

        let Ok((stream, addr)) = ret else {
            error!("failed to accept client {}", ret.unwrap_err());
            continue;
        };

        tokio::spawn(
            async move {
                info!("incoming control connection");
                if let Err(err) = handle_control_connection(stream, addr).await {
                    warn!("control connection {:?} exited with error：{}", addr, err);
                } else {
                    info!("control connection {:?} exited", addr);
                }
            }
            .instrument(debug_span!("conn", id = Uuid::new_v4().to_string())),
        );
    }
}

/// deal with control connection
async fn handle_control_connection(stream: TcpStream, addr: SocketAddr) -> Result<()> {
    let mut frame_stream = FrameStream::new(stream);

    // authentication client
    auth(&mut frame_stream).await?;

    let msg = frame_stream.recv_timeout().await?;
    match msg {
        Message::InitPort(port) => {
            let listener = init_port(&mut frame_stream, port, addr)
                .await
                .context("init port failed")?;

            let port = listener.local_addr().unwrap().port();

            let ret = enter_control_loop(listener, &mut frame_stream, port, addr).await;
            CTL_CONNS.get().unwrap().remove(port).await;
            ret?
        }
        Message::Connect(id) => {
            let conn = DATA_CONNS.get().unwrap().remove(id).await;

            if conn.is_none() {
                warn!("missing connection");
            } else {
                let stream2 = conn.unwrap();
                let stream1 = frame_stream.stream();
                let size = proxy(stream1, stream2.stream).await?;
                CTL_CONNS.get().unwrap().add_data(stream2.port, size);
            }
        }
        Message::Auth(_) => {
            frame_stream
                .send(&Message::Error("unexpected auth".to_string()))
                .await?;
            return Err(anyhow!("unexpect auth message"));
        }
        _ => {
            warn!("unexpect message: {:?}", msg);
            return Err(anyhow!("unexpect msg"));
        }
    }
    Ok(())
}

/// deal with InitPort message from client
async fn init_port(
    frame_stream: &mut FrameStream,
    port: u16,
    addr: SocketAddr,
) -> Result<TcpListener> {
    let listener = match create_listener(port).await {
        Ok(listener) => listener,
        Err(e) => {
            frame_stream
                .send(&Message::Error(format!("create control port failed:{}", e)))
                .await?;
            error!("{}", e);
            return Err(anyhow!("{}", e));
        }
    };
    let port = listener.local_addr().unwrap().port();
    info!("new client {}", port);

    frame_stream
        .send(&Message::InitPort(port))
        .await
        .context("send init port failed")?;

    let date = Local::now();
    let time = date.format("%Y-%m-%d %H:%M:%S").to_string();
    let ctl = CtlConInfo {
        port,
        src: addr.to_string(),
        time,
        data: 0,
    };
    CTL_CONNS.get().unwrap().insert(port, ctl).await;
    Ok(listener)
}

/// Handle the establishment of data links corresponding to each control port
async fn enter_control_loop(
    listener: TcpListener,
    frame_stream: &mut FrameStream,
    port: u16,
    addr: SocketAddr,
) -> Result<()> {
    loop {
        // if not existing,exit immediately
        let exist = CTL_CONNS.get().unwrap().contain(port).await;

        if !exist {
            frame_stream
                .send(&Message::Error("server closed this connection".to_string()))
                .await
                .context("send close this connection failed")?;
            return Ok(());
        }

        // check connect is ok
        frame_stream
            .send(&Message::Heartbeat)
            .await
            .context("send heartbeat failed")?;

        // try to recv the client's heartbeat
        let msg = frame_stream
            .recv_self_timeout(Duration::from_millis(200))
            .await;
        if msg.is_ok() {
            trace!("{} >> {:?}", addr.to_string(), msg.unwrap());
        }

        let proxy_conn = timeout(NETWORK_TIMEOUT, listener.accept()).await;
        let Ok(proxy_conn) = proxy_conn else {
            debug!("{}", proxy_conn.unwrap_err());
            continue;
        };

        let (stream, addr) = proxy_conn.context("accept data connect faild")?;

        info!("new connection {}:{}", addr, port);

        let id = Uuid::new_v4();
        DATA_CONNS
            .get()
            .unwrap()
            .insert(id, DataConn { port, stream })
            .await;

        tokio::spawn(async move {
            // Remove stale entries to avoid memory leaks.
            sleep(Duration::from_secs(15)).await;

            if DATA_CONNS.get().unwrap().remove(id).await.is_some() {
                warn!("removed stale connection {}", id);
            }
        });

        frame_stream
            .send(&Message::Connect(id))
            .await
            .context("send connect msg failed")?;
    }
}

/// authenticate client
async fn auth(frame_stream: &mut FrameStream) -> Result<()> {
    let secret = &G_CFG.get().unwrap().secret;
    if secret.is_none() {
        return Ok(());
    }
    let secret = secret.as_ref().unwrap();
    let msg = frame_stream.recv_timeout().await?;
    match msg {
        Message::Auth(token) => {
            if token.cmp(secret).is_eq() {
                frame_stream.send(&Message::Auth(token)).await?;
                return Ok(());
            } else {
                frame_stream
                    .send(&Message::Error("auth failed".to_string()))
                    .await?;
                return Err(anyhow!("auth failed,valid secret:{}", secret));
            }
        }
        _ => {
            frame_stream
                .send(&Message::Error("auth failed".to_string()))
                .await?;
            return Err(anyhow!("auth failed,unexpected message!"));
        }
    }
}

/// create a tcp listener for a port
async fn create_listener(port: u16) -> Result<TcpListener> {
    let port_range = &G_CFG.get().unwrap().port_range;
    if port > 0 {
        // Client requests a specific port number.
        if !port_range.contains(&port) {
            return Err(anyhow!("port not in range"));
        }
        return try_bind(port).await;
    }

    // Client requests any available port in range.
    let mut port = PORT_IDX.load(Ordering::Relaxed);
    let mut n = 0;
    loop {
        if !port_range.contains(&port) {
            port = port_range.start;
        }
        n += 1;

        if n >= port_range.len() {
            PORT_IDX.store(port_range.start, Ordering::Relaxed);
            return Err(anyhow!("not find port"));
        }
        let ret = try_bind(port).await;
        if ret.is_err() {
            port += 1;
            continue;
        }
        PORT_IDX.store(port + 1, Ordering::Relaxed);
        return ret;
    }
}

/// try to bind a port and return TcpListener
async fn try_bind(port: u16) -> Result<TcpListener> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    Ok(listener)
}
