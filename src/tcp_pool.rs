//! tcp pool module

use std::collections::{HashMap, LinkedList};

use tokio::{
    net::TcpStream,
    sync::{
        mpsc::{unbounded_channel, UnboundedSender},
        oneshot,
    },
};

use crate::config::G_CFG;

type ShotSender = oneshot::Sender<Option<TcpStream>>;

enum MapOpt {
    AddTcpStream(u16, TcpStream),
    GetTcpStream(ShotSender, u16),
    Remove(u16),
}

/// async map for control conn info
#[derive(Debug)]
pub struct TcpPool {
    opt_sender: UnboundedSender<MapOpt>,
}

impl TcpPool {
    /// create new CtlConns
    pub fn new() -> Self {
        let (opt_sender, mut opt_receiver) = unbounded_channel::<MapOpt>();
        tokio::spawn(async move {
            let mut tcp_pool: HashMap<u16, LinkedList<TcpStream>> = HashMap::new();
            while let Some(opt) = opt_receiver.recv().await {
                match opt {
                    MapOpt::AddTcpStream(port, tcp_stream) => {
                        let tcp_pool = tcp_pool.entry(port).or_insert(LinkedList::new());
                        let pool_size = G_CFG.get().unwrap().pool_size as usize;
                        if tcp_pool.len() < pool_size {
                            tcp_pool.push_back(tcp_stream);
                        }
                    }
                    MapOpt::Remove(port) => {
                        let _ = tcp_pool.remove(&port);
                    }
                    MapOpt::GetTcpStream(sender, port) => {
                        let tcp_stream = tcp_pool.get_mut(&port);
                        if let Some(links) = tcp_stream {
                            sender.send(links.pop_front()).unwrap();
                        } else {
                            sender.send(None).unwrap();
                        }
                    }
                }
            }
        });
        Self { opt_sender }
    }

    /// insert new value
    pub async fn add_tcp_stream(&self, port: u16, tcp_stream: TcpStream) {
        self.opt_sender
            .send(MapOpt::AddTcpStream(port, tcp_stream))
            .unwrap();
    }

    /// insert new value
    pub async fn get_tcp_stream(&self, port: u16) -> Option<TcpStream> {
        let (sender, receiver) = oneshot::channel();
        self.opt_sender
            .send(MapOpt::GetTcpStream(sender, port))
            .unwrap();
        let data = receiver.await.unwrap();
        data
    }

    /// remove key
    pub async fn remove(&self, port: u16) {
        self.opt_sender.send(MapOpt::Remove(port)).unwrap();
    }
}
