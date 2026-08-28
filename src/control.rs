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
