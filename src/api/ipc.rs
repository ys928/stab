//! Platform IPC transport: Windows named pipe / Unix domain socket.

use std::{io::ErrorKind, sync::Arc};

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{info, warn};

use super::{dispatch, ApiRequest, ApiResponse, ApiState};

const MAX_FRAME: u32 = 16 * 1024 * 1024;

/// Resolve the listen path.
pub fn default_or_path(api_path: Option<String>) -> Result<String> {
    if let Some(p) = api_path {
        return Ok(p);
    }
    let pid = std::process::id();
    #[cfg(windows)]
    {
        Ok(format!(r"\\.\pipe\stab-{pid}"))
    }
    #[cfg(not(windows))]
    {
        let dir = std::env::var_os("TMPDIR")
            .or_else(|| std::env::var_os("TMP"))
            .or_else(|| std::env::var_os("TEMP"))
            .unwrap_or_else(|| "/tmp".into());
        let path = std::path::PathBuf::from(dir).join(format!("stab-{pid}.sock"));
        Ok(path.to_string_lossy().into_owned())
    }
}

/// Bind and serve forever (until process exit).
///
/// Prints `STAB_API` / `STAB_API_TOKEN` only after the endpoint is listening.
pub async fn run_ipc(path: String, state: ApiState, token: String) -> Result<()> {
    #[cfg(windows)]
    {
        run_windows(path, state, token).await
    }
    #[cfg(not(windows))]
    {
        run_unix(path, state, token).await
    }
}

fn announce(path: &str, token: &str) {
    println!("STAB_API={path}");
    println!("STAB_API_TOKEN={token}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    info!("api listening on {path}");
}

#[cfg(windows)]
async fn run_windows(path: String, state: ApiState, token: String) -> Result<()> {
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};

    let state = Arc::new(state);
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .pipe_mode(PipeMode::Byte)
        .create(&path)
        .with_context(|| format!("create named pipe {path}"))?;

    announce(&path, &token);

    loop {
        server.connect().await.context("pipe connect")?;
        let connected = server;
        server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .create(&path)
            .with_context(|| format!("create named pipe {path}"))?;

        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(connected, state.as_ref()).await {
                warn!("api connection closed: {e}");
            }
        });
    }
}

#[cfg(not(windows))]
async fn run_unix(path: String, state: ApiState, token: String) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixListener;

    let sock_path = std::path::Path::new(&path);
    if sock_path.exists() {
        let _ = std::fs::remove_file(sock_path);
    }
    let listener =
        UnixListener::bind(sock_path).with_context(|| format!("bind unix socket {path}"))?;
    let _ = std::fs::set_permissions(sock_path, std::fs::Permissions::from_mode(0o600));

    announce(&path, &token);

    let state = Arc::new(state);
    loop {
        let (stream, _) = listener.accept().await.context("unix accept")?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(stream, state.as_ref()).await {
                warn!("api connection closed: {e}");
            }
        });
    }
}

async fn handle_conn<S>(mut stream: S, state: &ApiState) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let frame = match read_frame(&mut stream).await {
            Ok(f) => f,
            Err(e) => {
                if is_disconnect(&e) {
                    return Ok(());
                }
                return Err(e);
            }
        };
        let req: ApiRequest = serde_json::from_slice(&frame).context("invalid api request")?;
        let resp: ApiResponse = dispatch(state, req).await;
        let body = serde_json::to_vec(&resp)?;
        write_frame(&mut stream, &body).await?;
    }
}

fn is_disconnect(e: &anyhow::Error) -> bool {
    for cause in e.chain() {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            match io.kind() {
                ErrorKind::UnexpectedEof
                | ErrorKind::ConnectionReset
                | ErrorKind::BrokenPipe
                | ErrorKind::ConnectionAborted => return true,
                _ => {}
            }
        }
    }
    false
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME {
        bail!("frame too large: {len}");
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    if len > MAX_FRAME {
        bail!("frame too large: {len}");
    }
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}
