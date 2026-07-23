# Supervisorr

A zero-dependency, ultra-low-memory process supervisor in Rust, perfect for edge devices, old ARM hardware, or minimalist container setups. `supervisorr` is designed to be a drop-in single-binary replacement for legacy Python-based `supervisord`, giving you the same control without the bloat.

## Features

- **Micro Footprint**: Statically compiled asynchronous Rust daemon using Tokio. Zero system library dependencies required.
- **Embedded Web Dashboard**: Fully featured, interactive HTML/JS dashboard embedded directly in the binary using `axum` and `rust-embed`. Manage your cluster securely from the browser on port `3000`.
- **Integrated Logging**: Native `stdout` and `stderr` routing explicitly to target log files configured per-process.
- **Local IPC API**: Commands use a Unix domain socket on Unix and a named pipe (`\\.\pipe\supervisorr`) on Windows.
- **Graceful Takedowns**: Natively listens to `SIGINT` and `SIGTERM` to safely terminate workers and clean up OS socket bindings.

## Configuration

The configuration root contains a base `config.toml` and an `app/` directory.
Pass the configuration root with `-c`:

```text
etc/
├── config.toml
└── app/
    ├── my_app.toml
    └── worker.toml
```

Each program has its own TOML file under `app/`:

```toml
[program]
name = "my_app"
command = "node index.js"
directory = "/var/www/my_app"
autostart = true
autorestart = true
stdout_logfile = "/var/log/my_app.log"
stderr_logfile = "/var/log/my_app.err"

[program.environment]
PORT = "8080"
NODE_ENV = "production"
```

Base Web and logging settings belong in `config.toml`:

```toml
log.level = 3
log.dir = "./logs"
log.console = true

web.port = 3000
web.listen_addr = "127.0.0.1"
```

## Usage

First, generate an `etc/` directory containing `config.toml` and
`app/my_app.toml`:
```bash
./supervisorr init
```

Start the Daemon using that directory:
```bash
./supervisorr daemon -c ./etc
```

The Web dashboard is enabled by default. To build a smaller daemon without the
Web server and embedded static files:

```bash
cargo build --release --no-default-features
```

On Windows, run `supervisorr.exe daemon -c .\etc`. Program commands
are executed through `cmd.exe`; Unix uses `sh`. To connect the CLI to a custom
IPC endpoint, set `SUPERVISORR_IPC` to the configured socket path or named-pipe
name.

Manage Processes via Client CLI:
```bash
# Check the status of all managed applications
./supervisorr status

# Start or stop a target process
./supervisorr start my_app
./supervisorr stop my_app
```

## API Endpoint
The web dashboard listens by default on `http://0.0.0.0:3000`.  
Interact directly programmatically:
```bash
curl -X POST http://127.0.0.1:3000/api/action \
-H "Content-Type: application/json" \
-d '{"action":"start","target":"my_app"}'
```
