use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervisorr: Option<SupervisorrConfig>,
    #[serde(default)]
    pub program: HashMap<String, ProgramConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    supervisorr: Option<SupervisorrConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    program: Option<NamedProgramConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct NamedProgramConfig {
    name: String,
    #[serde(flatten)]
    config: ProgramConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SupervisorrConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_bind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TunnelConfig {
    pub domain: String,
    pub port: u16,
    pub is_quick: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProgramConfig {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<TunnelConfig>,
}

fn default_true() -> bool {
    true
}

pub fn load_directory(path: &Path) -> anyhow::Result<Config> {
    if !path.is_dir() {
        anyhow::bail!("Configuration path is not a directory: {}", path.display());
    }

    let mut config = Config {
        supervisorr: None,
        program: HashMap::new(),
    };
    let mut entries = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let file_path = entry.path();
        if file_path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }

        let contents = std::fs::read_to_string(&file_path)?;
        let file: ConfigFile = toml::from_str(&contents)
            .map_err(|error| anyhow::anyhow!("{}: {error}", file_path.display()))?;

        if let Some(supervisorr) = file.supervisorr
            && config.supervisorr.replace(supervisorr).is_some()
        {
            anyhow::bail!(
                "Multiple [supervisorr] sections found; latest file: {}",
                file_path.display()
            );
        }

        if let Some(program) = file.program
            && config
                .program
                .insert(program.name.clone(), program.config)
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

pub fn save_program(
    directory: &Path,
    name: &str,
    config: &ProgramConfig,
) -> anyhow::Result<PathBuf> {
    validate_program_name(name)?;
    std::fs::create_dir_all(directory)?;

    let file = ConfigFile {
        supervisorr: None,
        program: Some(NamedProgramConfig {
            name: name.to_string(),
            config: config.clone(),
        }),
    };
    let path = directory.join(format!("{name}.toml"));
    std::fs::write(&path, toml::to_string_pretty(&file)?)?;
    Ok(path)
}

fn validate_program_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        anyhow::bail!("Invalid program name: {name:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_program_file_format() {
        let file: ConfigFile = toml::from_str(
            r#"
[program]
name = "my_app"
command = "echo hello"
autostart = true
autorestart = false
"#,
        )
        .unwrap();

        let program = file.program.unwrap();
        assert_eq!(program.name, "my_app");
        assert_eq!(program.config.command, "echo hello");
        assert!(!program.config.autorestart);
    }

    #[test]
    fn rejects_program_names_that_escape_the_config_directory() {
        assert!(validate_program_name("../outside").is_err());
        assert!(validate_program_name(r"..\outside").is_err());
    }
}
