//! give some generic code

use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures::{sink::SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{io, net::TcpStream, time::timeout};
use tokio_util::codec::{AnyDelimiterCodec, Framed};
use uuid::Uuid;
/// Timeout for network connections.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);

/// Messages exchanged between the Local and the server
#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    /// init connect and specify port
    InitPort(u16),

    /// auth connect
    Auth(String),

    /// Accepts an incoming TCP connection, using this stream as a proxy.
    Connect(Uuid),

    /// sure connection is ok
    Heartbeat,

    /// error info
    Error(String),
}

/// frame stream, used to send/recv a message
pub struct FrameStream(Framed<TcpStream, AnyDelimiterCodec>);

impl FrameStream {
    /// create a new frame stream
    pub fn new(stream: TcpStream) -> Self {
        let codec = AnyDelimiterCodec::new(b"\0".to_vec(), b"\0".to_vec());
        Self(Framed::new(stream, codec))
    }

    /// send message as frame
    pub async fn send(&mut self, msg: &Message) -> Result<()> {
        self.0.send(serde_json::to_string(msg)?).await?;
        Ok(())
    }

    /// recv message as frame
    pub async fn recv(&mut self) -> Result<Message> {
        if let Some(msg) = self.0.next().await {
            let byte_msg = msg.context("recv frame failed")?;
            let msg = serde_json::from_slice(&byte_msg).context("invalid msg")?;
            Ok(msg)
        } else {
            bail!("no recv msg");
        }
    }

    /// recv message within the specified time
    pub async fn recv_timeout(&mut self) -> Result<Message> {
        let msg = timeout(NETWORK_TIMEOUT, self.recv()).await??;
        Ok(msg)
    }

    /// recv message within the customer time
    pub async fn recv_self_timeout(&mut self, time: Duration) -> Result<Message> {
        let msg = timeout(time, self.recv()).await??;
        Ok(msg)
    }

    /// get the TcpStream
    pub fn stream(self) -> TcpStream {
        self.0.into_parts().io
    }
}

/// Copy data mutually between two Tcpstreams.
pub async fn proxy(mut stream1: TcpStream, mut stream2: TcpStream) -> Result<u64> {
    let (mut s1_read, mut s1_write) = stream1.split();
    let (mut s2_read, mut s2_write) = stream2.split();
    let bytes = tokio::select! {
        res = io::copy(&mut s1_read, &mut s2_write) => res,
        res = io::copy(&mut s2_read, &mut s1_write) => res,
    }?;
    Ok(bytes)
}
