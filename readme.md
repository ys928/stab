<p align="left">
    <span>中文</span>
    <span> • </span>
    <a href="readme_en.md">English</a>
</p>

# stab

这是一个现代、简单、小巧的高性能 TCP 隧道工具，可轻松将本地端口暴露给远程服务器。

主要特点：

- 人性化
- 性能极高
- 心跳检测
- 流量统计
- 允许手动断开链接
- 允许暴露多个本地端口

下图为stab与[bore](https://github.com/ekzhang/bore)之间的性能对比：

![image](https://github.com/user-attachments/assets/47ada59e-1203-4dba-b309-7a034fc641d2)

环境为wsl ubuntu 24.04，压测命令为：

```bash
 ab -n 100000 -c 5000 http://127.0.0.1:2000/
```

### 1.安装

如果你安装了 rust 开发环境，那么使用 cargo 命令是最简单的方式：

```bash
cargo install stab
```

如果没有 cargo，那么你可以直接去[release](https://github.com/ys928/stab/releases)下载已经编译好的程序使用。

### 2.服务器

你可以在你的服务器上运行下面这个命令：

```bash
stab server
```

这将启动 stab 的服务器模式，其默认的控制端口为 5746，但您可以修改：

```bash
stab server -c 7777
```

运行成功后，你将看到下面这样的输出：

```bash
15:52:21 [INFO] stab::server:62 => server listening 0.0.0.0:5656
15:52:21 [INFO] stab::web:29 => web server:http://localhost:3400
```

其中`0.0.0.0:5656`代表控制端口，而`http://localhost:3400`则代表 web 服务，你可以打开该链接查看当前所有连接到本服务器的客户端信息，并可以主动手动断开该链接：

![image](https://github.com/user-attachments/assets/24cc756a-6e59-424d-bf99-344ef4d4dc4c)


### 3.本地

然后你可以在本地运行下面这条命令：

```bash
stab local -l 8000=server.com
```

上面命令为简写形式，其完整的格式为：

```bash
stab local --link 127.0.0.1:8000=server.com:0
```

该命令会把你的本地`127.0.0.1:8000`端口与你的`server.com:0`进行链接，这是默认行为，此时端口将由服务器自动分配。

当然你也可以指定服务器需要暴露端口：

```bash
stab local --link 127.0.0.1:8000=server.com:7878
```

如果你的服务器更改了默认的控制端口，那么这里也应该更改：

```bash
stab local -c 7777 --link 8000=server.com
```

### 4.示例

假设你在`server.com`中启动了 stab 服务器模式：

```bash
stab server
```

并且你在本地端口 8000 启动了一个 web 服务器，之后你就可以通过`stab`连接到服务器来暴露本地的 web 服务：

```bash
stab local -l 8000=server.com
```

当你成功连接到服务器后，你将得到类似下面这样的日志输出：

```bash
15:54:59 [INFO] stab::client:101 => 127.0.0.1:8000 link to server.com:1024
```

此时，你就能通过 `server.com:1024` 访问到你的本地 web 服务。

### 5.密钥

为了防止被别人滥用，你可以添加一个密钥：

```bash
stab server -s test
```

此时客户端就必须填入密钥才能连接到服务器：

```bash
stab local -l 8000=your.server.com -s test
```

### 6.Toml 配置（推荐）

除了使用命令行参数外，你可以通过 toml 配置文件的方式配置所有选项。

服务器配置文件实例`server.toml`：

```toml
mode = "Server"        # 选择服务器模式
port = 5959            # 设置控制端口
secret = "test secret" # 设置密钥
log = 5                # 设置日志等级：1-5，默认为5
log_path = "logs"      # 设置日志保存位置，默认为当前目录下的logs目录

[server]
web_port = 80            # 设置web端口
port_range = "2000-3000" # 设置允许使用的端口范围
pool_size = 8            # 设置连接池大小，默认8个
```

应用该配置文件：

```bash
stab -f server.toml
```

本地配置文件实例`local.toml`：

```toml
mode = "Local"         # 选择本地模式
port = 5959            # 设置控制端口
secret = "test secret" # 设置密钥
log = 5                # 设置日志等级：1-5，默认为5
log_path = "logs"      # 设置日志保存位置，默认为当前目录下的logs目录

[local]
to = "server.com"       # 设置默认服务器
links = [
    "127.0.0.1:8080=server.com:2000",  # 完整写法
    "8080=server.com:1900",            # 等价于：127.0.0.1:8080=server.com:1900
    "8081=server.com",                 # 等价于：127.0.0.1:8081=server.com:0
    "8082=2001",                       # 等价于：127.0.0.1:8082={to}:2001
    "8083",                            # 等价于：127.0.0.1:8083={to}:0
] # 设置将要与服务器建立的链接，支持同时建立多个链接
```

应用该配置文件：

```bash
stab -f local.toml
```
