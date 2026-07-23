mod client;
mod config;
mod daemon;
mod platform;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "supervisorr")]
#[command(about = "A zero-dependency process manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new default config file
    Init {
        #[arg(short, long, default_value = "./supervisorr.toml")]
        config: String,
    },
    /// Starts the supervisor daemon
    Daemon {
        #[arg(short, long, default_value = "/etc/supervisorr/supervisorr.toml")]
        config: String,
    },
    /// Status of processes
    Status,
    /// Start a process
    Start { target: String },
    /// Stop a process
    Stop { target: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Init { config } => {
            let default_config = r#"[supervisorr]
# socket_file = "/path/to/supervisorr.sock"
# On Windows, use a named pipe such as "supervisorr".

[program.my_app]
command = "echo 'Replace this with your process !'"
directory = "."
autostart = true
autorestart = true
stdout_logfile = "my_app.log"
stderr_logfile = "my_app.err"
"#;
            std::fs::write(config, default_config.trim())?;
            println!("Successfully generated default config at {}", config);
            Ok(())
        }
        Commands::Daemon { config } => daemon::run(config).await,
        Commands::Status => client::status().await,
        Commands::Start { target } => client::start(target).await,
        Commands::Stop { target } => client::stop(target).await,
    }
}
