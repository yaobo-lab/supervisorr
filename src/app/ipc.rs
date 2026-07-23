use crate::app::state::{Intent, SharedState, Status};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Serialize, Deserialize, Debug)]
pub enum IpcRequest {
    Status,
    Start { target: String },
    Stop { target: String },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum IpcResponse {
    StatusData(std::collections::HashMap<String, String>),
    Ok,
    Error(String),
}

#[cfg(unix)]
pub async fn setup_ipc(endpoint: &str, state: SharedState) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use tokio::net::UnixListener;

    if std::fs::metadata(endpoint).is_ok() {
        std::fs::remove_file(endpoint)?;
    }

    let listener = UnixListener::bind(endpoint)?;
    if let Ok(metadata) = std::fs::metadata(endpoint) {
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(endpoint, permissions)?;
    }
    println!("IPC server listening on {endpoint}");

    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream, state).await {
                eprintln!("Error handling IPC client: {error}");
            }
        });
    }
}

#[cfg(windows)]
pub async fn setup_ipc(endpoint: &str, state: SharedState) -> anyhow::Result<()> {
    use std::sync::Arc;
    use tokio::net::windows::named_pipe::ServerOptions;

    println!("IPC server listening on {endpoint}");
    loop {
        let server = ServerOptions::new().create(endpoint)?;
        server.connect().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(error) = handle_client(server, state).await {
                eprintln!("Error handling IPC client: {error}");
            }
        });
    }
}

async fn handle_client<S>(mut stream: S, state: SharedState) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..count]);
        match serde_json::from_slice::<IpcRequest>(&buffer) {
            Ok(request) => {
                let response = process_request(request, state).await;
                stream.write_all(&serde_json::to_vec(&response)?).await?;
                stream.shutdown().await?;
                return Ok(());
            }
            Err(error) if error.is_eof() => continue,
            Err(error) => {
                let response = IpcResponse::Error(format!("Invalid request: {error}"));
                stream.write_all(&serde_json::to_vec(&response)?).await?;
                stream.shutdown().await?;
                return Ok(());
            }
        }
    }
}

async fn process_request(request: IpcRequest, state: SharedState) -> IpcResponse {
    match request {
        IpcRequest::Status => {
            let state = state.read().await;
            let data = state
                .processes
                .iter()
                .map(|(name, process)| {
                    let status = match &process.status {
                        Status::Stopped => "STOPPED".to_string(),
                        Status::Running(pid) => format!("RUNNING (pid {pid})"),
                        Status::Exited(code) => format!("EXITED (code {code})"),
                        Status::Failed(error) => format!("FAILED ({error})"),
                    };
                    let intent = match process.intent {
                        Intent::Run => "intended: RUN",
                        Intent::Stop => "intended: STOP",
                    };
                    (name.clone(), format!("{status} [{intent}]"))
                })
                .collect();
            IpcResponse::StatusData(data)
        }
        IpcRequest::Start { target } => {
            let mut state = state.write().await;
            match state.processes.get_mut(&target) {
                Some(process) => {
                    process.intent = Intent::Run;
                    IpcResponse::Ok
                }
                None => IpcResponse::Error("Process not found".to_string()),
            }
        }
        IpcRequest::Stop { target } => {
            let pid = {
                let mut state = state.write().await;
                match state.processes.get_mut(&target) {
                    Some(process) => {
                        process.intent = Intent::Stop;
                        match process.status {
                            Status::Running(pid) => Some(pid),
                            _ => None,
                        }
                    }
                    None => return IpcResponse::Error("Process not found".to_string()),
                }
            };

            if let Some(pid) = pid
                && let Err(error) = crate::platform::terminate_process_tree(pid).await
            {
                return IpcResponse::Error(format!("Failed to stop process: {error}"));
            }
            IpcResponse::Ok
        }
    }
}
