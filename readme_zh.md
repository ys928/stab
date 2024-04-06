<p align="left">
    <a href="readme.md">English</a>
    <span> • </span>
    <span>中文</span>
</p>


# stab

这是一个现代、简单、由rust实现的高性能TCP隧道工具，可轻松将本地端口暴露给远程服务器，借鉴于bore。

### 1.安装

```bash
cargo install stab
```

### 2.服务器

你可以在你的服务器上运行下面这个命令：

```bash
stab server
```

这将启动stab的服务器模式，其默认的控制端口为5746，但您可以修改：

```bash
stab server -c 7777
```

运行成功后，你将看到下面这样的输出：

```bash
09:39:49 [INFO] src\server.rs:39 => server listening 0.0.0.0:5746
09:39:49 [INFO] src\web\mod.rs:31 => web server:http://localhost:3000
```

其中`0.0.0.0:5746`代表控制端口，而`http://localhost:3000`则代表web服务，你可以通过该链接查看到所有连接到本服务器的客户端信息，并可以主动手动断开该链接：

![image](https://github.com/ys928/stab/assets/80371119/4fce5945-02c0-49bb-8c46-8b6d53af7617)

### 3.本地

然后你可以在本地运行下面这条命令：

```bash
stab local -p --link 8000=server.com
```

上面命令为简写形式，其完整格式为：

```bash
stab local -p --link 127.0.0.1:8000=server.com:0
```

该命令会把你的本地`127.0.0.1:8000`端口与你的`server.com:0`进行链接，这是默认行为，此时端口将由服务器自动分配。

当然你也可以指定服务器暴露端口：

```bash
stab local -p --link 127.0.0.1:8000=server.com:7878
```


如果你的服务器更改了默认的控制端口，那么这里也应该更改：

```bash
stab local -c 7777 --link 8000=server.com
```

### 4.示例

假设你在`server.com`中启动了stab服务器模式：

```bash
stab server
```

并且你在本地端口8000启动了一个web服务器，之后你就可以通过`stab`连接到服务器来暴露本地的web服务：

```bash
stab local -l 8000=server.com
```

当你成功连接到服务器后，你将得到类似下面这样的日志输出：

```bash
09:46:42 [INFO] src\client.rs:72 => listening at server.com:1024
```

此时，你就能通过 `server.com:1024` 访问到你的本地web服务。

### 5.密钥

为了防止被别人滥用，你可以添加一个密钥：

```bash
stab server -s test
```

此时客户端就必须填入密钥才能连接到服务器：

```bash
stab local -l 8000=your.server.com -s test
```


### 6.可选参数

完整的可选参数如下：

```bash
a simple CLI tool for making tunnels to localhost

Usage: stab.exe [OPTIONS] <MODE>

Arguments:
  <MODE>  run mode [possible values: local, server]

Options:
  -c, --contrl-port <control port>  the control port [default: 5746]
  -s, --secret <secret>             an optional secret for authentication
  -l, --link <local mode>           create a link from the local to the server [default: 127.0.0.1:8080=127.0.0.1:0]
  -p, --port-range <server mode>    accepted TCP port number range [default: 1024-65535]
  -w, --web-port <server mode>      web manage server port [default: 3000]
  -h, --help                        Print help (see more with '--help')
  -V, --version                     Print version
```

注意，`-p`用于指定服务器可用端口的范围，客户端将忽略该参数。