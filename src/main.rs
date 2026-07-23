use clap::{Parser, Subcommand};
use supervisorr::{client, daemon};

#[derive(Parser)]
#[command(name = "supervisorr")]
#[command(about = "A zero-dependency process manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match &cli.command {
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
            std::fs::create_dir_all(config)?;
            let path = std::path::Path::new(config).join("my_app.toml");
            std::fs::write(&path, default_config.trim())?;
            println!(
                "Successfully generated default config at {}",
                path.display()
            );
            Ok(())
        }
        Commands::Daemon { config } => daemon::run(config).await,
        Commands::Status => client::status().await,
        Commands::Start { target } => client::start(target).await,
        Commands::Stop { target } => client::stop(target).await,
    }
}
