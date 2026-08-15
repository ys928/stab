//! tcp pool module

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};

use crate::share::FrameStream;

/// Concurrent map of pooled framed streams keyed by exposed port.
pub struct TcpPool {
    map: Mutex<HashMap<u16, VecDeque<FrameStream>>>,
}

impl std::fmt::Debug for TcpPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let map = self.map.lock().unwrap();
        f.debug_struct("TcpPool")
            .field("ports", &map.len())
            .finish()
    }
}

impl TcpPool {
    /// create new TcpPool
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Ensure `port` has an entry so waiters see an empty queue instead of `None`.
    pub fn ensure_port(&self, port: u16) {
        let mut map = self.map.lock().unwrap();
        map.entry(port).or_default();
    }

    /// insert a framed work connection (still in protocol mode, waiting for Start)
    pub fn add_frame_stream(&self, port: u16, frame_stream: FrameStream) {
        let mut map = self.map.lock().unwrap();
        map.entry(port).or_default().push_back(frame_stream);
    }

    /// Pop a pooled framed stream for `port`.
    ///
    /// - `None` — port is not in the pool (removed / never created)
    /// - `Some(None)` — port exists but the queue is empty
    /// - `Some(Some(stream))` — a stream was taken
    pub fn get_frame_stream(&self, port: u16) -> Option<Option<FrameStream>> {
        let mut map = self.map.lock().unwrap();
        map.get_mut(&port).map(|links| links.pop_front())
    }

    /// remove key
    pub fn remove(&self, port: u16) {
        let mut map = self.map.lock().unwrap();
        let _ = map.remove(&port);
    }
}
