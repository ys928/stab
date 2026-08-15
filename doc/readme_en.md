<p align="left">
    <a href="../readme.md">中文</a>
    <span> • </span>
    <span>English</span>
</p>

# stab

A modern, simple, lightweight, high-performance TCP tunnel for exposing local ports to a remote server.

Main features:

- Very high performance
- Heartbeat detection
- Traffic statistics
- Web dashboard to inspect and disconnect sessions
- Multiple local ports at once
- Automatic reconnect on disconnect (configurable)

Performance comparison with [bore](https://github.com/ekzhang/bore):

![image](https://github.com/user-attachments/assets/47ada59e-1203-4dba-b309-7a034fc641d2)

Environment: WSL Ubuntu 24.04. Benchmark command:

```bash
ab -n 100000 -c 5000 http://127.0.0.1:2000/
```

### 1. Installation

With a Rust toolchain:

```bash
cargo install stab
```

Or download a prebuilt binary from [Releases](https://github.com/ys928/stab/releases).

### 2. Server

On the remote host:

```bash
stab server
```

Default **control port** is `5656`; default **web dashboard port** is `3400`. Override as needed:

```bash
stab server -c 7777 -w 8080
```

Successful start looks like:

```bash
15:52:21 [INFO] stab::server:67 => server listening 0.0.0.0:5656
15:52:21 [INFO] stab::web:33 => web server:http://localhost:3400
```

Open the web URL to view connections, traffic, and disconnect sessions manually.

![image](https://github.com/ys928/stab/assets/80371119/8ee0615f-5e44-46bf-868b-f3f8bf99fbe5)

You can also limit the exposed port range and pool size:

```bash
stab server -p 2000-3000 --pool-size 16
```

### 3. Local

Create a tunnel locally:

```bash
stab local -l 8000=server.com
```

Full form:

```bash
stab local --link 127.0.0.1:8000=server.com:0
```

This maps local `127.0.0.1:8000` to `server.com`. Remote port `0` means the server picks a free port within its allowed range.

Pin a remote port:

```bash
stab local --link 127.0.0.1:8000=server.com:7878
```

If the server uses a non-default control port, match it on the client:

```bash
stab local -c 7777 --link 8000=server.com
```

### 4. Example

On `server.com`:

```bash
stab server
```

With a local service on port 8000:

```bash
stab local -l 8000=server.com
```

On success you should see something like:

```bash
15:54:59 [INFO] stab::local:133 => 127.0.0.1:8000 link to server.com:1024
```

Then open `server.com:1024` to reach the local service.

### 5. Secret

To reduce abuse, set a shared secret on both sides (plaintext is hashed with SHA-256 before comparison):

```bash
stab server -s test
stab local -l 8000=your.server.com -s test
```

### 6. Web dashboard key

Configurable via TOML only. When set, the dashboard API requires the key (entered in the page) to list or disconnect sessions:

```toml
[server]
web_key = "your-web-password"
```

If `web_key` is omitted, the dashboard has no auth.

### 7. Toml configuration (recommended)

CLI flags override the same options from the config file.

**Server** `server.toml`:

```toml
mode = "Server"        # Server or Local
port = 5656            # control port (default 5656)
secret = "test secret" # optional tunnel secret
log = 5                # log level: 1=error … 5=trace (default 5)
log_path = "logs"      # log directory (default logs)

[server]
web_port = 3400          # web dashboard port (default 3400)
web_key = "web password" # optional web auth key
port_range = "2000-3000" # inclusive data-port range (default 1024-65535)
pool_size = 8            # prebuilt connection pool size (default 8)
```

```bash
stab -f server.toml
```

**Local** `local.toml`:

```toml
mode = "Local"
port = 5656
secret = "test secret"
log = 5
log_path = "logs"

[local]
to = "server.com"   # default remote host for shorthand links
retry = -1          # reconnect attempts: -1 = forever, 0 = never, >0 = max tries (default -1)
retry_interval = 5  # seconds between reconnects (default 5)
links = [
    "127.0.0.1:8080=server.com:2000",  # full form
    "8080=server.com:1900",            # → 127.0.0.1:8080=server.com:1900
    "8081=server.com",                 # → 127.0.0.1:8081=server.com:0
    "8082=2001",                       # → 127.0.0.1:8082={to}:2001
    "8083",                            # → 127.0.0.1:8083={to}:0
]
```

```bash
stab -f local.toml
```

> `retry`, `retry_interval`, and `web_key` are config-file only (no CLI flags).

### 8. CLI reference

| Flag | Description | Default | Mode |
|------|-------------|---------|------|
| `server` / `local` | Run mode | — | Required (or set `mode` via `-f`) |
| `-f, --file <PATH>` | Config file | — | Both |
| `-c, --control-port <PORT>` | Control port | `5656` | Both |
| `-s, --secret <SECRET>` | Tunnel secret | none | Both |
| `--log <1-5>` | Log level | `5` | Both |
| `--log-path <PATH>` | Log directory | `logs` | Both |
| `-l, --link <LINK>` | One tunnel mapping | — | Local |
| `-w, --web-port <PORT>` | Web dashboard port | `3400` | Server |
| `-p, --port-range <A-B>` | Data port range | `1024-65535` | Server |
| `--pool-size <N>` | Connection pool size | `8` | Server |

Built-in help:

```bash
stab --help
stab server --help
stab local --help
```
