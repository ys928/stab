//! the config file

use std::{ops::Range, sync::OnceLock};

use anstyle::{
    AnsiColor::{BrightBlue, BrightCyan, BrightGreen, Green, Red},
    Color::Ansi,
    Style,
};
use clap::{Parser, ValueEnum};

use serde::Deserialize;
use sha2::{Digest, Sha256};

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
    /// an optional secret for authentication
    pub secret: Option<String>,
    /// client mode,all link to server
    pub links: Vec<Link>,
    /// server mode,port range
    pub port_range: Range<u16>,
    /// web manage server port
    pub web_port: u16,
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

    /// an optional secret for authentication
    #[clap(short, long, value_name = "secret")]
    pub secret: Option<String>,

    /// create a link from the local to the server,for example: 8000=www.example.com
    #[clap(short,long,value_name = "local mode",value_parser=cmd_parse_link)]
    pub link: Option<Link>,

    /// accepted TCP port number range
    #[clap(short, long,value_name = "server mode", value_parser = cmd_parse_range)]
    pub port_range: Option<Range<u16>>,

    /// web manage server port
    #[clap(short, long, value_name = "server mode")]
    pub web_port: Option<u16>,
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
}

/// Server configuration
#[derive(Deserialize, Debug)]
pub struct ServerConfig {
    /// the web port
    web_port: Option<u16>,
    /// port range to use
    port_range: Option<String>,
}

/// parse config from command line arguments,must first be called
pub fn init_config() {
    let mut args = StabArgs::parse();

    // default config
    let mut stab_config = StabConfig {
        mode: Mode::Server,
        port: 5656,
        log: 5,
        secret: None,
        links: Vec::new(),
        port_range: 1024..65535,
        web_port: 3400,
    };

    if args.file.is_some() {
        init_by_config_file(args.file.unwrap().as_str(), &mut stab_config);
    }

    args.mode.map(|m| stab_config.mode = m);
    args.control_port.map(|c| stab_config.port = c);
    args.log.map(|l| stab_config.log = l);

    // hash secret
    if let Some(secret) = args.secret {
        let hashed_secret = Sha256::new().chain_update(secret).finalize();
        args.secret = Some(format!("{:x}", hashed_secret));
    }

    if let Some(link) = args.link {
        stab_config.links.push(link);
    }

    if stab_config.mode == Mode::Local && stab_config.links.is_empty() {
        panic!("No provide links");
    }

    G_CFG.get_or_init(|| stab_config);
}

/// init config with file
pub fn init_by_config_file(file: &str, stab_config: &mut StabConfig) {
    let cfg_str = std::fs::read_to_string(file);
    if cfg_str.is_err() {
        panic!("{:?}", cfg_str.unwrap_err());
    }
    let cfg_str = cfg_str.unwrap();

    let file_config = toml::from_str(&cfg_str);

    if let Err(e) = file_config {
        panic!("parse config file failed {}", e);
    }

    let file_config: FileConfig = file_config.unwrap();

    file_config.mode.map(|a| stab_config.mode = a);
    file_config.port.map(|a| stab_config.port = a);
    file_config.log.map(|l| stab_config.log = l);

    if let Some(s) = file_config.secret {
        let hashed_secret = Sha256::new().chain_update(s).finalize();
        stab_config.secret = Some(format!("{:x}", hashed_secret));
    }
    if let Some(s) = file_config.server {
        s.web_port.map(|p| stab_config.web_port = p);
        let p_range = s.port_range.unwrap_or("1024-65535".to_string());
        stab_config.port_range = cmd_parse_range(p_range.as_str()).unwrap();
    }

    if let Some(c) = file_config.local {
        if c.links.is_some() {
            for link in c.links.unwrap().iter() {
                let lin = parse_link(&link, c.to.as_deref());
                if lin.is_err() {
                    panic!("parse link failed: {:?}", link);
                }
                stab_config.links.push(lin.unwrap());
            }
        }
    }
}

/// config the log
pub fn init_log() {
    tracing_subscriber::fmt()
        .with_target(true)
        .with_line_number(true)
        .with_writer(std::io::stdout)
        .with_max_level(log_level())
        .init();
}

/// get the log level from the config
fn log_level() -> tracing::Level {
    let f_cfg = G_CFG.get().unwrap();

    match f_cfg.log {
        1 => tracing::Level::ERROR,
        2 => tracing::Level::WARN,
        3 => tracing::Level::INFO,
        4 => tracing::Level::DEBUG,
        5 => tracing::Level::TRACE,
        _ => tracing::Level::TRACE,
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

/// parse port range
fn cmd_parse_range(s: &str) -> Result<Range<u16>, String> {
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

fn cmd_parse_link(raw_link: &str) -> Result<Link, String> {
    parse_link(raw_link, None)
}

fn parse_link(raw_link: &str, to: Option<&str>) -> Result<Link, String> {
    let err_msg = "parse link failed,format: 80=stab.com or localhost:80=stab.com:8989".to_string();
    let mut link = Link::default();

    let addrs: Vec<&str> = raw_link.split("=").collect();

    // only port
    if addrs.len() == 1 && to.is_some() {
        // parse local address
        let local_addr = parse_address(addrs[0], Some("127.0.0.1"), None);
        if local_addr.is_none() {
            return Err(err_msg);
        }
        let local_addr = local_addr.unwrap();

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
    if local_addr.is_none() {
        return Err(err_msg);
    }
    let local_addr = local_addr.unwrap();

    // pares remote address
    let remote_addr = parse_address(remote_addr, to, Some(0));
    if remote_addr.is_none() {
        return Err(err_msg);
    }
    let remote_addr = remote_addr.unwrap();
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
    // host or port
    if addr.len() == 1 {
        let port = addr[0].parse::<u16>();
        if port.is_err() {
            let host = addr[0].to_string();
            if default_port.is_none() {
                return None;
            }
            return Some(Address {
                host,
                port: default_port.unwrap(),
            });
        } else {
            let port = port.unwrap();
            if default_host.is_none() {
                return None;
            }
            return Some(Address {
                host: default_host.unwrap().to_string(),
                port,
            });
        }
    }

    // host:port
    let port = addr[1].parse::<u16>();
    if port.is_err() {
        return None;
    }
    return Some(Address {
        host: addr[0].to_string(),
        port: port.unwrap(),
    });
}
