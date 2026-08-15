//! tcp pool module

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};

use tokio::net::TcpStream;

/// Concurrent map of pooled TCP streams keyed by control port.
#[derive(Debug)]
pub struct TcpPool {
    map: Mutex<HashMap<u16, VecDeque<TcpStream>>>,
}

impl TcpPool {
    /// create new TcpPool
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// insert new value
    pub fn add_tcp_stream(&self, port: u16, tcp_stream: TcpStream) {
        let mut map = self.map.lock().unwrap();
        map.entry(port).or_default().push_back(tcp_stream);
    }

    /// Pop a pooled stream for `port`.
    ///
    /// - `None` — port is not in the pool (removed / never created)
    /// - `Some(None)` — port exists but the queue is empty
    /// - `Some(Some(stream))` — a stream was taken
    pub fn get_tcp_stream(&self, port: u16) -> Option<Option<TcpStream>> {
        let mut map = self.map.lock().unwrap();
        map.get_mut(&port).map(|links| links.pop_front())
    }

    /// remove key
    pub fn remove(&self, port: u16) {
        let mut map = self.map.lock().unwrap();
        let _ = map.remove(&port);
    }
}
