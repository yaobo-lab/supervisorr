use crate::app::state::{Intent, SharedState, Status};
use axum::{
    Router,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use rust_embed::Embed;
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use toolkit_rs::AppResult;

#[derive(Embed)]
#[folder = "static/"]
struct Asset;

pub async fn start(state: SharedState) -> AppResult {
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
        );

    let config_bind = {
        let s = state.read().await;
        s.config.web.into_addr()
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
    mem: Option<f64>,
    command: String,
    directory: Option<String>,
    autostart: bool,
    autorestart: bool,
}

async fn api_status(state: SharedState) -> Json<Vec<ProcessStatusDto>> {
    let mut data = Vec::new();
    let s = state.read().await;
    for (name, ps) in &s.processes {
        let status_str = match &ps.status {
            Status::Stopped => "Stopped".to_string(),
            Status::Running(pid) => format!("Running (pid {})", pid),
            Status::Exited(c) => format!("Exited (code {})", c),
            Status::Failed(e) => format!("Failed: {}", e),
        };
        let mem = match ps.status {
            Status::Running(pid) => crate::platform::process_memory_bytes(pid)
                .map(|bytes| bytes as f64 / 1024.0 / 1024.0),
            _ => None,
        };
        let intent_str = match ps.intent {
            Intent::Run => "Run".to_string(),
            Intent::Stop => "Stop".to_string(),
        };
        let program = s.config.program.get(name);

        data.push(ProcessStatusDto {
            name: name.clone(),
            status: status_str,
            intent: intent_str,
            mem,
            command: program
                .map(|config| config.command.clone())
                .unwrap_or_default(),
            directory: program.and_then(|config| config.directory.clone()),
            autostart: program.is_some_and(|config| config.autostart),
            autorestart: program.is_some_and(|config| config.autorestart),
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
