//! the server mode code

use anyhow::{anyhow, bail, Context, Result};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU16, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedSender},
    oneshot,
};

use crate::share::{proxy_with_prepend, FrameStream, Msg, NETWORK_TIMEOUT, PAIR_TIMEOUT};
use crate::{config::G_CFG, tcp_pool::TcpPool};
use crate::{control::CtlConns};
use chrono::Local;
use serde::{Deserialize, Serialize};
use tokio::{
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};
use tracing::{debug, debug_span, error, info, trace, warn, Instrument};
use uuid::Uuid;

/// connection information
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CtlConInfo {
    /// server port
    pub port: u16,
    /// src address
    pub src: String,
    /// begin time
    pub time: String,
    /// upstream data size
    pub upstream: u64,
    /// downstream data size
    pub downstream: u64,
    /// transmission data size
    pub total: u64,
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
                info!("incoming connection");
                if let Err(err) = handle_control_connection(stream, addr).await {
                    warn!("connection {:?} exited with error：{}", addr, err);
                } else {
                    info!("connection {:?} exited", addr);
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
            CTL_CONNS.get().unwrap().remove(port);
            TCP_POOL.get().unwrap().remove(port);
            ret?
        }
        Msg::Connect(port, secret) => {
            if !auth(&secret) {
                frame_stream
                    .send(&Msg::Error("auth failed".to_string()))
                    .await?;
                bail!("auth failed:{} {:?} {:?}", port, addr, secret);
            }

            // Keep the framed stream in the pool until a client is paired and
            // we send Msg::Start — so local does not dial the target early
            // (which breaks SSH and other server-speaks-first protocols).
            TCP_POOL.get().unwrap().add_frame_stream(port, frame_stream);
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
        upstream: 0,
        downstream: 0,
        total: 0,
    };
    CTL_CONNS.get().unwrap().insert(port, ctl);
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

    // So accept-path waiters see Some(None) instead of None before the first
    // work connection is registered.
    TCP_POOL.get().unwrap().ensure_port(port);

    let is_exit = Arc::new(AtomicBool::new(false));
    let is_exit_clone = is_exit.clone();
    tokio::spawn(async move {
        // try to recv the client's heartbeat
        while let Ok(_) = frame_receiver.recv().await {
            trace!("{} >> heartbeat", addr.to_string());

            let is_exit = is_exit.load(Ordering::Relaxed);
            if is_exit {
                info!("recv msg loop exit:{}", port);
                break;
            }
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
            let Some(msg) = msg else {
                info!("send msg loop exit:{}", port);
                break;
            };

            if let Err(e) = frame_sender.send(&msg).await {
                warn!("send msg loop exit:{},err:{}", port, e);
                break;
            }
        }
        is_exit_clone.store(true, Ordering::Relaxed);
    });

    let msg_sender_clone = msg_sender.clone();

    //Heartbeat packet is sent every 15 seconds
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(15)).await;
            if let Err(e) = msg_sender_clone.send(Some(Msg::Heartbeat)) {
                info!("send heartbeat loop exit:{} err:{}", port, e);
                break;
            }
        }
    });

    loop {
        // if not existing,exit immediately
        let exist = CTL_CONNS.get().unwrap().contain(port);

        if !exist || msg_sender.is_closed() {
            let _ = msg_sender.send(None);
            break;
        }

        let proxy_conn = timeout(NETWORK_TIMEOUT, listener.accept()).await;
        let Ok(proxy_conn) = proxy_conn else {
            debug!("{}", proxy_conn.unwrap_err());
            continue;
        };

        let (stream, addr) = proxy_conn.context("accept data connect faild")?;

        info!("new connection {}:{}", addr, port);

        let msg_sender = msg_sender.clone();
        tokio::spawn(async move {
            if let Err(e) = pair_and_proxy(stream, port, msg_sender).await {
                warn!("proxy on port {} exited: {}", port, e);
            }
        });
    }

    info!("control connect exit:{}", port);

    Ok(())
}

/// Pair a public client TCP stream with a local work connection and proxy.
async fn pair_and_proxy(
    client: TcpStream,
    port: u16,
    msg_sender: UnboundedSender<Option<Msg>>,
) -> Result<()> {
    let pool = TCP_POOL.get().unwrap();

    // Prefer a live idle stream to skip a round-trip. Dead NAT-killed sockets
    // fail Start quickly (timeout); then flush the rest of the idle queue.
    while let Some(mut frame_stream) = pool.get_frame_stream(port) {
        match timeout(NETWORK_TIMEOUT, frame_stream.send(&Msg::Start)).await {
            Ok(Ok(())) => {
                // Refill when we consume a pre-pooled connection.
                let _ = msg_sender.send(Some(Msg::Connect(port, None)));
                return finish_proxy(port, client, frame_stream).await;
            }
            Ok(Err(e)) => warn!("pooled Start failed on port {}: {}", port, e),
            Err(_) => warn!("pooled Start timed out on port {}", port),
        }
        pool.clear_idle(port);
        break;
    }

    // Demand a fresh work connection and wait for that specific dial.
    let (tx, rx) = oneshot::channel();
    pool.add_waiter(port, tx);

    if msg_sender.send(Some(Msg::Connect(port, None))).is_err() {
        bail!("control channel closed");
    }

    let mut frame_stream = timeout(PAIR_TIMEOUT, rx)
        .await
        .context("timeout waiting for proxy stream")?
        .map_err(|_| anyhow!("work connection waiter dropped"))?;

    timeout(NETWORK_TIMEOUT, frame_stream.send(&Msg::Start))
        .await
        .context("timeout sending Start")?
        .context("send Start failed")?;

    finish_proxy(port, client, frame_stream).await
}

async fn finish_proxy(port: u16, client: TcpStream, frame_stream: FrameStream) -> Result<()> {
    let (proxy_stream, head) = frame_stream.into_tcp_stream();
    let (down, up) = proxy_with_prepend(client, proxy_stream, &head).await?;
    CTL_CONNS.get().unwrap().add_data(port, up, down);
    Ok(())
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

/// Atomically claim the next candidate port in `port_range`.
fn claim_next_port(port_range: &std::ops::RangeInclusive<u16>) -> u16 {
    let start = *port_range.start();
    let end = *port_range.end();
    loop {
        let cur = PORT_IDX.load(Ordering::Relaxed);
        let port = if port_range.contains(&cur) { cur } else { start };
        let next = if port >= end { start } else { port + 1 };
        if PORT_IDX
            .compare_exchange_weak(cur, next, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            return port;
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
    // Each candidate is claimed atomically so concurrent allocators do not
    // share the same scan cursor (which could falsely report "not find port").
    for _ in 0..port_range.len() {
        let port = claim_next_port(port_range);
        if let Ok(listener) = try_bind(port).await {
            return Ok(listener);
        }
    }
    Err(anyhow!("not find port"))
}

/// try to bind a port and return TcpListener
async fn try_bind(port: u16) -> Result<TcpListener> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    Ok(listener)
}
