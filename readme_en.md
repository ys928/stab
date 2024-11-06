<p align="left">
    <a href="readme.md">中文</a>
    <span> • </span>
    <span>English</span>
</p>

# stab

This is a modern, simple, lightweight, and high-performance TCP tunnel tool for easily exposing local ports to remote servers.

### 1.Installation 

If you have the Rust development environment installed, the easiest way is to use the cargo command:

```bash
cargo install stab
```

If you don't have cargo, you can directly download the precompiled program from the [release](https://github.com/ys928/stab/releases) page and use it.

### 2.Server

You can run the following command on your server:

```bash
stab server
```

This will start stab in server mode, with the default control port being 5746, but you can modify it:

```bash
stab server -c 7777
```

Once successfully running, you will see output similar to the following:

```bash
09:39:49 [INFO] src\server.rs:39 => server listening 0.0.0.0:5746
09:39:49 [INFO] src\web\mod.rs:31 => web server:http://localhost:3000
```

Here, 0.0.0.0:5656 represents the control port, and http://localhost:3400 represents the web service. You can open this link to view information about all clients currently connected to this server, and you can manually disconnect these links if needed:

![image](https://github.com/ys928/stab/assets/80371119/8ee0615f-5e44-46bf-868b-f3f8bf99fbe5)

### 3.Local

Then you can run the following command locally:

```bash
stab local -l 8000=server.com
```

The above command is a shorthand form, and the full format is:

```bash
stab local --link 127.0.0.1:8000=server.com:0
```

This command links your local `127.0.0.1:8000` port with `server.com:0`, which is the default behavior, and the port will be automatically allocated by the server.

Of course, you can also specify the port that needs to be exposed on the server:

```bash
stab local --link 127.0.0.1:8000=server.com:7878
```

If your server has changed the default control port, you should also change it here:

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

### 6.Toml Configuration (Recommended)

In addition to using command line parameters, you can configure all options via a toml configuration file.

Server configuration file example `server.toml`:

```toml
mode = "Server" # Select server mode
port = 5959 # Set control port
secret = "test secret" # Set secret key
log = 5 # Set log level: 1-5, default is 5
log_path = "logs"      # Set the log saving location. The default is the logs directory under the current directory

[server]
web_port = 80 # Set web port
port_range = "2000-3000" # Set the range of ports allowed to use
```

Apply this configuration file:

```bash
stab -f server.toml
```

Local configuration file example `local.toml`:

```toml
mode = "Local"         # Select local mode
port = 5959            # Set control port
secret = "test secret" # Set secret key
log = 5                # Set log level: 1-5, default is 5
log_path = "logs"      # Set the log saving location. The default is the logs directory under the current directory

[local]
to = "server.com"       # Set the default server
links = [
    "127.0.0.1:8080=server.com:2000",  # Complete writing method
    "8080=server.com:1900",            # Equivalent to: 127.0.0.1:8080=server.com:1900
    "8081=server.com",                 # Equivalent to: 127.0.0.1:8081=server.com:0
    "8082=2001",                       # Equivalent to: 127.0.0.1:8082={to}:2001
    "8083",                            # Equivalent to: 127.0.0.1:8083={to}:0
] # Set the links to be established with the server, supporting multiple links simultaneously

```

Apply this configuration file:

```bash
stab -f local.toml
```