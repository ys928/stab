//! the server mode code

use anyhow::{anyhow, bail, Context, Result};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicU16, Ordering},
        OnceLock,
    },
    time::Duration,
};
use tokio::sync::mpsc::unbounded_channel;

use crate::share::{FrameStream, Msg, NETWORK_TIMEOUT};
use crate::{config::G_CFG, tcp_pool::TcpPool};
use crate::{control::CtlConns, share::proxy};
use chrono::Local;
use log::{error, info, trace, warn};
use serde::{Deserialize, Serialize};
use tokio::{
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};
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

/// tcp stream pool
static TCP_POOL: OnceLock<TcpPool> = OnceLock::new();

/// All control connect
pub static CTL_CONNS: OnceLock<CtlConns> = OnceLock::new();

/// current port number
static PORT_IDX: AtomicU16 = AtomicU16::new(0);

/// Start the server, listening for new control connections.
pub async fn run() {
    CTL_CONNS.set(CtlConns::new()).unwrap();

    TCP_POOL.set(TcpPool::new()).unwrap();

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

    let msg = frame_stream.recv_timeout().await?;
    match msg {
        Msg::InitPort(port, secret) => {
            if !auth(&secret) {
                frame_stream
                    .send(&Msg::Error("auth failed".to_string()))
                    .await?;
                bail!("auth failed:{} {:?} {:?}", port, addr, secret);
            }
            let listener = init_port(&mut frame_stream, port, addr)
                .await
                .context("init port failed")?;

            let port = listener.local_addr().unwrap().port();

            let ret = enter_control_loop(listener, frame_stream, port, addr).await;
            CTL_CONNS.get().unwrap().remove(port).await;
            TCP_POOL.get().unwrap().remove(port).await;
            ret?
        }
        Msg::Connect(port, secret) => {
            if !auth(&secret) {
                frame_stream
                    .send(&Msg::Error("auth failed".to_string()))
                    .await?;
                bail!("auth failed:{} {:?} {:?}", port, addr, secret);
            }

            TCP_POOL
                .get()
                .unwrap()
                .add_tcp_stream(port, frame_stream.stream())
                .await;
        }
        _ => {
            bail!("unexpect msg:{:?}", msg);
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
                .send(&Msg::Error(format!("create control port failed:{}", e)))
                .await?;
            error!("{}", e);
            return Err(anyhow!("{}", e));
        }
    };
    let port = listener.local_addr().unwrap().port();
    info!("new client {}", port);

    frame_stream
        .send(&Msg::InitPort(port, None))
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
    frame_stream: FrameStream,
    port: u16,
    addr: SocketAddr,
) -> Result<()> {
    let (msg_sender, mut msg_recv) = unbounded_channel();

    let (mut frame_sender, mut frame_receiver) = frame_stream.split();

    tokio::spawn(async move {
        // try to recv the client's heartbeat
        while let Ok(_) = frame_receiver.recv().await {
            trace!("{} >> heartbeat", addr.to_string());
        }
    });

    // send msg to client
    tokio::spawn(async move {
        // init tcp stream pool
        let pool_size = G_CFG.get().unwrap().pool_size as usize;
        for _ in 0..pool_size {
            if let Err(e) = frame_sender.send(&Msg::Connect(port, None)).await {
                warn!("send msg failed:{}", e);
                break;
            }
        }

        while let Some(msg) = msg_recv.recv().await {
            if let Err(e) = frame_sender.send(&msg).await {
                warn!("send msg failed:{}", e);
                break;
            }
        }
    });

    let msg_sender_clone = msg_sender.clone();

    //Heartbeat packet is sent every 15 seconds
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(15)).await;
            if let Err(e) = msg_sender_clone.send(Msg::Heartbeat) {
                warn!("send heartbeat failed: {}", e);
                break;
            }
        }
    });

    loop {
        // if not existing,exit immediately
        let exist = CTL_CONNS.get().unwrap().contain(port).await;

        if !exist || msg_sender.is_closed() {
            return Ok(());
        }

        let proxy_conn = timeout(NETWORK_TIMEOUT, listener.accept()).await;
        let Ok(proxy_conn) = proxy_conn else {
            debug!("{}", proxy_conn.unwrap_err());
            continue;
        };

        let (stream, addr) = proxy_conn.context("accept data connect faild")?;

        info!("new connection {}:{}", addr, port);

        let msg_sender_clone = msg_sender.clone();

        if msg_sender.send(Msg::Connect(port, None)).is_err() {
            return Ok(());
        };

        tokio::spawn(async move {
            loop {
                let tcp_pool = TCP_POOL.get().unwrap().get_tcp_stream(port).await;
                let Some(proxy_stream) = tcp_pool else {
                    if msg_sender_clone.send(Msg::Connect(port, None)).is_err() {
                        break;
                    };
                    sleep(Duration::from_millis(100)).await;
                    continue;
                };

                let byte_num = proxy(stream, proxy_stream).await;
                if let Ok(byte_num) = byte_num {
                    CTL_CONNS.get().unwrap().add_data(port, byte_num);
                }
                break;
            }
        });
    }
}

/// authenticate client
fn auth(local_secret: &Option<String>) -> bool {
    let server_secret = &G_CFG.get().unwrap().secret;
    if local_secret.is_none() && server_secret.is_none() {
        return true;
    }
    if server_secret.is_some() && local_secret.is_some() {
        let server_secret = server_secret.as_ref().unwrap();
        let local_secret = local_secret.as_ref().unwrap();
        if server_secret.eq(local_secret) {
            return true;
        }
    }
    false
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
