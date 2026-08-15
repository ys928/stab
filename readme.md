<p align="left">
    <span>中文</span>
    <span> • </span>
    <a href="doc/readme_en.md">English</a>
</p>

# stab

现代、简单、小巧的高性能 TCP 隧道工具，可轻松将本地端口暴露到远程服务器。

主要特点：

- 性能极高
- 心跳检测
- 流量统计
- 支持 Web 管理页手动断开连接
- 支持同时暴露多个本地端口
- 断线自动重连（可配置）

### 1. 安装

若已安装 Rust 开发环境，推荐使用 cargo：

```bash
cargo install stab
```

也可从 [Releases](https://github.com/ys928/stab/releases) 下载对应平台的预编译二进制。

### 2. 服务器

在远程主机上启动服务端：

```bash
stab server
```

默认**控制端口**为 `5656`，**Web 管理端口**为 `3400`。可按需修改：

```bash
stab server -c 7777 -w 8080
```

成功后大致会看到：

```bash
15:52:21 [INFO] stab::server:67 => server listening 0.0.0.0:5656
15:52:21 [INFO] stab::web:33 => web server:http://localhost:3400
```

打开 Web 地址可查看当前连接、流量，并手动断开会话。

![image](https://github.com/user-attachments/assets/24cc756a-6e59-424d-bf99-344ef4d4dc4c)

也可限制服务端对外暴露的端口范围与连接池大小：

```bash
stab server -p 2000-3000 --pool-size 16
```

### 3. 本地

在本地建立隧道：

```bash
stab local -l 8000=server.com
```

完整写法：

```bash
stab local --link 127.0.0.1:8000=server.com:0
```

含义：把本地 `127.0.0.1:8000` 映射到 `server.com`；远程端口为 `0` 时由服务端在允许范围内自动分配。

指定远程暴露端口：

```bash
stab local --link 127.0.0.1:8000=server.com:7878
```

若服务端改了控制端口，本地也需一致：

```bash
stab local -c 7777 --link 8000=server.com
```

### 4. 示例

在 `server.com` 上：

```bash
stab server
```

本地 8000 有 Web 服务时：

```bash
stab local -l 8000=server.com
```

连接成功后会看到类似日志：

```bash
15:54:59 [INFO] stab::local:133 => 127.0.0.1:8000 link to server.com:1024
```

即可通过 `server.com:1024` 访问本地服务。

### 5. 密钥

防止滥用可设置共享密钥（两端一致；明文会经 SHA-256 哈希后比对）：

```bash
stab server -s test
stab local -l 8000=your.server.com -s test
```

### 6. Web 管理页密钥

仅能通过配置文件设置。设置后，管理页 API 需在页面中输入密钥才能查看/断开连接：

```toml
[server]
web_key = "your-web-password"
```

未设置 `web_key` 时，管理页不设鉴权。

### 7. Toml 配置（推荐）

命令行参数会覆盖配置文件中的同名项。

**服务端** `server.toml`：

```toml
mode = "Server"        # 运行模式：Server / Local
port = 5656            # 控制端口，默认 5656
secret = "test secret" # 隧道认证密钥，可选
log = 5                # 日志等级：1=error … 5=trace，默认 5
log_path = "logs"      # 日志目录，默认 logs

[server]
web_port = 3400          # Web 管理端口，默认 3400
web_key = "web password" # Web 管理页密钥，可选
port_range = "2000-3000" # 可分配的数据端口范围（含两端），默认 1024-65535
pool_size = 8            # 预建连接池大小，默认 8
```

```bash
stab -f server.toml
```

**本地** `local.toml`：

```toml
mode = "Local"
port = 5656
secret = "test secret"
log = 5
log_path = "logs"

[local]
to = "server.com"   # 默认远程主机，供简写 link 使用
retry = -1          # 断线重连次数：-1 无限，0 不重连，>0 为最大次数；默认 -1
retry_interval = 5  # 重连间隔（秒），默认 5
links = [
    "127.0.0.1:8080=server.com:2000",  # 完整写法
    "8080=server.com:1900",            # → 127.0.0.1:8080=server.com:1900
    "8081=server.com",                 # → 127.0.0.1:8081=server.com:0
    "8082=2001",                       # → 127.0.0.1:8082={to}:2001
    "8083",                            # → 127.0.0.1:8083={to}:0
]
```

```bash
stab -f local.toml
```

> `retry` / `retry_interval` / `web_key` 仅支持配置文件，无对应 CLI 参数。

### 8. 命令行参数一览

| 参数 | 说明 | 默认 | 适用 |
|------|------|------|------|
| `server` / `local` | 运行模式 | — | 必选（或用 `-f` 指定 `mode`） |
| `-f, --file <PATH>` | 配置文件路径 | — | 通用 |
| `-c, --control-port <PORT>` | 控制端口 | `5656` | 通用 |
| `-s, --secret <SECRET>` | 隧道密钥 | 无 | 通用 |
| `--log <1-5>` | 日志等级 | `5` | 通用 |
| `--log-path <PATH>` | 日志目录 | `logs` | 通用 |
| `-l, --link <LINK>` | 一条隧道映射 | — | Local |
| `-w, --web-port <PORT>` | Web 管理端口 | `3400` | Server |
| `-p, --port-range <A-B>` | 数据端口范围 | `1024-65535` | Server |
| `--pool-size <N>` | 连接池大小 | `8` | Server |

查看内置帮助：

```bash
stab --help
stab server --help
stab local --help
```
