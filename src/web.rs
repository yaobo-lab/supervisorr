use crate::config::ProgramConfig;
use crate::daemon::state::{Intent, ProcessState, SharedState, Status};
use axum::extract::Multipart;
use axum::{
    Router,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use rust_embed::Embed;
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::fs;

#[derive(Embed)]
#[folder = "static/"]
struct Asset;

pub async fn start_web(state: SharedState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(index_handler))
        .route(
            "/api/status",
            get({
                let state = Arc::clone(&state);
                move || api_status(state)
            }),
        )
        .route(
            "/api/action",
            post({
                let state = Arc::clone(&state);
                move |payload| api_action(state, payload)
            }),
        )
        .route(
            "/api/upload",
            post({
                let state = Arc::clone(&state);
                move |multipart| api_upload(state, multipart)
            }),
        )
        .route(
            "/api/tunnel",
            post({
                let state = Arc::clone(&state);
                move |payload| api_tunnel(state, payload)
            }),
        )
        .route(
            "/api/tunnel_url",
            post({
                let state = Arc::clone(&state);
                move |payload| api_tunnel_url(state, payload)
            }),
        )
        .layer(axum::extract::DefaultBodyLimit::disable());

    let config_bind = {
        let s = state.read().await;
        s.config
            .web
            .as_ref()
            .map(|web| web.into_addr())
            .or_else(|| {
                s.config
                    .supervisord
                    .as_ref()
                    .and_then(|sup| sup.web_bind.clone())
            })
            .unwrap_or_else(|| "127.0.0.1:3000".to_string())
    };

    let addr: SocketAddr = config_bind
        .parse()
        .unwrap_or_else(|_| "127.0.0.1:3000".parse().unwrap());
    println!("Web Dashboard listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index_handler() -> impl IntoResponse {
    match Asset::get("index.html") {
        Some(file) => axum::response::Response::builder()
            .header("content-type", "text/html; charset=utf-8")
            .body(axum::body::Body::from(file.data))
            .unwrap(),
        None => axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::from("UI not found"))
            .unwrap(),
    }
}

#[derive(Serialize)]
struct ProcessStatusDto {
    name: String,
    status: String,
    intent: String,
    memory_bytes: Option<u64>,
    tunnel_domain: Option<String>,
}

async fn api_status(state: SharedState) -> Json<Vec<ProcessStatusDto>> {
    let mut data = Vec::new();
    let s = state.read().await;
    for (name, ps) in &s.processes {
        if name.starts_with("_tunnel_") {
            continue;
        }
        let status_str = match &ps.status {
            Status::Stopped => "Stopped".to_string(),
            Status::Running(pid) => format!("Running (pid {})", pid),
            Status::Exited(c) => format!("Exited (code {})", c),
            Status::Failed(e) => format!("Failed: {}", e),
        };
        let memory_bytes = match ps.status {
            Status::Running(pid) => crate::platform::process_memory_bytes(pid),
            _ => None,
        };
        let intent_str = match ps.intent {
            Intent::Run => "Run".to_string(),
            Intent::Stop => "Stop".to_string(),
        };

        let tunnel_domain = s.config.program.get(name).and_then(|p| {
            p.tunnel.as_ref().map(|t| {
                if t.is_quick {
                    "Quick Tunnel (Check Logs)".to_string()
                } else {
                    t.domain.clone()
                }
            })
        });

        data.push(ProcessStatusDto {
            name: name.clone(),
            status: status_str,
            intent: intent_str,
            memory_bytes,
            tunnel_domain,
        });
    }
    data.sort_by(|a, b| a.name.cmp(&b.name));
    Json(data)
}

#[derive(serde::Deserialize)]
struct ActionPayload {
    action: String,
    target: String,
}

#[derive(Serialize)]
struct ActionResponse {
    success: bool,
    error: Option<String>,
}

async fn api_action(
    state: SharedState,
    axum::Json(payload): axum::Json<ActionPayload>,
) -> Json<ActionResponse> {
    let pid = {
        let mut s = state.write().await;
        let target = &payload.target;
        let Some(ps) = s.processes.get_mut(target) else {
            return Json(ActionResponse {
                success: false,
                error: Some("Process not found".to_string()),
            });
        };
        if payload.action == "start" {
            ps.intent = Intent::Run;
            return Json(ActionResponse {
                success: true,
                error: None,
            });
        } else if payload.action == "stop" {
            ps.intent = Intent::Stop;
            match ps.status {
                Status::Running(pid) => Some(pid),
                _ => None,
            }
        } else {
            return Json(ActionResponse {
                success: false,
                error: Some("Unknown action".to_string()),
            });
        }
    };

    if let Some(pid) = pid
        && let Err(error) = crate::platform::terminate_process_tree(pid).await
    {
        return Json(ActionResponse {
            success: false,
            error: Some(format!("Failed to stop process: {error}")),
        });
    }
    Json(ActionResponse {
        success: true,
        error: None,
    })
}

#[derive(serde::Deserialize)]
struct TunnelPayload {
    action: String,
    target: String,
    domain: Option<String>,
    port: Option<u16>,
}
async fn resolve_cloudflared() -> Result<String, String> {
    let executable_name = if cfg!(windows) {
        "cloudflared.exe"
    } else {
        "cloudflared"
    };

    // Check common locations
    for path in [
        executable_name,
        "/usr/local/bin/cloudflared",
        "/usr/bin/cloudflared",
    ] {
        let check = tokio::process::Command::new(path)
            .arg("--version")
            .output()
            .await;
        if check.is_ok() && check.unwrap().status.success() {
            return Ok(path.to_string());
        }
    }

    // Check the per-user location.
    if let Some(home) = std::env::var_os("HOME") {
        let local_path = if cfg!(windows) {
            std::path::PathBuf::from(home).join("cloudflared.exe")
        } else {
            std::path::PathBuf::from(home).join(".local/bin/cloudflared")
        };
        let check = tokio::process::Command::new(&local_path)
            .arg("--version")
            .output()
            .await;
        if check.is_ok() && check.unwrap().status.success() {
            return Ok(local_path.to_string_lossy().into_owned());
        }
    }

    // Check current directory
    let cwd = std::env::current_dir().unwrap_or_default();
    let local_bin = cwd.join(executable_name);
    if local_bin.exists() {
        return Ok(local_bin.to_string_lossy().to_string());
    }

    // Auto-download
    let arch = std::env::consts::ARCH;
    let arch_suffix = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return Err(format!("Unsupported architecture: {}", arch)),
    };
    let (platform, extension) = if cfg!(windows) {
        ("windows", ".exe")
    } else {
        ("linux", "")
    };
    let url = format!(
        "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-{platform}-{arch_suffix}.exe"
    );
    let url = if extension.is_empty() {
        url.trim_end_matches(".exe").to_string()
    } else {
        url
    };
    let dest = local_bin.to_string_lossy().to_string();

    let download = tokio::process::Command::new("curl")
        .args(["-L", &url, "-o", &dest])
        .output()
        .await;

    match download {
        Ok(out) if out.status.success() => {
            let _ = crate::platform::make_executable(std::path::Path::new(&dest)).await;
            Ok(dest)
        }
        _ => Err("Failed to download cloudflared. Install it manually: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/".to_string()),
    }
}

async fn detect_port(pid: u32) -> Option<u16> {
    #[cfg(windows)]
    {
        let _ = pid;
        None
    }

    #[cfg(unix)]
    {
        // Use ss to find listening ports for this specific PID
        let output = tokio::process::Command::new("ss")
            .args(["-tlnp"])
            .output()
            .await
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let pid_str = format!("pid={}", pid);

        for line in stdout.lines() {
            if line.contains(&pid_str) && line.contains("LISTEN") {
                // Parse port from address column like "0.0.0.0:28019" or "*:8080"
                for part in line.split_whitespace() {
                    if part.contains(':') && !part.contains("pid=") {
                        if let Some(port_str) = part.rsplit(':').next() {
                            if let Ok(port) = port_str.parse::<u16>() {
                                return Some(port);
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

async fn api_tunnel(
    state: SharedState,
    axum::Json(payload): axum::Json<TunnelPayload>,
) -> Json<ActionResponse> {
    let cloudflared = match resolve_cloudflared().await {
        Ok(path) => path,
        Err(e) => {
            return Json(ActionResponse {
                success: false,
                error: Some(e),
            });
        }
    };

    let mut s = state.write().await;
    let target = &payload.target;

    if !s.processes.contains_key(target) {
        return Json(ActionResponse {
            success: false,
            error: Some("Process not found".to_string()),
        });
    }

    if payload.action == "start" {
        let domain_str = payload.domain.unwrap_or_default();
        let is_quick = domain_str.trim().is_empty();
        let domain_final = if is_quick {
            "quick".to_string()
        } else {
            domain_str.trim().to_string()
        };

        // Auto-detect port from the running process
        let detected_port = if let Some(ps) = s.processes.get(target) {
            if let Status::Running(pid) = ps.status {
                detect_port(pid).await
            } else {
                None
            }
        } else {
            None
        };
        let port_final = detected_port.or(payload.port).unwrap_or(8080);
        let t_config = crate::config::TunnelConfig {
            domain: domain_final.clone(),
            port: port_final,
            is_quick,
        };

        let updated_program = if let Some(prog) = s.config.program.get_mut(target) {
            prog.tunnel = Some(t_config.clone());
            prog.clone()
        } else {
            return Json(ActionResponse {
                success: false,
                error: Some("Process not found".to_string()),
            });
        };
        let config_dir = s.config_dir.clone();

        drop(s);

        if let Err(error) =
            crate::config::save_program(std::path::Path::new(&config_dir), target, &updated_program)
        {
            return Json(ActionResponse {
                success: false,
                error: Some(format!("Failed to save program config: {error}")),
            });
        }

        let state_clone = state.clone();
        let target_clone = target.clone();
        let cf_bin = cloudflared.clone();
        tokio::spawn(async move {
            let tunnel_prog_name = format!("_tunnel_{}", target_clone);
            let command = if is_quick {
                format!("{} tunnel --url http://127.0.0.1:{}", cf_bin, port_final)
            } else {
                let _ = tokio::process::Command::new(&cf_bin)
                    .args(["tunnel", "create", &tunnel_prog_name])
                    .output()
                    .await;
                let _ = tokio::process::Command::new(&cf_bin)
                    .args(["tunnel", "route", "dns", &tunnel_prog_name, &domain_final])
                    .output()
                    .await;
                format!("{} tunnel run {}", cf_bin, tunnel_prog_name)
            };

            let new_prog = crate::config::ProgramConfig {
                command,
                directory: None,
                autostart: true,
                autorestart: true,
                environment: None,
                stdout_logfile: Some(format!("{}.log", tunnel_prog_name)),
                stderr_logfile: Some(format!("{}.err", tunnel_prog_name)),
                tunnel: None,
            };

            {
                let mut ss = state_clone.write().await;
                ss.processes.insert(
                    tunnel_prog_name.clone(),
                    ProcessState {
                        intent: Intent::Run,
                        status: Status::Stopped,
                    },
                );
            }

            crate::daemon::supervise_program(tunnel_prog_name, new_prog, state_clone).await;
        });

        Json(ActionResponse {
            success: true,
            error: None,
        })
    } else if payload.action == "stop" {
        let updated_program = if let Some(prog) = s.config.program.get_mut(target) {
            prog.tunnel = None;
            prog.clone()
        } else {
            return Json(ActionResponse {
                success: false,
                error: Some("Process not found".to_string()),
            });
        };
        let config_dir = s.config_dir.clone();

        let tunnel_prog_name = format!("_tunnel_{}", target);
        let pid = if let Some(ps) = s.processes.get_mut(&tunnel_prog_name) {
            ps.intent = Intent::Stop;
            match ps.status {
                Status::Running(pid) => Some(pid),
                _ => None,
            }
        } else {
            None
        };
        drop(s);
        if let Err(error) =
            crate::config::save_program(std::path::Path::new(&config_dir), target, &updated_program)
        {
            return Json(ActionResponse {
                success: false,
                error: Some(format!("Failed to save program config: {error}")),
            });
        }
        if let Some(pid) = pid
            && let Err(error) = crate::platform::terminate_process_tree(pid).await
        {
            return Json(ActionResponse {
                success: false,
                error: Some(format!("Failed to stop tunnel: {error}")),
            });
        }
        Json(ActionResponse {
            success: true,
            error: None,
        })
    } else {
        Json(ActionResponse {
            success: false,
            error: Some("Unknown action".to_string()),
        })
    }
}

#[derive(serde::Deserialize)]
struct TunnelUrlPayload {
    target: String,
}

#[derive(Serialize)]
struct TunnelUrlResponse {
    url: Option<String>,
}

async fn api_tunnel_url(
    _state: SharedState,
    axum::Json(payload): axum::Json<TunnelUrlPayload>,
) -> Json<TunnelUrlResponse> {
    let err_path = format!("_tunnel_{}.err", payload.target);
    let log_path = format!("_tunnel_{}.log", payload.target);

    // cloudflared prints the URL to stderr typically, but check both
    for path in [&err_path, &log_path] {
        if let Ok(contents) = tokio::fs::read_to_string(path).await {
            for line in contents.lines().rev() {
                if line.contains("trycloudflare.com") || line.contains("cfargotunnel.com") {
                    // Extract URL from the line
                    for word in line.split_whitespace() {
                        if word.starts_with("https://") {
                            return Json(TunnelUrlResponse {
                                url: Some(word.trim().to_string()),
                            });
                        }
                    }
                }
            }
        }
    }
    Json(TunnelUrlResponse { url: None })
}

async fn api_upload(state: SharedState, mut multipart: Multipart) -> Json<ActionResponse> {
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("executable") {
            let mut raw_name = field.file_name().unwrap_or("uploaded_bin").to_string();
            if let Some(clean) = std::path::Path::new(&raw_name).file_name() {
                raw_name = clean.to_string_lossy().to_string();
            }
            let file_name = raw_name;

            let data = match field.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    return Json(ActionResponse {
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            };

            let current_dir = std::env::current_dir().unwrap_or_default();
            let path = current_dir.join(&file_name);
            if let Err(e) = fs::write(&path, data).await {
                return Json(ActionResponse {
                    success: false,
                    error: Some(format!("Failed to save: {}", e)),
                });
            }

            let _ = crate::platform::make_executable(&path).await;

            let new_prog = ProgramConfig {
                command: path.to_string_lossy().to_string(),
                directory: Some(current_dir.to_string_lossy().to_string()),
                autostart: true,
                autorestart: true,
                environment: None,
                stdout_logfile: Some(
                    current_dir
                        .join(format!("{}.log", file_name))
                        .to_string_lossy()
                        .to_string(),
                ),
                stderr_logfile: Some(
                    current_dir
                        .join(format!("{}.err", file_name))
                        .to_string_lossy()
                        .to_string(),
                ),
                tunnel: None,
            };

            let config_dir = {
                let s = state.read().await;
                s.config_dir.clone()
            };
            if let Err(error) = crate::config::save_program(
                std::path::Path::new(&config_dir),
                &file_name,
                &new_prog,
            ) {
                return Json(ActionResponse {
                    success: false,
                    error: Some(format!("Failed to save program config: {error}")),
                });
            }

            {
                let mut s = state.write().await;
                s.config.program.insert(file_name.clone(), new_prog.clone());
                s.processes.insert(
                    file_name.clone(),
                    ProcessState {
                        intent: Intent::Run,
                        status: Status::Stopped,
                    },
                );
            }

            let state_clone = state.clone();
            tokio::spawn(async move {
                crate::daemon::supervise_program(file_name, new_prog, state_clone).await;
            });

            return Json(ActionResponse {
                success: true,
                error: None,
            });
        }
    }
    Json(ActionResponse {
        success: false,
        error: Some("No executable found".to_string()),
    })
}
