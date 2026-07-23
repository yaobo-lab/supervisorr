use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process;
use toolkit_rs::{AppResult, config, logger::LogConfig};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct WebServerConf {
    pub port: u16,
    pub listen_addr: String,
}

impl WebServerConf {
    pub fn into_addr(&self) -> String {
        format!("{}:{}", self.listen_addr, self.port)
    }
    pub fn into_http_addr(&self) -> String {
        format!("http://{}:{}", self.listen_addr, self.port)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub log: LogConfig,
    pub web: WebServerConf,
    #[serde(default)]
    pub program: HashMap<String, ProgramConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProgramConfig {
    pub name: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default = "default_true")]
    pub autorestart: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_logfile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_logfile: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn load_basic(path: &str) -> Config {
    config::read_config::<Config>(path).unwrap_or_else(|error| {
        println!("read config err:{error}");
        process::exit(1);
    })
}

pub fn load_directory(path: &Path) -> AppResult<Config> {
    if !path.is_dir() {
        anyhow::bail!("Configuration path is not a directory: {}", path.display());
    }

    let base_path = path.join("config.toml");
    if !base_path.is_file() {
        anyhow::bail!("Base configuration file not found: {}", base_path.display());
    }

    let mut config: Config = load_basic(&base_path.to_string_lossy());
    if !config.program.is_empty() {
        anyhow::bail!(
            "{} must contain only base settings; move [program] to app/",
            base_path.display()
        );
    }

    let app_dir = path.join("app");
    if !app_dir.is_dir() {
        anyhow::bail!(
            "Program configuration directory not found: {}",
            app_dir.display()
        );
    }

    let mut entries = std::fs::read_dir(&app_dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let file_path = entry.path();
        if file_path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }

        let program = match config::read_config::<ProgramConfig>(&file_path.to_string_lossy()) {
            Ok(program) => program,
            Err(e) => {
                log::error!(
                    "program config file:{},err:{}",
                    file_path.to_string_lossy(),
                    e
                );
                continue;
            }
        };

        if config
            .program
            .insert(program.name.clone(), program.clone())
            .is_some()
        {
            anyhow::bail!(
                "Duplicate program name {:?} in {}",
                program.name,
                file_path.display()
            );
        }
    }

    Ok(config)
}
