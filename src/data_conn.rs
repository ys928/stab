//! data connect

use std::collections::HashMap;

use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedSender},
    oneshot,
};
use uuid::Uuid;

use crate::server::DataConn;

type ShotSender = oneshot::Sender<Option<DataConn>>;

enum CtlOpt {
    Insert(ShotSender, Uuid, DataConn),
    Remove(ShotSender, Uuid),
}

/// async map for control conn info
#[derive(Debug)]
pub struct DataConns {
    opt_sender: UnboundedSender<CtlOpt>,
}

impl DataConns {
    /// create new CtlConns
    pub fn new() -> Self {
        let (opt_sender, mut opt_receiver) = unbounded_channel::<CtlOpt>();
        tokio::spawn(async move {
            let mut data_conns: HashMap<Uuid, DataConn> = HashMap::new();
            while let Some(opt) = opt_receiver.recv().await {
                match opt {
                    CtlOpt::Insert(sender, port, ctl_con_info) => {
                        let ret = data_conns.insert(port, ctl_con_info);
                        sender.send(ret).unwrap();
                    }
                    CtlOpt::Remove(sender, port) => {
                        let data = data_conns.remove(&port);
                        if let Some(data) = data {
                            sender.send(Some(data)).unwrap();
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
    pub async fn insert(&self, port: Uuid, ctl: DataConn) -> Option<DataConn> {
        let (sender, receiver) = oneshot::channel();
        self.opt_sender
            .send(CtlOpt::Insert(sender, port, ctl))
            .unwrap();
        let data = receiver.await.unwrap();
        data
    }

    /// remove key
    pub async fn remove(&self, port: Uuid) -> Option<DataConn> {
        let (sender, receiver) = oneshot::channel();
        self.opt_sender.send(CtlOpt::Remove(sender, port)).unwrap();
        let data = receiver.await.unwrap();
        data
    }
}
