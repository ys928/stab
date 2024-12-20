//! give some generic code

use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream},
    StreamExt,
};
use serde::{Deserialize, Serialize};
use tokio::{io::copy_bidirectional, net::TcpStream, time::timeout};
use tokio_util::codec::{AnyDelimiterCodec, Framed};
/// Timeout for network connections.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);

/// Messages exchanged between the Local and the server
#[derive(Debug, Serialize, Deserialize)]
pub enum Msg {
    /// init connect,specify port and auth
    #[serde(rename = "I")]
    InitPort(u16, Option<String>),

    /// Accepts an incoming TCP connection, using this stream as a proxy, and auth.
    #[serde(rename = "C")]
    Connect(u16, Option<String>),

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

    /// get the TcpStream
    pub fn stream(self) -> TcpStream {
        self.sender.reunite(self.receiver).unwrap().into_parts().io
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
pub async fn proxy(mut stream1: TcpStream, mut stream2: TcpStream) -> Result<(u64, u64)> {
    let (s1, s2) = copy_bidirectional(&mut stream1, &mut stream2).await?;
    Ok((s1, s2))
}
