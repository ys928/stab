<p align="left">
    <span>English</span>
    <span> • </span>
    <a href="readme_zh.md">中文</a>
</p>

# stab

This is a modern, simple, and small high-performance TCP tunnelling tool that makes it easy to expose local ports to remote servers.

### 1.Installation 

If you have the rust development environment installed, then using cargo is probably the easiest way to go:

```bash
cargo install stab
```

If there is no cargo, then you can go to [release](https://github.com/ys928/stab/releases) and download the appropriate version.

### 2.Server

You can run this command below on your server:

```bash
stab server
```

This will start stab in server mode with a default control port of 5746, but you can change this:

```bash
stab server -c 7777
```

After a successful run, you will see output like the following:

```bash
09:39:49 [INFO] src\server.rs:39 => server listening 0.0.0.0:5746
09:39:49 [INFO] src\web\mod.rs:31 => web server:http://localhost:3000
```

Where `0.0.0.0:5746` stands for the control port and `http://localhost:3000` stands for the web service, you can view information about all clients connected to this server through this link, and you can proactively disconnect the link manually:

![image](https://github.com/ys928/stab/assets/80371119/8ee0615f-5e44-46bf-868b-f3f8bf99fbe5)

### 3.Local

You can then run the following command locally:

```bash
stab local -l 8000=server.com
```

The above command is in short form and its full format is:

```bash
stab local --link 127.0.0.1:8000=server.com:0
```

This command will link your local `127.0.0.1:8000` port with your `server.com:0`, which is the default behaviour, at which point the port will be automatically assigned by the server.

Of course you can also specify the server to expose the port:

```bash
stab local --link 127.0.0.1:8000=server.com:7878
```


If your server changed the default control port, it should be changed here as well:

```bash
stab local -c 7777 --link 8000=server.com
```

### 4.Example

Let's say you start stab server mode in `server.com`:

```bash
stab server
```

And you start a web server on local port 8000, after which you can connect to the server via `stab` to expose the local web service:

```bash
stab local -l 8000=server.com
```

When you successfully connect to the server, you will get log output similar to the following:

```bash
09:46:42 [INFO] src\client.rs:72 => listening at server.com:1024
```

At this point, you will be able to access your local web service via `server.com:1024`.

### 5.Secret

To prevent abuse by others, you can add a key:

```bash
stab server -s test
```

At this point the client will have to fill in the key to connect to the server:

```bash
stab local -l 8000=your.server.com -s test
```


### 6.Option

The complete optional parameters are listed below:

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

Note that `-p` is used to specify a range of ports available to the server, which is ignored by the client.
