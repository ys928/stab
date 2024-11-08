//! control link

use std::{collections::HashMap, sync::Arc};

use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedSender},
    oneshot,
};

use crate::server::CtlConInfo;

type ShotSender = oneshot::Sender<Option<Arc<CtlConInfo>>>;

enum CtlOpt {
    Get(ShotSender, u16),
    Insert(ShotSender, u16, CtlConInfo),
    Remove(ShotSender, u16),
    Contain(oneshot::Sender<bool>, u16),
    AddData(u16, u64),
    View(oneshot::Sender<Vec<Arc<CtlConInfo>>>),
}

/// async map for control conn info
#[derive(Debug)]
pub struct CtlConns {
    opt_sender: UnboundedSender<CtlOpt>,
}

impl CtlConns {
    /// create new CtlConns
    pub fn new() -> Self {
        let (opt_sender, mut opt_receiver) = unbounded_channel::<CtlOpt>();
        tokio::spawn(async move {
            let mut ctl_conns: HashMap<u16, Arc<CtlConInfo>> = HashMap::new();
            while let Some(opt) = opt_receiver.recv().await {
                match opt {
                    CtlOpt::Get(sender, port) => {
                        let data = ctl_conns.get(&port);
                        if let Some(data) = data {
                            sender.send(Some(data.clone())).unwrap();
                        } else {
                            sender.send(None).unwrap();
                        }
                    }
                    CtlOpt::Insert(sender, port, ctl_con_info) => {
                        let ret = ctl_conns.insert(port, Arc::new(ctl_con_info));
                        sender.send(ret).unwrap();
                    }
                    CtlOpt::Remove(sender, port) => {
                        let data = ctl_conns.remove(&port);
                        if let Some(data) = data {
                            sender.send(Some(data)).unwrap();
                        } else {
                            sender.send(None).unwrap();
                        }
                    }
                    CtlOpt::AddData(port, size) => {
                        let data = ctl_conns.get_mut(&port);
                        if let Some(data) = data {
                            let info = CtlConInfo {
                                port,
                                src: data.src.clone(),
                                time: data.time.clone(),
                                data: data.data + size,
                            };
                            *data = Arc::new(info);
                        }
                    }
                    CtlOpt::Contain(sender, port) => {
                        let ret = ctl_conns.contains_key(&port);
                        sender.send(ret).unwrap();
                    }
                    CtlOpt::View(sender) => {
                        let mut ret = Vec::new();
                        for (_, v) in ctl_conns.iter() {
                            ret.push(v.clone());
                        }
                        sender.send(ret).unwrap();
                    }
                }
            }
        });
        Self { opt_sender }
    }

    /// get value by key
    pub async fn get(&self, port: u16) -> Option<Arc<CtlConInfo>> {
        let (sender, receiver) = oneshot::channel();
        self.opt_sender.send(CtlOpt::Get(sender, port)).unwrap();
        let data = receiver.await.unwrap();
        data
    }

    /// insert new value
    pub async fn insert(&self, port: u16, ctl: CtlConInfo) -> Option<Arc<CtlConInfo>> {
        let (sender, receiver) = oneshot::channel();
        self.opt_sender
            .send(CtlOpt::Insert(sender, port, ctl))
            .unwrap();
        let data = receiver.await.unwrap();
        data
    }

    /// remove key
    pub async fn remove(&self, port: u16) -> Option<Arc<CtlConInfo>> {
        let (sender, receiver) = oneshot::channel();
        self.opt_sender.send(CtlOpt::Remove(sender, port)).unwrap();
        let data = receiver.await.unwrap();
        data
    }

    /// add data
    pub fn add_data(&self, port: u16, size: u64) {
        self.opt_sender.send(CtlOpt::AddData(port, size)).unwrap();
    }

    /// add data
    pub async fn contain(&self, port: u16) -> bool {
        let (sender, receiver) = oneshot::channel();
        self.opt_sender.send(CtlOpt::Contain(sender, port)).unwrap();
        let ret = receiver.await.unwrap();
        ret
    }

    /// view data
    pub async fn view(&self) -> Vec<Arc<CtlConInfo>> {
        let (sender, receiver) = oneshot::channel();
        self.opt_sender.send(CtlOpt::View(sender)).unwrap();
        let ret = receiver.await.unwrap();
        ret
    }
}
