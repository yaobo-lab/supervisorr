pub mod client;
pub mod config;
pub mod daemon;
pub mod platform;
#[cfg(feature = "web")]
pub mod web;

use clap::{Parser, Subcommand};
use std::process;
use toolkit_rs::{
    logger,
    painc::{PaincConf, set_panic_handler},
};

#[derive(Parser)]
#[command(name = "supervisord")]
#[command(about = "A zero-dependency process manager", long_about = None)]
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

pub async fn run_cli() -> anyhow::Result<()> {
    set_panic_handler(PaincConf {
        version: "1.0.0".into(),
        build_time: "".into(),
        painc_exit: true,
    });

    let cli = Cli::parse();
    let config_root = match &cli.command {
        Commands::Init { config } | Commands::Daemon { config } => config.as_str(),
        _ => "./etc",
    };
    let base_config_path = std::path::Path::new(config_root).join("config.toml");
    if base_config_path.is_file() {
        let cfg = config::load(&base_config_path.to_string_lossy());
        logger::setup(cfg.log.clone()).unwrap_or_else(|error| {
            println!("日志初始化失败: {error:?}");
            process::exit(1);
        });
    }

    run_command(cli.command).await
}

pub async fn run_command(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Init { config } => {
            let default_config = r#"[program]
name = "my_app"
command = "echo 'Replace this with your process !'"
directory = "."
autostart = true
autorestart = true
stdout_logfile = "my_app.log"
stderr_logfile = "my_app.err"
"#;
            std::fs::create_dir_all(&config)?;
            let config_path = std::path::Path::new(&config).join("config.toml");
            if !config_path.exists() {
                std::fs::write(
                    &config_path,
                    r#"log.level = 3
log.size_mb = 5
log.style = "Module"
log.dir = "./logs"
log.console = true
log.filters = []

web.port = 3000
web.listen_addr = "127.0.0.1"
"#,
                )?;
            }
            let app_dir = std::path::Path::new(&config).join("app");
            std::fs::create_dir_all(&app_dir)?;
            let path = app_dir.join("my_app.toml");
            std::fs::write(&path, default_config.trim())?;
            println!(
                "Successfully generated default config at {}",
                path.display()
            );
            Ok(())
        }
        Commands::Daemon { config } => daemon::run(&config).await,
        Commands::Status => client::status().await,
        Commands::Start { target } => client::start(&target).await,
        Commands::Stop { target } => client::stop(&target).await,
    }
}
