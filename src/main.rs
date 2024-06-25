//! the start file

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use config::G_CFG;
use log::error;

pub mod config;
pub mod local;
pub mod server;
pub mod share;
pub mod web;

#[tokio::main]
async fn main() {
    config::init_config();
    config::init_log();
    match G_CFG.get().unwrap().mode {
        config::Mode::Local => local::run().await,
        config::Mode::Server => {
            tokio::spawn(async {
                web::run().await;
            });
            server::run().await.unwrap_or_else(|e| {
                error!("{}", e);
            });
        }
    }
}
