pub mod app;
pub(crate) mod client;
pub(crate) mod config;
pub(crate) mod ipc;
pub(crate) mod platform;

#[cfg(feature = "web")]
pub(crate) mod web;

#[cfg(windows)]
mod windows;

mod unix;

// #[cfg(unix)]
// mod unix;

use clap::{Parser, Subcommand};
use std::process;
use toolkit_rs::{AppResult, logger};

#[derive(Parser)]
#[command(name = "supervisord")]
#[command(about = "\n A simple and easy-to-use daemon that supports Linux and Windows \n", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a config directory with an example program
    Init {
        #[arg(short, long, default_value = "./etc")]
        config: String,
    },
    ///  Install as a service
    Install {},
    ///  Uninstall as a service
    Uninstall {},
    /// Internal entry point used by the Windows Service Control Manager
    #[command(hide = true)]
    Service { service_name: String },
    /// Starts the supervisor daemon
    Daemon {
        #[arg(short, long, default_value = "./etc")]
        config: String,
    },
    /// Status of processes
    Status,
    /// Start a process
    Start { target: String },
    /// Stop a process
    Stop { target: String },
}

pub async fn cli() -> AppResult {
    let cli = Cli::parse();
    let config_path = match &cli.command {
        Commands::Init { config } | Commands::Daemon { config } => config.as_str(),
        _ => config::default_config_path(),
    };
    let config_path = config::resolve_config_path(config_path);
    let base_config_path = config_path.join("config.toml");
    if base_config_path.is_file() {
        let cfg = config::load_basic(&base_config_path.to_string_lossy());
        logger::setup(cfg.log.clone()).unwrap_or_else(|error| {
            println!("日志初始化失败: {error:?}");
            process::exit(1);
        });
    }

    command(cli.command).await
}

fn init(config: &std::path::Path) -> AppResult {
    let default_config = r#"name = "demo"
command = "echo 'Replace this with your process !'"
directory = "."
autostart = true
autorestart = true
stdout_logfile = "demo.log"
stderr_logfile = "demo.err"
"#;
    std::fs::create_dir_all(config)?;
    let config_path = config.join("config.toml");
    if !config_path.exists() {
        std::fs::write(
            &config_path,
            r#"socket_file = "supervisord"

log.level = 3
log.size_mb = 5
log.style = "Default"
log.dir = "./logs"
log.console = true
log.filters = []

web.port = 18099
web.listen_addr = "127.0.0.1"
"#,
        )?;
    }
    let app_dir = config.join("app");
    std::fs::create_dir_all(&app_dir)?;
    let path = app_dir.join("demo.toml");
    std::fs::write(&path, default_config.trim())?;
    log::info!(
        "Successfully generated default config at {}",
        path.display()
    );
    Ok(())
}

fn install() -> AppResult {
    #[cfg(windows)]
    windows::Install::supervisord().install()?;
    Ok(())
}

fn uninstall() -> AppResult {
    #[cfg(windows)]
    windows::Install::supervisord().uninstall()?;
    Ok(())
}

fn run_as_service(service_name: &str) -> AppResult {
    #[cfg(windows)]
    windows::service::run_as_service(service_name)?;

    #[cfg(not(windows))]
    anyhow::bail!("Windows service mode is only supported on Windows");

    Ok(())
}

pub async fn command(cmd: Commands) -> AppResult {
    match cmd {
        Commands::Init { config } => {
            let config = config::resolve_config_path(&config);
            init(&config)
        }
        Commands::Install {} => install(),
        Commands::Uninstall {} => uninstall(),
        Commands::Service { service_name } => run_as_service(&service_name),
        Commands::Daemon { config } => {
            let config = config::resolve_config_path(&config);
            app::run(&config.to_string_lossy()).await
        }
        Commands::Status => client::status().await,
        Commands::Start { target } => client::start(&target).await,
        Commands::Stop { target } => client::stop(&target).await,
    }
}
