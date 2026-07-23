use tokio::process::Command;
use toolkit_rs::AppResult;
pub fn default_ipc_endpoint() -> String {
    #[cfg(unix)]
    {
        std::env::temp_dir()
            .join("supervisord.sock")
            .to_string_lossy()
            .into_owned()
    }

    #[cfg(windows)]
    {
        r"\\.\pipe\supervisord".to_string()
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

pub async fn terminate_process_tree(pid: u32) -> AppResult {
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

#[cfg(windows)]
pub fn process_memory_bytes(pid: u32) -> Option<u64> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    // SAFETY: the process handle is checked before use, the counters buffer has
    // the exact size expected by K32GetProcessMemoryInfo, and the handle is
    // always closed before returning.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle.is_null() {
            return None;
        }

        let mut counters: PROCESS_MEMORY_COUNTERS = zeroed();
        counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let success = K32GetProcessMemoryInfo(
            handle,
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        );
        CloseHandle(handle);

        (success != 0).then_some(counters.WorkingSetSize as u64)
    }
}

#[cfg(target_os = "linux")]
pub fn process_memory_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_linux_vm_rss(&status)
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn process_memory_bytes(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn parse_linux_vm_rss(status: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kibibytes = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    kibibytes.checked_mul(1024)
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

    #[cfg(windows)]
    #[test]
    fn reads_current_process_memory_on_windows() {
        assert!(process_memory_bytes(std::process::id()).is_some_and(|bytes| bytes > 0));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_linux_resident_memory() {
        assert_eq!(
            parse_linux_vm_rss("Name:\ttest\nVmRSS:\t  1234 kB\n"),
            Some(1_263_616)
        );
    }
}
