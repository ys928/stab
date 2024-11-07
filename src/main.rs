//! the start file

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use config::G_CFG;

pub mod config;
pub mod control;
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
            tokio::spawn(web::run());
            server::run().await;
        }
    }
}

#[tokio::test]
async fn test_main() {
    config::init_config();
    config::init_log();
    match G_CFG.get().unwrap().mode {
        config::Mode::Local => local::run().await,
        config::Mode::Server => {
            tokio::spawn(web::run());
            server::run().await;
        }
    }
}
