//! the config file

use std::sync::OnceLock;

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

    /// local port to expose.
    #[clap(short = 'p', value_name = "local mode", long, default_value = "8080")]
    pub local_port: u16,

    /// local host to expose.
    #[clap(short, long, value_name = "local mode", default_value = "localhost")]
    pub local_host: String,

    /// address of the remote server.
    #[clap(long = "to", value_name = "local mode", default_value = "localhost")]
    pub server: String,

    /// optional port on the remote server to select.
    #[clap(short, long, value_name = "local mode", default_value_t = 0)]
    pub remote_port: u16,

    /// minimum accepted TCP port number.
    #[clap(long = "min", value_name = "server mode", default_value_t = 1024)]
    pub min_port: u16,

    /// maximum accepted TCP port number.
    #[clap(long = "max", value_name = "server mode", default_value_t = 65535)]
    pub max_port: u16,
}
/// the run mode
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Mode {
    /// local mode
    Local,

    /// server mode
    Server,
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
            "{d(%H:%M:%S)} [{l}] {f}:{L} => {m}{n}",
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
pub fn help_styles() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .usage(Style::new().fg_color(Some(Ansi(BrightBlue))))
        .header(Style::new().fg_color(Some(Ansi(BrightBlue))))
        .literal(Style::new().fg_color(Some(Ansi(BrightGreen))))
        .invalid(Style::new().bold().fg_color(Some(Ansi(Red))))
        .error(Style::new().bold().fg_color(Some(Ansi(Red))))
        .valid(Style::new().fg_color(Some(Ansi(Green))))
        .placeholder(Style::new().fg_color(Some(Ansi(BrightCyan))))
}
