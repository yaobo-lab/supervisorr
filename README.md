# supervisord

[中文](#中文) · [English](#english)

一个使用 Rust 编写的轻量级跨平台进程守护程序。它通过单个可执行文件管理多个前台进程，支持自动启动、异常退出后重启、日志重定向、本地 CLI 控制，以及可选的 Web 管理界面。

> 项目目前处于早期开发阶段（`0.1.0`）。它借鉴了 Python Supervisor 的使用方式，但并非与其配置或功能完全兼容。

## 中文

### 功能特性

- 支持 Linux、其他 Unix 系统和 Windows
- 每个应用使用独立的 TOML 配置文件
- 支持自动启动和退出后自动重启
- 可设置工作目录、环境变量及 stdout/stderr 日志文件
- 通过 Unix Domain Socket 或 Windows Named Pipe 提供本地控制
- 提供 `status`、`start`、`stop` 命令
- 内置可选的 Web 控制台和 JSON API
- Linux 和 Windows Web 控制台可显示进程内存占用
- 支持将程序安装为 Windows 系统服务
- 收到 `SIGINT`/`SIGTERM`（Unix）或 `Ctrl+C`（Windows）时停止托管进程

### 快速开始

要求 Rust stable 工具链。克隆项目后执行：

```bash
cargo build --release

# 在当前项目下生成 etc/config.toml 和 etc/app/demo.toml
./target/release/supervisord init -c ./etc

# 启动守护进程
./target/release/supervisord daemon -c ./etc
```

Windows PowerShell：

```powershell
cargo build --release
.\target\release\supervisord.exe init -c .\etc
.\target\release\supervisord.exe daemon -c .\etc
```

默认示例的 Web 控制台地址为 <http://127.0.0.1:18099>。

> 未显式传入 `-c` 时，程序会从可执行文件所在目录读取或创建 `etc/`，而不是始终使用当前工作目录。

### 配置

配置目录结构如下：

```text
etc/
├── config.toml
└── app/
    ├── api.toml
    └── worker.toml
```

`config.toml` 保存日志和 Web 设置：

```toml
log.level = 3
log.size_mb = 5
log.style = "Default"
log.dir = "./logs"
log.console = true
log.filters = []

web.port = 18099
web.listen_addr = "127.0.0.1"
```

`app/` 下的每个 `.toml` 文件定义一个程序，例如 `app/api.toml`：

```toml
name = "api"
command = "node index.js"
directory = "/var/www/api"
autostart = true
autorestart = true
stdout_logfile = "/var/log/api.log"
stderr_logfile = "/var/log/api.err"

[environment]
PORT = "8080"
NODE_ENV = "production"
```

程序配置字段：

| 字段 | 必填 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `name` | 是 | — | 唯一的程序名称，也是 CLI 操作目标 |
| `command` | 是 | — | Unix 通过 `sh -c`、Windows 通过 `cmd.exe` 执行 |
| `directory` | 否 | 当前目录 | 程序工作目录 |
| `autostart` | 否 | `true` | 守护进程启动后是否运行程序 |
| `autorestart` | 否 | `true` | 程序退出后是否重新启动 |
| `stdout_logfile` | 否 | 丢弃输出 | stdout 追加写入的文件 |
| `stderr_logfile` | 否 | 丢弃输出 | stderr 追加写入的文件 |
| `environment` | 否 | — | 传递给程序的环境变量表 |

应用名称不能重复。无法解析的应用配置会记录错误并跳过；基础配置缺失、配置目录无效或名称重复会导致启动失败。

### 命令行

守护进程运行后，可从另一个终端执行：

```bash
supervisord status
supervisord start api
supervisord stop api
```

完整命令：

```text
supervisord init [-c <CONFIG>]
supervisord daemon [-c <CONFIG>]
supervisord status
supervisord start <TARGET>
supervisord stop <TARGET>
supervisord install
supervisord uninstall
```

本地 IPC 默认使用：

- Unix：系统临时目录下的 `supervisord.sock`
- Windows：`\\.\pipe\supervisord`

客户端可通过区分大小写的环境变量 `supervisord_IPC` 指定其他端点。Unix 端点需要完整路径；Windows 可使用管道名称或完整的 Named Pipe 路径。

### Web 控制台与 API

默认 feature 包含 Web 控制台。监听地址由 `web.listen_addr` 和 `web.port` 决定。若要允许远程访问，可将地址设为 `0.0.0.0`；请仅在可信网络中使用，因为当前 Web API 不含身份认证或 TLS。

```bash
# 获取所有进程状态
curl http://127.0.0.1:18099/api/status

# 启动程序
curl -X POST http://127.0.0.1:18099/api/action \
  -H "Content-Type: application/json" \
  -d '{"action":"start","target":"api"}'

# 停止程序
curl -X POST http://127.0.0.1:18099/api/action \
  -H "Content-Type: application/json" \
  -d '{"action":"stop","target":"api"}'
```

不需要 Web 功能时，可生成更精简的版本：

```bash
cargo build --release --no-default-features
```

### Windows 服务

先将 `etc/` 放在 `supervisord.exe` 旁，并使用管理员权限运行：

```powershell
.\supervisord.exe install
```

该命令会把可执行文件及配置复制到 `C:\ProgramData\supervisord\`，注册自动启动的 `supervisord` 服务并立即启动。卸载：

```powershell
.\supervisord.exe uninstall
```

卸载会停止并删除服务，同时移除 `C:\ProgramData\supervisord\` 目录。

### 运行注意事项

- 托管程序必须保持前台运行。不要在命令末尾添加 `&`，也不要让程序自行 daemonize，否则 supervisor 无法正确跟踪其生命周期。
- `stop` 在 Unix 上向被跟踪进程发送 `SIGTERM`；Windows 使用 `taskkill /T /F` 终止进程树。
- 日志文件以追加方式写入。请确保目标目录已存在且 supervisor 具有写权限。
- 当前配置只在守护进程启动时读取，修改后需要重启守护进程。

---

## English

`supervisord` is a lightweight, cross-platform process supervisor written in Rust. A single executable manages multiple foreground processes with autostart, restart-on-exit, log redirection, local CLI control, and an optional web dashboard.

> The project is currently at an early stage (`0.1.0`). It takes inspiration from Python Supervisor, but it is not fully compatible with its configuration or feature set.

### Features

- Linux, other Unix platforms, and Windows support
- One TOML file per managed application
- Automatic startup and restart after exit
- Configurable working directory, environment, and stdout/stderr log files
- Local control through a Unix domain socket or Windows named pipe
- `status`, `start`, and `stop` CLI commands
- Optional embedded web dashboard and JSON API
- Process memory usage in the web UI on Linux and Windows
- Windows service installation
- Managed-process shutdown on `SIGINT`/`SIGTERM` (Unix) or `Ctrl+C` (Windows)

### Quick start

A stable Rust toolchain is required. After cloning the repository:

```bash
cargo build --release

# Generate etc/config.toml and etc/app/demo.toml in the current project
./target/release/supervisord init -c ./etc

# Start the daemon
./target/release/supervisord daemon -c ./etc
```

Windows PowerShell:

```powershell
cargo build --release
.\target\release\supervisord.exe init -c .\etc
.\target\release\supervisord.exe daemon -c .\etc
```

The generated example exposes the web dashboard at <http://127.0.0.1:18099>.

> When `-c` is omitted, the `etc/` directory is resolved relative to the executable, rather than always relative to the current working directory.

### Configuration

The configuration directory has this structure:

```text
etc/
├── config.toml
└── app/
    ├── api.toml
    └── worker.toml
```

`config.toml` contains logging and web settings:

```toml
log.level = 3
log.size_mb = 5
log.style = "Default"
log.dir = "./logs"
log.console = true
log.filters = []

web.port = 18099
web.listen_addr = "127.0.0.1"
```

Each `.toml` file under `app/` defines one program. For example, `app/api.toml`:

```toml
name = "api"
command = "node index.js"
directory = "/var/www/api"
autostart = true
autorestart = true
stdout_logfile = "/var/log/api.log"
stderr_logfile = "/var/log/api.err"

[environment]
PORT = "8080"
NODE_ENV = "production"
```

Program fields:

| Field | Required | Default | Description |
| --- | --- | --- | --- |
| `name` | Yes | — | Unique program name and CLI target |
| `command` | Yes | — | Executed with `sh -c` on Unix or `cmd.exe` on Windows |
| `directory` | No | Current directory | Program working directory |
| `autostart` | No | `true` | Start the program with the supervisor |
| `autorestart` | No | `true` | Restart the program after it exits |
| `stdout_logfile` | No | Discarded | File receiving appended stdout |
| `stderr_logfile` | No | Discarded | File receiving appended stderr |
| `environment` | No | — | Environment variables passed to the program |

Program names must be unique. Invalid application files are logged and skipped; a missing base configuration, invalid configuration directory, or duplicate name prevents startup.

### CLI

With the daemon running, use another terminal to manage processes:

```bash
supervisord status
supervisord start api
supervisord stop api
```

Available commands:

```text
supervisord init [-c <CONFIG>]
supervisord daemon [-c <CONFIG>]
supervisord status
supervisord start <TARGET>
supervisord stop <TARGET>
supervisord install
supervisord uninstall
```

The default local IPC endpoint is:

- Unix: `supervisord.sock` in the system temporary directory
- Windows: `\\.\pipe\supervisord`

Set the case-sensitive `supervisord_IPC` environment variable to use another client endpoint. Supply a full path on Unix; on Windows, use either a pipe name or a complete named-pipe path.

### Web dashboard and API

The default feature set includes the dashboard. Its address is controlled by `web.listen_addr` and `web.port`. Set the address to `0.0.0.0` for remote access only on a trusted network: the current web API provides neither authentication nor TLS.

```bash
# Get all process states
curl http://127.0.0.1:18099/api/status

# Start a program
curl -X POST http://127.0.0.1:18099/api/action \
  -H "Content-Type: application/json" \
  -d '{"action":"start","target":"api"}'

# Stop a program
curl -X POST http://127.0.0.1:18099/api/action \
  -H "Content-Type: application/json" \
  -d '{"action":"stop","target":"api"}'
```

Build without the dashboard when it is not required:

```bash
cargo build --release --no-default-features
```

### Windows service

Place `etc/` beside `supervisord.exe`, then run from an elevated terminal:

```powershell
.\supervisord.exe install
```

This copies the executable and configuration to `C:\ProgramData\supervisord\`, registers the auto-starting `supervisord` service, and starts it. To uninstall:

```powershell
.\supervisord.exe uninstall
```

Uninstalling stops and deletes the service and removes `C:\ProgramData\supervisord\`.

### Operational notes

- Managed applications must stay in the foreground. Do not append `&` or let them daemonize, because the supervisor would lose accurate lifecycle tracking.
- `stop` sends `SIGTERM` to the tracked process on Unix. On Windows, it uses `taskkill /T /F` to terminate the process tree.
- Log files are opened in append mode. Their parent directories must exist and be writable by the supervisor.
- Configuration is read only when the daemon starts; restart the daemon after making changes.
