//! the server mode code

use std::io::{Error, ErrorKind};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::config::G_CFG;
use crate::share::{proxy, FrameStream, Message, NETWORK_TIMEOUT};
use chrono::Local;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// connection information
#[derive(Deserialize, Serialize)]
pub struct CtlConInfo {
    /// server port
    pub port: u16,
    /// src address
    pub src: String,
    /// begin time
    pub time: String,
}

/// Concurrent map of IDs to incoming connections.
static CLI_CONNS: Mutex<Option<HashMap<Uuid, TcpStream>>> = Mutex::new(None);

/// All control connect
pub static CTL_CONNS: Mutex<Option<HashMap<u16, CtlConInfo>>> = Mutex::new(None);

/// current port number
static PORT_IDX: AtomicU16 = AtomicU16::new(0);

/// Start the server, listening for new control connections.
pub async fn run() -> Result<(), Error> {
    {
        let mut conns = CLI_CONNS.lock().unwrap();
        *conns = Some(HashMap::new());
        let mut ctl_conns = CTL_CONNS.lock().unwrap();
        *ctl_conns = Some(HashMap::new());
    }

    let addr = format!("0.0.0.0:{}", G_CFG.get().unwrap().contrl_port);

    let control_listener = TcpListener::bind(&addr).await?;

    info!("server listening {}", addr);

    loop {
        let (stream, addr) = control_listener.accept().await?;

        tokio::spawn(async move {
            info!("incoming control connection");
            if let Err(err) = handle_control_connection(stream, addr).await {
                warn!("control connection {:?} exited with error：{}", addr, err);
            } else {
                info!("control connection {:?} exited", addr);
            }
        });
    }
}

/// deal with control connection
async fn handle_control_connection(stream: TcpStream, addr: SocketAddr) -> Result<(), Error> {
    let mut frame_stream = FrameStream::new(stream);

    // authentication client
    auth(&mut frame_stream).await?;

    let msg = frame_stream.recv_timeout().await?;
    match msg {
        Message::InitPort(port) => {
            let ret = init_port(&mut frame_stream, port, addr).await;
            let mut ctl_conns = CTL_CONNS.lock().unwrap();
            ctl_conns.as_mut().unwrap().remove(&port);
            ret?
        }
        Message::Connect(id) => {
            let conn = {
                let mut conns = CLI_CONNS.lock().unwrap();
                conns.as_mut().unwrap().remove(&id)
            };
            if conn.is_none() {
                warn!("missing connection");
            } else {
                let stream2 = conn.unwrap();
                let stream1 = frame_stream.stream();
                proxy(stream1, stream2).await?;
            }
        }
        Message::Auth(_) => {
            frame_stream
                .send(&Message::Error("unexpected auth".to_string()))
                .await?;
            return Err(Error::new(ErrorKind::InvalidData, "unexpect auth message"));
        }
        _ => {
            warn!("unexpect message: {:?}", msg);
            return Err(Error::new(ErrorKind::InvalidData, "unexpect msg"));
        }
    }
    Ok(())
}

/// deal with InitPort message from client
async fn init_port(
    frame_stream: &mut FrameStream,
    port: u16,
    addr: SocketAddr,
) -> Result<(), Error> {
    let listener = match create_listener(port).await {
        Ok(listener) => listener,
        Err(e) => {
            frame_stream.send(&Message::Error(e.to_string())).await?;
            return Ok(());
        }
    };
    let port = listener.local_addr().unwrap().port();
    info!("new client {}", port);
    {
        let mut ctl_conns = CTL_CONNS.lock().unwrap();
        let date = Local::now();
        let time = date.format("%Y-%m-%d %H:%M:%S").to_string();
        ctl_conns.as_mut().unwrap().insert(
            port,
            CtlConInfo {
                port,
                src: addr.to_string(),
                time,
            },
        );
    }

    frame_stream.send(&Message::InitPort(port)).await?;

    loop {
        // if not existing,exit immediately
        let exist = {
            let ctl_conns = CTL_CONNS.lock().unwrap();
            ctl_conns.as_ref().unwrap().contains_key(&port)
        };

        if !exist {
            frame_stream
                .send(&Message::Error("server closed this connection".to_string()))
                .await?;
            return Ok(());
        }

        // check connect is ok
        frame_stream.send(&Message::Heartbeat).await?;

        let proxy_conn = timeout(NETWORK_TIMEOUT, listener.accept()).await;
        if proxy_conn.is_err() {
            continue;
        }

        let (stream2, addr) = proxy_conn.unwrap()?;

        info!("new connection {}:{}", addr, port);

        let id = Uuid::new_v4();
        {
            let mut conns = CLI_CONNS.lock().unwrap();
            conns.as_mut().unwrap().insert(id, stream2);
        }

        tokio::spawn(async move {
            // Remove stale entries to avoid memory leaks.
            sleep(Duration::from_secs(15)).await;
            let mut conns = CLI_CONNS.lock().unwrap();
            if conns.as_mut().unwrap().remove(&id).is_some() {
                warn!("removed stale connection {}", id);
            }
        });

        frame_stream.send(&Message::Connect(id)).await?;
    }
}

/// authenticate client
async fn auth(frame_stream: &mut FrameStream) -> Result<(), Error> {
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
                return Err(Error::new(ErrorKind::PermissionDenied, "auth failed"));
            }
        }
        _ => {
            frame_stream
                .send(&Message::Error("auth failed".to_string()))
                .await?;
            return Err(Error::new(ErrorKind::PermissionDenied, "auth failed"));
        }
    }
}

/// create a tcp listener for a port
async fn create_listener(port: u16) -> Result<TcpListener, Error> {
    let port_range = &G_CFG.get().unwrap().port_range;
    if port > 0 {
        // Client requests a specific port number.
        if !port_range.contains(&port) {
            return Err(Error::new(ErrorKind::InvalidData, "port not in range"));
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
            return Err(Error::new(ErrorKind::Unsupported, "not find port"));
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
async fn try_bind(port: u16) -> Result<TcpListener, Error> {
    TcpListener::bind(("0.0.0.0", port)).await
}
