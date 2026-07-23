use crate::app::ipc::{IpcRequest, IpcResponse};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use toolkit_rs::AppResult;
async fn exchange<S>(mut stream: S, request: IpcRequest) -> AppResult<IpcResponse>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream.write_all(&serde_json::to_vec(&request)?).await?;

    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    Ok(serde_json::from_slice(&buffer)?)
}

async fn send_request(request: IpcRequest) -> AppResult<IpcResponse> {
    let configured_endpoint = std::env::var("supervisord_IPC")
        .unwrap_or_else(|_| crate::platform::default_ipc_endpoint());
    let endpoint = crate::platform::normalize_ipc_endpoint(&configured_endpoint);

    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(endpoint).await?;
        exchange(stream, request).await
    }

    #[cfg(windows)]
    {
        let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)?;
        exchange(stream, request).await
    }
}

pub async fn status() -> AppResult {
    match send_request(IpcRequest::Status).await? {
        IpcResponse::StatusData(data) => {
            if data.is_empty() {
                println!("No processes configured.");
            }
            for (name, status) in data {
                println!("{name:<20} {status}");
            }
        }
        IpcResponse::Error(error) => println!("Error: {error}"),
        _ => println!("Unexpected response"),
    }
    Ok(())
}

pub async fn start(target: &str) -> AppResult {
    match send_request(IpcRequest::Start {
        target: target.to_string(),
    })
    .await?
    {
        IpcResponse::Ok => println!("Started {target}"),
        IpcResponse::Error(error) => println!("Error: {error}"),
        _ => println!("Unexpected response"),
    }
    Ok(())
}

pub async fn stop(target: &str) -> AppResult {
    match send_request(IpcRequest::Stop {
        target: target.to_string(),
    })
    .await?
    {
        IpcResponse::Ok => println!("Stopped {target}"),
        IpcResponse::Error(error) => println!("Error: {error}"),
        _ => println!("Unexpected response"),
    }
    Ok(())
}
