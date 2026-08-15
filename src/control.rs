//! control link

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::server::CtlConInfo;

/// Concurrent map for control connection info.
#[derive(Debug)]
pub struct CtlConns {
    map: Mutex<HashMap<u16, Arc<CtlConInfo>>>,
}

impl CtlConns {
    /// create new CtlConns
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// get value by key
    pub fn get(&self, port: u16) -> Option<Arc<CtlConInfo>> {
        let map = self.map.lock().unwrap();
        map.get(&port).cloned()
    }

    /// insert new value
    pub fn insert(&self, port: u16, ctl: CtlConInfo) -> Option<Arc<CtlConInfo>> {
        let mut map = self.map.lock().unwrap();
        map.insert(port, Arc::new(ctl))
    }

    /// remove key
    pub fn remove(&self, port: u16) {
        let mut map = self.map.lock().unwrap();
        let _ = map.remove(&port);
    }

    /// add traffic counters
    pub fn add_data(&self, port: u16, up_stream: u64, down_stream: u64) {
        let mut map = self.map.lock().unwrap();
        if let Some(data) = map.get_mut(&port) {
            let info = Arc::make_mut(data);
            info.upstream += up_stream;
            info.downstream += down_stream;
            info.total += up_stream + down_stream;
        }
    }

    /// whether the port exists
    pub fn contain(&self, port: u16) -> bool {
        let map = self.map.lock().unwrap();
        map.contains_key(&port)
    }

    /// snapshot of all control connections
    pub fn view(&self) -> Vec<Arc<CtlConInfo>> {
        let map = self.map.lock().unwrap();
        map.values().cloned().collect()
    }
}
