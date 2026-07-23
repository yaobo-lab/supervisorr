use std::path::Path;
use tokio::process::Command;

pub fn default_ipc_endpoint() -> String {
    #[cfg(unix)]
    {
        std::env::temp_dir()
            .join("supervisorr.sock")
            .to_string_lossy()
            .into_owned()
    }

    #[cfg(windows)]
    {
        r"\\.\pipe\supervisorr".to_string()
    }
}

pub fn normalize_ipc_endpoint(endpoint: &str) -> String {
    #[cfg(unix)]
    {
        endpoint.to_string()
    }

    #[cfg(windows)]
    {
        if endpoint.starts_with(r"\\.\pipe\") {
            endpoint.to_string()
        } else {
            format!(r"\\.\pipe\{endpoint}")
        }
    }
}

pub fn command(command_line: &str) -> Command {
    #[cfg(unix)]
    {
        let mut command = Command::new("sh");
        command.arg("-c").arg(command_line);
        command
    }

    #[cfg(windows)]
    {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/S", "/C", command_line]);
        command
    }
}

pub async fn terminate_process_tree(pid: u32) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        kill(Pid::from_raw(pid as i32), Signal::SIGTERM)?;
    }

    #[cfg(windows)]
    {
        let status = Command::new("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("taskkill failed with status {status}");
        }
    }

    Ok(())
}

pub async fn make_executable(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = tokio::fs::metadata(path).await?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        tokio::fs::set_permissions(path, permissions).await?;
    }

    #[cfg(windows)]
    {
        let _ = path;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_endpoint_is_not_empty() {
        assert!(!default_ipc_endpoint().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn windows_pipe_names_are_normalized() {
        assert_eq!(normalize_ipc_endpoint("custom"), r"\\.\pipe\custom");
        assert_eq!(
            normalize_ipc_endpoint(r"\\.\pipe\custom"),
            r"\\.\pipe\custom"
        );
    }
}
