//! give some generic code

use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream},
    StreamExt,
};
use log::warn;
use serde::{Deserialize, Serialize};
use tokio::{io, net::TcpStream, time::timeout};
use tokio_util::codec::{AnyDelimiterCodec, Framed};
use uuid::Uuid;
/// Timeout for network connections.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);

/// Messages exchanged between the Local and the server
#[derive(Debug, Serialize, Deserialize)]
pub enum M {
    /// init connect and specify port
    I(u16),

    /// auth connect
    A(String),

    /// Accepts an incoming TCP connection, using this stream as a proxy.
    C(Uuid),

    /// Heartbeat to sure connection is ok
    H,

    /// error info
    E(String),
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
    pub async fn send(&mut self, msg: &M) -> Result<()> {
        self.sender.send(serde_json::to_string(msg)?).await?;
        Ok(())
    }

    /// recv message as frame
    pub async fn recv(&mut self) -> Result<M> {
        if let Some(msg) = self.receiver.next().await {
            let byte_msg = msg.context("recv frame failed")?;
            let msg = serde_json::from_slice(&byte_msg).context("invalid msg")?;
            Ok(msg)
        } else {
            bail!("no recv msg");
        }
    }

    /// send message as frame
    pub async fn send_timeout(&mut self, msg: &M) -> Result<()> {
        let ret = timeout(
            NETWORK_TIMEOUT,
            self.sender.send(serde_json::to_string(msg)?),
        )
        .await;
        let Ok(ret) = ret else {
            warn!("send msg timeout:{:?}", msg);
            return Ok(());
        };
        Ok(ret?)
    }

    /// recv message within the specified time
    pub async fn recv_timeout(&mut self) -> Result<M> {
        let msg = timeout(NETWORK_TIMEOUT, self.recv()).await??;
        Ok(msg)
    }

    /// recv message within the customer time
    pub async fn recv_self_timeout(&mut self, time: Duration) -> Result<M> {
        let msg = timeout(time, self.recv()).await??;
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
    pub async fn send(&mut self, msg: &M) -> Result<()> {
        self.sender.send(serde_json::to_string(msg)?).await?;
        Ok(())
    }

    /// send message as frame
    pub async fn send_timeout(&mut self, msg: &M) -> Result<()> {
        let ret = timeout(
            NETWORK_TIMEOUT,
            self.sender.send(serde_json::to_string(msg)?),
        )
        .await;
        let Ok(ret) = ret else {
            warn!("send msg timeout:{:?}", msg);
            return Ok(());
        };
        Ok(ret?)
    }
}

impl FrameReceiver {
    /// recv message as frame
    pub async fn recv(&mut self) -> Result<M> {
        if let Some(msg) = self.receiver.next().await {
            let byte_msg = msg.context("recv frame failed")?;
            let msg = serde_json::from_slice(&byte_msg).context("invalid msg")?;
            Ok(msg)
        } else {
            bail!("no recv msg");
        }
    }

    /// recv message within the specified time
    pub async fn recv_timeout(&mut self) -> Result<M> {
        let msg = timeout(NETWORK_TIMEOUT, self.recv()).await??;
        Ok(msg)
    }

    /// recv message within the customer time
    pub async fn recv_self_timeout(&mut self, time: Duration) -> Result<M> {
        let msg = timeout(time, self.recv()).await??;
        Ok(msg)
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
