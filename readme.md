# stab

A modern, simple TCP tunnel in Rust that exposes local ports to a remote server.

### 1.Installation

```bash
cargo install stab
```

### 2.Server

```bash
stab server
```

This will start server mode, and the default control port is 5746, but you can modify：

```bash
stab server -c 7777
```

### 3.Local

```bash
stab local -p 8000 --to your.server.com
```

This will expose your local port at localhost:8000 to the public internet at your.server.com, where the port number are assigned by the server.

If the server has changed the default control port, then it should be changed here as well:

```bash
stab local -c 7777 -p 8000 --to your.server.com
```

### 4.Example

Let's say you start the stab server on the cloud server `your.server.com`:

```bash
stab server
```

And you have opened a local web server on port 8000, then you can connect to the server via `stab`, exposing the local web service.

```bash
stab local -p 8000 --to your.server.com
```

After successfully connecting to the server, you will get an output log message similar to the following:

```bash
09:46:42 [INFO] src\client.rs:72 => listening at your.server.com:1024
```

At this point you can access your local web service at `your.server.com:1024`.

### 5.Secret

To prevent malicious use by others, you can add a key：

```bash
stab server -c 7777 -s test
```

At this point the client must also pass the key to connect to the server:

```bash
stab local -p 8000 --to your.server.com -s test
```


### 6.Options

The full options are shown below.

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

Note: Some options are only available in server mode, some options are only available in local mode, others is generic.

