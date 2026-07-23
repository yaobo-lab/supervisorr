use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intent {
    Run,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Stopped,
    Running(u32),   // pid
    Exited(i32),    // exit code
    Failed(String), // e.g. command not found
}

#[derive(Debug, Clone)]
pub struct ProcessState {
    pub intent: Intent,
    pub status: Status,
}

use crate::config::Config;

pub struct AppState {
    pub processes: HashMap<String, ProcessState>,
    pub config: Config,
    pub config_dir: String,
}

impl AppState {
    pub fn new(config: Config, config_dir: String) -> Self {
        Self {
            processes: HashMap::new(),
            config,
            config_dir,
        }
    }
}

pub type SharedState = Arc<RwLock<AppState>>;
