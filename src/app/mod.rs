pub mod ipc;
pub mod state;

use crate::config::ProgramConfig;
use state::{AppState, Intent, ProcessState, SharedState, Status};
use std::fs::OpenOptions;
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::RwLock;
use toolkit_rs::AppResult;

pub async fn run(config_dir: &str) -> AppResult {
    let config_dir_path = std::path::Path::new(config_dir);
    if !config_dir_path.is_dir() {
        log::error!("Configuration directory not found at: {}", config_dir);
        log::error!("Run `supervisord init` to generate a default configuration directory.");
        std::process::exit(1);
    }
    log::info!("Starting supervisord daemon using config directory: {config_dir}");
    let config = crate::config::load_directory(config_dir_path)?;

    let state = Arc::new(RwLock::new(AppState::new(config.clone())));

    for (name, prog_config) in config.program.into_iter() {
        let intent = if prog_config.autostart {
            Intent::Run
        } else {
            Intent::Stop
        };
        state.write().await.processes.insert(
            name.clone(),
            ProcessState {
                intent,
                status: Status::Stopped,
            },
        );

        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            supervise_program(name, prog_config, state_clone).await;
        });
    }

    let socket_path = crate::platform::default_ipc_endpoint();
    let socket_path = crate::platform::normalize_ipc_endpoint(&socket_path);

    let state_clone = Arc::clone(&state);
    let socket_path_clone = socket_path.clone();
    tokio::spawn(async move {
        if let Err(e) = ipc::setup_ipc(&socket_path_clone, state_clone).await {
            eprintln!("IPC server failed: {}", e);
        }
    });

    #[cfg(feature = "web")]
    {
        let state_clone_web = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = crate::web::start(state_clone_web).await {
                eprintln!("Web server failed: {}", e);
            }
        });
    }

    wait_for_shutdown().await?;

    shutdown_processes(&state).await;

    // Cleanup socket file on exit
    #[cfg(unix)]
    let _ = std::fs::remove_file(&socket_path);

    Ok(())
}

pub(crate) async fn supervise_program(name: String, config: ProgramConfig, state: SharedState) {
    loop {
        let intent = {
            let s = state.read().await;
            s.processes
                .get(&name)
                .map(|ps| ps.intent)
                .unwrap_or(Intent::Stop)
        };

        if intent == Intent::Stop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            continue;
        }

        let mut cmd = crate::platform::command(&config.command);
        if let Some(dir) = &config.directory {
            cmd.current_dir(dir);
        }
        if let Some(envs) = &config.environment {
            cmd.envs(envs);
        }

        if let Some(out_log) = &config.stdout_logfile {
            if let Ok(file) = OpenOptions::new().create(true).append(true).open(out_log) {
                cmd.stdout(Stdio::from(file));
            } else {
                cmd.stdout(Stdio::null());
            }
        } else {
            cmd.stdout(Stdio::null());
        }

        if let Some(err_log) = &config.stderr_logfile {
            if let Ok(file) = OpenOptions::new().create(true).append(true).open(err_log) {
                cmd.stderr(Stdio::from(file));
            } else {
                cmd.stderr(Stdio::null());
            }
        } else {
            cmd.stderr(Stdio::null());
        }

        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id().unwrap_or(0);
                {
                    let mut s = state.write().await;
                    if let Some(ps) = s.processes.get_mut(&name) {
                        ps.status = Status::Running(pid);
                    }
                }

                let status = child.wait().await;

                let exit_code = match status {
                    Ok(exit_status) => exit_status.code().unwrap_or(-1),
                    Err(_) => -1,
                };

                {
                    let mut s = state.write().await;
                    if let Some(ps) = s.processes.get_mut(&name) {
                        ps.status = Status::Exited(exit_code);
                    }
                }
            }
            Err(e) => {
                let mut s = state.write().await;
                if let Some(ps) = s.processes.get_mut(&name) {
                    ps.status = Status::Failed(e.to_string());
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        let intent = {
            let s = state.read().await;
            s.processes
                .get(&name)
                .map(|ps| ps.intent)
                .unwrap_or(Intent::Stop)
        };

        if !config.autorestart {
            let mut s = state.write().await;
            if let Some(ps) = s.processes.get_mut(&name) {
                ps.intent = Intent::Stop;
            }
        }

        if intent == Intent::Stop || !config.autorestart {
            while {
                let s = state.read().await;
                s.processes
                    .get(&name)
                    .map(|ps| ps.intent)
                    .unwrap_or(Intent::Stop)
                    != Intent::Run
            } {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        } else {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }
}

async fn shutdown_processes(state: &SharedState) {
    let pids = {
        let mut state = state.write().await;
        state
            .processes
            .values_mut()
            .filter_map(|process| {
                process.intent = Intent::Stop;
                match process.status {
                    Status::Running(pid) => Some(pid),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
    };

    for pid in pids {
        if let Err(error) = crate::platform::terminate_process_tree(pid).await {
            eprintln!("Failed to stop child process {pid}: {error}");
        }
    }
}

#[cfg(unix)]
async fn wait_for_shutdown() -> AppResult {
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            result?;
            println!("Received SIGINT. Shutting down.");
        }
        _ = sigterm.recv() => {
            println!("Received SIGTERM. Shutting down.");
        }
    }
    Ok(())
}

#[cfg(windows)]
async fn wait_for_shutdown() -> AppResult {
    tokio::signal::ctrl_c().await?;
    println!("Received Ctrl+C. Shutting down.");
    Ok(())
}
