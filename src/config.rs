//! the config file

use std::{ops::Range, sync::OnceLock};

use anstyle::{
    AnsiColor::{BrightBlue, BrightCyan, BrightGreen, Green, Red},
    Color::Ansi,
    Style,
};
use clap::{Parser, ValueEnum};

use sha2::{Digest, Sha256};

use log4rs::{
    append::console::ConsoleAppender,
    config::{Appender, Root},
    encode::pattern::PatternEncoder,
    Config,
};

/// global configuration
pub static G_CFG: OnceLock<HoleArgs> = OnceLock::new();

/// the command line arguments
#[derive(Parser, Debug)]
#[clap(author, version, about)]
#[command(styles=help_styles())]
pub struct HoleArgs {
    /// run mode
    #[clap(value_enum)]
    pub mode: Mode,

    /// the control port
    #[clap(short, long, value_name = "control port", default_value_t = 5746)]
    pub contrl_port: u16,

    /// an optional secret for authentication
    #[clap(short, long, value_name = "secret")]
    pub secret: Option<String>,

    /// create a link from the local to the server
    #[clap(short,long,value_name = "local mode",value_parser=parse_link,default_value = "127.0.0.1:8080=127.0.0.1:0")]
    pub link: Link,

    /// accepted TCP port number range
    #[clap(short, long,value_name = "server mode", value_parser = parse_range,default_value="1024-65535")]
    pub port_range: Range<u16>,
}
/// the run mode
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Mode {
    /// local mode
    Local,

    /// server mode
    Server,
}

/// a link between a local port and a server port
#[derive(Debug, Clone, Default)]
pub struct Link {
    /// local host
    pub local_host: String,
    /// local port
    pub local_port: u16,
    /// server host
    pub remote_host: String,
    /// server port
    pub remote_port: u16,
}

/// parse config from command line arguments
pub fn init_config() {
    let mut args = HoleArgs::parse();
    // hash secret
    if let Some(secret) = args.secret {
        let hashed_secret = Sha256::new().chain_update(secret).finalize();
        args.secret = Some(format!("{:x}", hashed_secret));
    }

    G_CFG.get_or_init(|| args);
}

/// config the log
pub fn init_log() {
    let con = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "{d(%H:%M:%S)} [{l}] {M}:{L} => {m}{n}",
        )))
        .build();

    let config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(con)))
        .build(
            Root::builder()
                .appender("stdout")
                .build(log::LevelFilter::Debug),
        )
        .unwrap();

    let _ = log4rs::init_config(config).unwrap();
}

/// config the style of help info
fn help_styles() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .usage(Style::new().fg_color(Some(Ansi(BrightBlue))))
        .header(Style::new().fg_color(Some(Ansi(BrightBlue))))
        .literal(Style::new().fg_color(Some(Ansi(BrightGreen))))
        .invalid(Style::new().bold().fg_color(Some(Ansi(Red))))
        .error(Style::new().bold().fg_color(Some(Ansi(Red))))
        .valid(Style::new().fg_color(Some(Ansi(Green))))
        .placeholder(Style::new().fg_color(Some(Ansi(BrightCyan))))
}

/// parse port range
fn parse_range(s: &str) -> Result<Range<u16>, String> {
    let err_msg = "parse port range failed".to_string();

    let p: Vec<&str> = s.split("-").collect();
    if p.len() != 2 {
        return Err(err_msg);
    }
    let min = p[0].parse::<u16>();
    if min.is_err() {
        return Err(err_msg);
    }
    let min = min.unwrap();
    let max = p[1].parse::<u16>();
    if max.is_err() {
        return Err(err_msg);
    }
    let max = max.unwrap();
    if min >= max {
        return Err(err_msg);
    }
    Ok(min..max)
}

fn parse_link(raw_link: &str) -> Result<Link, String> {
    let err_msg =
        "parse link failed,format: 80=stab.com or localhost:80=stab.com:8989".to_string();

    let addrs: Vec<&str> = raw_link.split("=").collect();
    if addrs.len() != 2 {
        return Err(err_msg);
    }
    let local_addr = addrs[0];
    let remote_addr = addrs[1];

    let mut link = Link::default();
    // parse local address
    let pos = local_addr.find(":");
    if pos.is_none() {
        let port = local_addr.parse::<u16>();
        if port.is_err() {
            return Err(err_msg);
        }
        link.local_host = "127.0.0.1".to_string();
        link.local_port = port.unwrap();
    } else {
        let addr: Vec<&str> = local_addr.split(":").collect();
        if addr.len() != 2 {
            return Err(err_msg);
        }
        let port = addr[1].parse::<u16>();
        if port.is_err() {
            return Err(err_msg);
        }
        link.local_host = addr[0].to_string();
        link.local_port = port.unwrap();
    }
    // pares remote address
    let pos = remote_addr.find(":");
    if pos.is_none() {
        link.remote_host = remote_addr.to_string();
        link.remote_port = 0;
        return Ok(link);
    } else {
        let addr: Vec<&str> = remote_addr.split(":").collect();
        if addr.len() != 2 {
            return Err(err_msg);
        }
        let port = addr[1].parse::<u16>();
        if port.is_err() {
            return Err(err_msg);
        }
        link.remote_host = addr[0].to_string();
        link.remote_port = port.unwrap();
        return Ok(link);
    }
}
