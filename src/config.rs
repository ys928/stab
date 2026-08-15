//! the config file

use std::{
    ops::RangeInclusive,
    sync::{Arc, OnceLock},
};

use anstyle::{
    AnsiColor::{BrightBlue, BrightCyan, BrightGreen, Green, Red},
    Color::Ansi,
    Style,
};
use anyhow::{anyhow, Result};
use clap::{Parser, ValueEnum};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tracing::error;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

/// global configuration
pub static G_CFG: OnceLock<StabConfig> = OnceLock::new();

/// global configuration
#[derive(Debug)]
pub struct StabConfig {
    /// run mode
    pub mode: Mode,
    /// control port
    pub port: u16,
    /// log level
    pub log: u8,
    /// log sava path
    pub log_path: String,
    /// an optional secret for authentication
    pub secret: Option<String>,
    /// client mode,all link to server
    pub links: Vec<Arc<Link>>,
    /// server mode,port range (inclusive)
    pub port_range: RangeInclusive<u16>,
    /// web manage server port
    pub web_port: u16,
    /// web manage page auth key (hashed); None means no auth
    pub web_key: Option<String>,
    /// connect pool size
    pub pool_size: u16,
    /// local reconnect attempts; `-1` means infinite, `0` means no retry
    pub retry: i32,
    /// local reconnect interval in seconds
    pub retry_interval: u64,
}

/// the command line arguments
#[derive(Parser, Debug)]
#[clap(author, version, about)]
#[command(styles=cmd_help_styles())]
pub struct StabArgs {
    /// run mode
    #[clap(value_enum)]
    pub mode: Option<Mode>,

    /// config file
    #[clap(short, long)]
    pub file: Option<String>,

    /// the control port
    #[clap(short, long, value_name = "control port")]
    pub control_port: Option<u16>,

    /// the log level,1=error,2=warn,3=info,4=debug,5=trace
    #[clap(long, value_name = "log level")]
    pub log: Option<u8>,

    /// the log save path
    #[clap(long, value_name = "log path")]
    pub log_path: Option<String>,

    /// an optional secret for authentication
    #[clap(short, long, value_name = "secret")]
    pub secret: Option<String>,

    /// create a link from the local to the server,for example: 8000=www.example.com
    #[clap(short,long,value_name = "local mode",value_parser=cmd_parse_link)]
    pub link: Option<Link>,

    /// accepted TCP port number range
    #[clap(short, long,value_name = "server mode", value_parser = cmd_parse_range)]
    pub port_range: Option<RangeInclusive<u16>>,

    /// web manage server port
    #[clap(short, long, value_name = "server mode")]
    pub web_port: Option<u16>,

    /// prebuild this many idle work connections (0 = on-demand, recommended)
    #[clap(long, value_name = "pool size")]
    pub pool_size: Option<u16>,
}
/// the run mode
#[derive(Copy, Clone, Debug, ValueEnum, Deserialize, PartialEq)]
pub enum Mode {
    /// local mode
    Local,

    /// server mode
    Server,
}

/// a link between a local port and a server port
#[derive(Debug, Clone, Default)]
pub struct Address {
    /// host
    pub host: String,
    /// port
    pub port: u16,
}

/// a link between a local port and a server port
#[derive(Debug, Clone, Default)]
pub struct Link {
    /// local
    pub local: Address,
    /// server
    pub remote: Address,
}

/// File configuration
#[derive(Deserialize, Default, Debug)]
pub struct FileConfig {
    /// run mode
    mode: Option<Mode>,
    /// control port
    port: Option<u16>,
    /// the secret
    secret: Option<String>,
    /// the log level
    log: Option<u8>,
    /// the log save path
    log_path: Option<String>,
    /// the client config
    local: Option<LocalConfig>,
    /// the server config
    server: Option<ServerConfig>,
}

/// Client configuration
#[derive(Deserialize, Debug)]
pub struct LocalConfig {
    /// all link to server
    links: Option<Vec<String>>,
    /// default server
    to: Option<String>,
    /// reconnect attempts after disconnect; -1 means infinite, 0 means no retry
    retry: Option<i32>,
    /// reconnect interval in seconds
    retry_interval: Option<u64>,
}

/// Server configuration
#[derive(Deserialize, Debug)]
pub struct ServerConfig {
    /// the web port
    web_port: Option<u16>,
    /// port range to use
    port_range: Option<String>,
    /// pool size
    pool_size: Option<u16>,
    /// web manage page auth key
    web_key: Option<String>,
}

fn default_config() -> StabConfig {
    StabConfig {
        mode: Mode::Server,
        port: 5656,
        log: 5,
        log_path: "logs".to_string(),
        secret: None,
        links: Vec::new(),
        port_range: 1024..=65535,
        web_port: 3400,
        web_key: None,
        // On-demand work connections by default. Pre-pooling (pool_size > 0) can
        // leave idle TCP sockets that NAT/firewalls silently kill; the server
        // then blocks on Start to a dead conn and SSH reconnect hangs.
        pool_size: 0,
        retry: -1,
        retry_interval: 5,
    }
}

fn hash_secret(secret: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::new().chain_update(secret).finalize())
}

/// Hash a plaintext key the same way secrets are stored.
pub fn hash_key(key: impl AsRef<[u8]>) -> String {
    hash_secret(key)
}

/// Parse CLI / config file and store into [`G_CFG`]. Must be called first.
pub fn init_config() {
    let args = StabArgs::parse();
    let mut stab_config = default_config();

    if let Some(file) = &args.file {
        init_by_config_file(file, &mut stab_config);
    }

    if let Some(m) = args.mode {
        stab_config.mode = m;
    }
    if let Some(c) = args.control_port {
        stab_config.port = c;
    }
    if let Some(l) = args.log {
        stab_config.log = l;
    }
    if let Some(p) = args.log_path {
        stab_config.log_path = p;
    }
    if let Some(p) = args.pool_size {
        stab_config.pool_size = p;
    }
    if let Some(secret) = args.secret {
        stab_config.secret = Some(hash_secret(secret));
    }
    if let Some(link) = args.link {
        stab_config.links.push(Arc::new(link));
    }
    if let Some(range) = args.port_range {
        stab_config.port_range = range;
    }
    if let Some(w) = args.web_port {
        stab_config.web_port = w;
    }

    if stab_config.mode == Mode::Local && stab_config.links.is_empty() {
        panic!("No provide links");
    }

    G_CFG.get_or_init(|| stab_config);
}

/// init config with file
pub fn init_by_config_file(file: &str, stab_config: &mut StabConfig) {
    let cfg_str = std::fs::read_to_string(file);

    let Ok(cfg_str) = cfg_str else {
        panic!("{:?}", cfg_str.unwrap_err());
    };

    let file_config = toml::from_str(&cfg_str);

    if let Err(e) = file_config {
        panic!("parse config file failed {}", e);
    }

    let file_config: FileConfig = file_config.unwrap();

    file_config.mode.map(|a| stab_config.mode = a);
    file_config.port.map(|a| stab_config.port = a);
    file_config.log.map(|l| stab_config.log = l);
    file_config.log_path.map(|p| stab_config.log_path = p);

    if let Some(s) = file_config.secret {
        stab_config.secret = Some(hash_secret(s));
    }
    if let Some(s) = file_config.server {
        s.web_port.map(|p| stab_config.web_port = p);
        s.pool_size.map(|p| stab_config.pool_size = p);
        let p_range = s.port_range.unwrap_or("1024-65535".to_string());
        stab_config.port_range = cmd_parse_range(p_range.as_str()).unwrap();
        if let Some(k) = s.web_key {
            stab_config.web_key = Some(hash_secret(k));
        }
    }

    if let Some(c) = file_config.local {
        if let Some(r) = c.retry {
            stab_config.retry = r;
        }
        if let Some(i) = c.retry_interval {
            stab_config.retry_interval = i;
        }
        let links = c.links.unwrap_or_default();
        for link in links {
            let lin = parse_link(&link, c.to.as_deref());

            let Ok(lin) = lin else {
                panic!("parse link failed: {:?}", link);
            };

            stab_config.links.push(Arc::new(lin));
        }
    }
}

/// config the log
pub fn init_log() {
    let timer = tracing_subscriber::fmt::time::ChronoLocal::new("%Y-%m-%d %H:%M:%S".to_owned());

    let cfg = G_CFG.get().unwrap();

    let logfile = tracing_appender::rolling::daily(&cfg.log_path, "stab.log");

    // console Layer
    let console_layer = tracing_subscriber::fmt::layer()
        .with_timer(timer.clone())
        .with_target(true)
        .with_line_number(true)
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_filter(log_level());

    // file Layer
    let file_layer = tracing_subscriber::fmt::layer()
        .with_timer(timer)
        .with_target(true)
        .with_line_number(true)
        .with_writer(logfile)
        .with_ansi(false)
        .with_filter(log_level());

    tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .init();
}

/// get the log level from the config
fn log_level() -> LevelFilter {
    let f_cfg = G_CFG.get().unwrap();

    match f_cfg.log {
        1 => LevelFilter::ERROR,
        2 => LevelFilter::WARN,
        3 => LevelFilter::INFO,
        4 => LevelFilter::DEBUG,
        5 => LevelFilter::TRACE,
        _ => LevelFilter::TRACE,
    }
}

/// config the style of help info
fn cmd_help_styles() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .usage(Style::new().fg_color(Some(Ansi(BrightBlue))))
        .header(Style::new().fg_color(Some(Ansi(BrightBlue))))
        .literal(Style::new().fg_color(Some(Ansi(BrightGreen))))
        .invalid(Style::new().bold().fg_color(Some(Ansi(Red))))
        .error(Style::new().bold().fg_color(Some(Ansi(Red))))
        .valid(Style::new().fg_color(Some(Ansi(Green))))
        .placeholder(Style::new().fg_color(Some(Ansi(BrightCyan))))
}

/// parse port range (inclusive on both ends)
fn cmd_parse_range(s: &str) -> Result<RangeInclusive<u16>> {
    let err_msg = anyhow!("parse port range failed");

    let p: Vec<&str> = s.split("-").collect();
    if p.len() != 2 {
        return Err(err_msg);
    }

    let min = p[0].parse::<u16>();

    let Ok(min) = min else {
        error!("{}", min.unwrap_err());
        return Err(err_msg);
    };

    let max = p[1].parse::<u16>();

    let Ok(max) = max else {
        error!("{}", max.unwrap_err());
        return Err(err_msg);
    };

    if min > max {
        return Err(err_msg);
    }
    Ok(min..=max)
}

fn cmd_parse_link(raw_link: &str) -> Result<Link> {
    parse_link(raw_link, None)
}

fn parse_link(raw_link: &str, to: Option<&str>) -> Result<Link> {
    let err_msg = anyhow!("parse link failed,format: 80=stab.com or localhost:80=stab.com:8989");
    let mut link = Link::default();

    let addrs: Vec<&str> = raw_link.split("=").collect();

    // only port
    if addrs.len() == 1 && to.is_some() {
        // parse local address
        let local_addr = parse_address(addrs[0], Some("127.0.0.1"), None);

        let Some(local_addr) = local_addr else {
            return Err(err_msg);
        };

        let remote_addr = Address {
            host: to.unwrap().to_string(),
            port: 0,
        };
        link.local = local_addr;
        link.remote = remote_addr;
        return Ok(link);
    }

    if addrs.len() != 2 {
        return Err(err_msg);
    }
    let local_addr = addrs[0];
    let remote_addr = addrs[1];

    let local_addr = parse_address(local_addr, Some("127.0.0.1"), None);

    let Some(local_addr) = local_addr else {
        return Err(err_msg);
    };

    // pares remote address
    let remote_addr = parse_address(remote_addr, to, Some(0));

    let Some(remote_addr) = remote_addr else {
        return Err(err_msg);
    };

    link.local = local_addr;
    link.remote = remote_addr;
    return Ok(link);
}

fn parse_address(
    addr: &str,
    default_host: Option<&str>,
    default_port: Option<u16>,
) -> Option<Address> {
    let addr: Vec<&str> = addr.split(":").collect();

    if addr.len() > 2 {
        return None;
    }
    let mut address = Address::default();

    // host or port
    if addr.len() == 1 {
        let port = addr[0].parse::<u16>();
        if let Ok(port) = port {
            address.host = default_host?.to_string();
            address.port = port;
        } else {
            let host = addr[0].to_string();
            address.host = host;
            address.port = default_port?;
        }
        return Some(address);
    }

    // host:port
    let port = addr[1].parse::<u16>();
    let Ok(port) = port else {
        error!("{}", port.unwrap_err());
        return None;
    };
    address.host = addr[0].to_string();
    address.port = port;
    return Some(address);
}
