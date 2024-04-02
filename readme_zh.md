<p align="left">
    <a href="readme.md">英文</a>
    <span> • </span>
    <span>中文</span>
</p>


# stab

这是一个由Rust实现的、现代的、简单的 TCP 隧道工具，可轻松将本地端口暴露给远程服务器。

### 1.安装

```bash
cargo install stab
```

### 2.服务器

```bash
stab server
```

这将启动服务器模式，默认控制端口为 5746，但您可以修改：

```bash
stab server -c 7777
```

### 3.本地

```bash
stab local -p 8000 --to your.server.com
```

这将暴露你本地 localhost:8000 端口到你的公网 your.server.com 上，并且端口由服务器自动分配。

如果你的服务器更改了默认的控制端口，那么这里也应该更改：

```bash
stab local -c 7777 -p 8000 --to your.server.com
```

### 4.示例

假设你在你的`your.server.com`中启动了stab服务器模式：

```bash
stab server
```

并且你在本地端口8000启动了一个web服务器，之后你就可以通过`stab`连接到服务器来暴露本地的web服务：

```bash
stab local -p 8000 --to your.server.com
```

当你成功连接到服务器后，你将得到类似下面这样的日志输出：

```bash
09:46:42 [INFO] src\client.rs:72 => listening at your.server.com:1024
```

此时，你就能通过 `your.server.com:1024` 访问到你的本地web服务。

### 5.密钥

为了防止被别人滥用，你可以添加一个密钥：

```bash
stab server -c 7777 -s test
```

此时客户端就必须填入密钥才能连接到服务器：

```bash
stab local -p 8000 --to your.server.com -s test
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
  -p, --local-port <local mode>     local port to expose [default: 8080]
  -l, --local-host <local mode>     local host to expose [default: localhost]
      --to <local mode>             address of the remote server [default: localhost]
  -r, --remote-port <local mode>    optional port on the remote server to select [default: 0]
      --min <server mode>           minimum accepted TCP port number [default: 1024]
      --max <server mode>           maximum accepted TCP port number [default: 65535]
  -h, --help                        Print help (see more with '--help')
  -V, --version                     Print version
```

注意：某些选项只在服务器模式下有效，某些模式仅在本地模式下有效，剩下的则是共用的。
