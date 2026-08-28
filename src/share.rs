//! give some generic code

use std::{
    io::{self, IoSlice},
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
    time::Duration,
};

use anyhow::{bail, Context as _, Result};
use futures::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream},
    StreamExt,
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_util::codec::{AnyDelimiterCodec, Framed};
/// Timeout for network connections.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the server waits to pair a public client with a local work connection.
pub const PAIR_TIMEOUT: Duration = Duration::from_secs(15);

/// How long either side waits without a peer heartbeat before treating the
/// control channel as dead (default; server may override via config).
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(60);

/// Server → client heartbeat interval.
pub const HEARTBEAT_INTERVAL_SERVER: Duration = Duration::from_secs(15);

/// Client → server heartbeat interval.
pub const HEARTBEAT_INTERVAL_CLIENT: Duration = Duration::from_secs(3);

/// Messages exchanged between the Local and the server
#[derive(Debug, Serialize, Deserialize)]
pub enum Msg {
    /// init connect,specify port and auth
    #[serde(rename = "I")]
    InitPort(u16, Option<String>),

    /// Accepts an incoming TCP connection, using this stream as a proxy, and auth.
    #[serde(rename = "C")]
    Connect(u16, Option<String>),

    /// Server signals that a client was paired; local should connect to the target.
    #[serde(rename = "S")]
    Start,

    /// Heartbeat to sure connection is ok
    #[serde(rename = "H")]
    Heartbeat,

    /// error info
    #[serde(rename = "E")]
    Error(String),
}

/// frame stream, used to send/recv a message
pub struct FrameStream {
    sender: SplitSink<Framed<TcpStream, AnyDelimiterCodec>, String>,
    receiver: SplitStream<Framed<TcpStream, AnyDelimiterCodec>>,
}

/// frame sender
pub struct FrameSender {
    sender: SplitSink<Framed<TcpStream, AnyDelimiterCodec>, String>,
}

/// frame receiver
pub struct FrameReceiver {
    receiver: SplitStream<Framed<TcpStream, AnyDelimiterCodec>>,
}

impl FrameStream {
    /// create a new frame stream
    pub fn new(stream: TcpStream) -> Self {
        let codec = AnyDelimiterCodec::new(b"\0".to_vec(), b"\0".to_vec());
        let frame = Framed::new(stream, codec);
        let (sender, receiver) = frame.split::<String>();
        Self { sender, receiver }
    }

    /// send message as frame
    pub async fn send(&mut self, msg: &Msg) -> Result<()> {
        self.sender.send(serde_json::to_string(msg)?).await?;
        Ok(())
    }

    /// recv message as frame
    pub async fn recv(&mut self) -> Result<Msg> {
        if let Some(msg) = self.receiver.next().await {
            let byte_msg = msg.context("recv frame failed")?;
            let msg = serde_json::from_slice(&byte_msg).context("invalid msg")?;
            Ok(msg)
        } else {
            bail!("no recv msg");
        }
    }

    /// recv message within the specified time
    pub async fn recv_timeout(&mut self) -> Result<Msg> {
        let msg = timeout(NETWORK_TIMEOUT, self.recv()).await??;
        Ok(msg)
    }

    /// split to sender and receiver
    pub fn split(self) -> (FrameSender, FrameReceiver) {
        (
            FrameSender {
                sender: self.sender,
            },
            FrameReceiver {
                receiver: self.receiver,
            },
        )
    }

    /// Convert back to a raw `TcpStream`, preserving any bytes already buffered
    /// by the codec (e.g. an SSH banner that arrived with the Connect frame).
    pub fn into_tcp_stream(self) -> (TcpStream, Vec<u8>) {
        let parts = self.sender.reunite(self.receiver).unwrap().into_parts();
        (parts.io, parts.read_buf.to_vec())
    }
}

impl FrameSender {
    /// send message as frame
    pub async fn send(&mut self, msg: &Msg) -> Result<()> {
        self.sender.send(serde_json::to_string(msg)?).await?;
        Ok(())
    }
}

impl FrameReceiver {
    /// recv message as frame
    pub async fn recv(&mut self) -> Result<Msg> {
        if let Some(msg) = self.receiver.next().await {
            let byte_msg = msg.context("recv frame failed")?;
            let msg = serde_json::from_slice(&byte_msg).context("invalid msg")?;
            Ok(msg)
        } else {
            bail!("no recv msg");
        }
    }
}

/// Copy data mutually between two Tcpstreams.
pub async fn proxy(stream1: TcpStream, stream2: TcpStream) -> Result<(u64, u64)> {
    proxy_with_stats(stream1, stream2, &[], None, None).await
}

/// Like [`proxy`], but first writes `prepend` to `stream1`.
///
/// `prepend` is data already read from `stream2` (codec leftover) that must
/// still be delivered to the peer on `stream1`.
pub async fn proxy_with_prepend(
    stream1: TcpStream,
    stream2: TcpStream,
    prepend: &[u8],
) -> Result<(u64, u64)> {
    proxy_with_stats(stream1, stream2, prepend, None, None).await
}

/// Like [`proxy_with_prepend`], incrementing `down`/`up` as bytes are forwarded.
///
/// `down` counts `stream1 → stream2` (public client toward the tunnel).
/// `up` counts `stream2 → stream1` (tunnel toward the public client).
/// Counters are updated while the copy runs, including if it later errors.
pub async fn proxy_with_stats(
    mut stream1: TcpStream,
    mut stream2: TcpStream,
    prepend: &[u8],
    down: Option<&AtomicU64>,
    up: Option<&AtomicU64>,
) -> Result<(u64, u64)> {
    let mut extra = 0u64;
    if !prepend.is_empty() {
        stream1.write_all(prepend).await?;
        extra = prepend.len() as u64;
        if let Some(up) = up {
            up.fetch_add(extra, Ordering::Relaxed);
        }
    }
    let (s1, s2) = copy_bidirectional_counted(&mut stream1, &mut stream2, down, up).await?;
    Ok((s1, s2 + extra))
}

/// Writer that adds successfully written bytes to an optional live counter.
struct CountedWriter<'a, W> {
    inner: W,
    counter: Option<&'a AtomicU64>,
}

impl<'a, W> CountedWriter<'a, W> {
    fn add(&self, n: usize) {
        if n > 0 {
            if let Some(counter) = self.counter {
                counter.fetch_add(n as u64, Ordering::Relaxed);
            }
        }
    }
}

impl<'a, W: AsyncWrite + Unpin> AsyncWrite for CountedWriter<'a, W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let n = match Pin::new(&mut self.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => n,
            other => return other,
        };
        self.add(n);
        Poll::Ready(Ok(n))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let n = match Pin::new(&mut self.inner).poll_write_vectored(cx, bufs) {
            Poll::Ready(Ok(n)) => n,
            other => return other,
        };
        self.add(n);
        Poll::Ready(Ok(n))
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

async fn copy_bidirectional_counted(
    a: &mut TcpStream,
    b: &mut TcpStream,
    a_to_b: Option<&AtomicU64>,
    b_to_a: Option<&AtomicU64>,
) -> io::Result<(u64, u64)> {
    let (mut ra, wa) = a.split();
    let (mut rb, wb) = b.split();
    let mut wb = CountedWriter {
        inner: wb,
        counter: a_to_b,
    };
    let mut wa = CountedWriter {
        inner: wa,
        counter: b_to_a,
    };

    let a_to_b_fut = async {
        let n = tokio::io::copy(&mut ra, &mut wb).await?;
        wb.shutdown().await?;
        Ok(n)
    };
    let b_to_a_fut = async {
        let n = tokio::io::copy(&mut rb, &mut wa).await?;
        wa.shutdown().await?;
        Ok(n)
    };

    tokio::try_join!(a_to_b_fut, b_to_a_fut)
}
