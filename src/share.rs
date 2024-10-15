//! give some generic code

use std::time::Duration;

use log::warn;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use uuid::Uuid;

use anyhow::{Context, Result};
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
pub struct FrameStream {
    stream: TcpStream,
    msg: String,
}

impl FrameStream {
    /// create a new frame stream
    pub fn new(stream: TcpStream) -> Self {
        FrameStream {
            stream,
            msg: String::new(),
        }
    }

    /// send message as frame
    pub async fn send(&mut self, msg: &Message) -> Result<()> {
        let mut data = serde_json::to_string(msg).unwrap();
        data.push('\n');
        self.stream
            .write_all(data.as_bytes())
            .await
            .context(format!("send msg:{:?}", msg))?;
        Ok(())
    }

    /// recv message as frame
    pub async fn recv(&mut self) -> Result<Message> {
        loop {
            let pos = self.msg.find("\n");
            if pos.is_none() {
                let mut buf: [u8; 255] = [0; 255];
                let size = self.stream.read(&mut buf).await?;
                let msg = String::from_utf8(buf[0..size].to_vec());
                let Ok(msg) = msg else {
                    warn!("failed to convert message from stream:{}", msg.unwrap_err());
                    continue;
                };
                self.msg.push_str(&msg);
                continue;
            }

            let mut msg: String = self.msg.drain(0..=pos.unwrap()).collect();
            msg.pop(); // remove \n
            let msg = serde_json::from_str(&msg);
            let Ok(msg) = msg else {
                warn!("{}", msg.unwrap_err());
                continue;
            };
            return Ok(msg);
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
        self.stream
    }
}

/// Copy data mutually between two read/write streams.
pub async fn proxy<T>(stream1: T, stream2: T) -> Result<u64>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let (mut s1_read, mut s1_write) = io::split(stream1);
    let (mut s2_read, mut s2_write) = io::split(stream2);
    let bytes = tokio::select! {
        res = io::copy(&mut s1_read, &mut s2_write) => res,
        res = io::copy(&mut s2_read, &mut s1_write) => res,
    }?;
    Ok(bytes)
}
