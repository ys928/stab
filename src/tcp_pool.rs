//! tcp pool module

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};

use tokio::sync::oneshot;

use crate::share::FrameStream;

type Waiter = oneshot::Sender<FrameStream>;

/// Concurrent map of pooled framed streams keyed by exposed port.
///
/// Pending accept waiters are preferred over the idle queue so a freshly dialed
/// work connection is never stuck behind NAT-killed pooled sockets.
pub struct TcpPool {
    map: Mutex<HashMap<u16, PortSlot>>,
}

struct PortSlot {
    idle: VecDeque<FrameStream>,
    waiters: VecDeque<Waiter>,
}

impl PortSlot {
    fn empty() -> Self {
        Self {
            idle: VecDeque::new(),
            waiters: VecDeque::new(),
        }
    }
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
        map.entry(port).or_insert_with(PortSlot::empty);
    }

    /// Deliver a work connection to a pending accept waiter, or park it idle.
    pub fn add_frame_stream(&self, port: u16, frame_stream: FrameStream) {
        let mut map = self.map.lock().unwrap();
        let slot = map.entry(port).or_insert_with(PortSlot::empty);

        let mut stream = frame_stream;
        while let Some(waiter) = slot.waiters.pop_front() {
            match waiter.send(stream) {
                Ok(()) => return,
                Err(returned) => {
                    // Waiter timed out / dropped; try the next pending accept.
                    stream = returned;
                }
            }
        }

        slot.idle.push_back(stream);
    }

    /// Register a waiter for the next work connection on `port`.
    ///
    /// If an idle stream is already available, it is sent on `waiter` immediately.
    pub fn add_waiter(&self, port: u16, waiter: Waiter) {
        let mut map = self.map.lock().unwrap();
        let slot = map.entry(port).or_insert_with(PortSlot::empty);

        if let Some(stream) = slot.idle.pop_front() {
            // If the accept task already dropped, park the stream again.
            if let Err(stream) = waiter.send(stream) {
                slot.idle.push_front(stream);
            }
            return;
        }

        slot.waiters.push_back(waiter);
    }

    /// Pop an idle pooled framed stream for `port`, if any.
    pub fn get_frame_stream(&self, port: u16) -> Option<FrameStream> {
        let mut map = self.map.lock().unwrap();
        map.get_mut(&port)?.idle.pop_front()
    }

    /// Drop all idle streams for `port` (e.g. after detecting a dead pooled conn).
    pub fn clear_idle(&self, port: u16) {
        let mut map = self.map.lock().unwrap();
        if let Some(slot) = map.get_mut(&port) {
            slot.idle.clear();
        }
    }

    /// remove key
    pub fn remove(&self, port: u16) {
        let mut map = self.map.lock().unwrap();
        let _ = map.remove(&port);
    }
}
