// sc.exe query supervisord
// sc.exe stop supervisord
// sc.exe create supervisord binPath= "\"C:\ProgramData\supervisord\supervisord.exe\" service" DisplayName= Supervisord start= auto obj= LocalSystem
// sc.exe description supervisord "A watchdog developed by Rust, guarding the base system"
// sc.exe failure supervisord reset= 60 actions= restart/5000/restart/5000/restart/10000
// sc.exe start supervisord

#![allow(dead_code)]
use anyhow::anyhow;
use encoding_rs::GBK;
use toolkit_rs::AppResult;

use crate::iface::IInstall;

fn decode_windows_output(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => {
            let (text, _, _) = GBK.decode(bytes);
            text.into_owned()
        }
    }
}

fn install_default_dir(service_name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".into()),
    )
    .join(service_name)
}

fn exe_path(service_name: &str) -> std::path::PathBuf {
    install_default_dir(service_name).join(format!("{}.exe", service_name))
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> AppResult {
    if !src.is_dir() {
        return Err(anyhow!(
            "configuration directory not found: {}",
            src.display()
        ));
    }

    std::fs::create_dir_all(dst)
        .map_err(|e| anyhow!("failed to create {}: {}", dst.display(), e))?;

    for entry in
        std::fs::read_dir(src).map_err(|e| anyhow!("failed to read {}: {}", src.display(), e))?
    {
        let entry =
            entry.map_err(|e| anyhow!("failed to read entry in {}: {}", src.display(), e))?;
        let source_path = entry.path();
        let destination_path = dst.join(entry.file_name());

        if source_path.is_dir() {
            copy_dir(&source_path, &destination_path)?;
        } else {
            std::fs::copy(&source_path, &destination_path).map_err(|e| {
                anyhow!(
                    "failed to copy {} -> {}: {}",
                    source_path.display(),
                    destination_path.display(),
                    e
                )
            })?;
        }
    }

    Ok(())
}

fn exe_powershell(script: &str, what: &str) -> AppResult {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|e| anyhow!("failed to run powershell for {}: {}", what, e))?;

    if !out.status.success() {
        return Err(anyhow!(
            "{} failed: {}",
            what,
            decode_windows_output(&out.stderr).trim()
        ));
    }
    Ok(())
}

fn exe_sc(args: &[&str]) -> AppResult<std::process::Output> {
    let command = args
        .iter()
        .map(|arg| {
            if arg.contains(char::is_whitespace) {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                (*arg).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    log::info!("executing command: sc.exe {}", command);

    let out = std::process::Command::new("sc")
        .args(args)
        .output()
        .map_err(|e| anyhow!("failed to run sc {}: {}", args.first().unwrap_or(&""), e))?;
    Ok(out)
}

fn copy_service_file(
    service_name: &str,
    exe_configs_dirs: &[String],
) -> AppResult<std::path::PathBuf> {
    let src = std::env::current_exe().map_err(|e| anyhow!("current_exe(): {}", e))?;
    let dst = exe_path(service_name);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create {}: {}", parent.display(), e))?;
    }

    if src == dst {
        return Ok(src);
    }

    std::fs::copy(&src, &dst).map_err(|e| {
        anyhow!(
            "failed to copy {} -> {}: {}",
            src.display(),
            dst.display(),
            e
        )
    })?;

    let src_parent = src
        .parent()
        .ok_or_else(|| anyhow!("executable has no parent directory: {}", src.display()))?;
    let dst_parent = dst
        .parent()
        .ok_or_else(|| anyhow!("install path has no parent directory: {}", dst.display()))?;

    for dir in exe_configs_dirs {
        let relative_dir = std::path::Path::new(dir);
        if relative_dir.is_absolute()
            || relative_dir.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(anyhow!(
                "configuration directory must be relative to the executable: {}",
                dir
            ));
        }

        copy_dir(
            &src_parent.join(relative_dir),
            &dst_parent.join(relative_dir),
        )?;
    }

    Ok(dst)
}

fn remove_service_dir(service_name: &str) {
    let _ = std::fs::remove_dir_all(install_default_dir(service_name));
}

//sc.exe create supervisord binPath= "C:\ProgramData\supervisord.exe service"
fn register_service(exe: &std::path::Path, service_name: &str) -> AppResult {
    let bin_path = format!("\"{}\" service {}", exe.display(), service_name);

    let create = exe_sc(&[
        "create",
        service_name,
        "binPath=",
        &bin_path,
        "DisplayName=",
        "Supervisord",
        "start=",
        "auto",
        "obj=",
        "LocalSystem",
    ])?;
    if !create.status.success() {
        let out = decode_windows_output(&create.stdout);
        // "service already exists" is 1073 — treat as idempotent success.
        if !out.contains("1073") {
            return Err(anyhow!("sc create failed: {}", out.trim()));
        }
    }

    let _ = exe_sc(&[
        "description",
        service_name,
        "A watchdog developed by Rust, guarding the base system",
    ]);

    // Restart on crash: 5s, 5s, 10s; reset failure counter after 60s.
    let _ = exe_sc(&[
        "failure",
        service_name,
        "reset=",
        "60",
        "actions=",
        "restart/5000/restart/5000/restart/10000",
    ]);

    Ok(())
}

fn start_service(service_name: &str) -> AppResult {
    //sc.exe start supervisord
    let out = exe_sc(&["start", service_name])?;
    if !out.status.success() {
        let text = decode_windows_output(&out.stdout);
        // already running
        if text.contains("1056") {
            return Ok(());
        }
        return Err(anyhow!("sc start failed: {}", text.trim()));
    }
    Ok(())
}

fn stop_service(service_name: &str) {
    let _ = exe_sc(&["stop", service_name]);
    // Wait up to 10s for the service to reach STOPPED state so the
    // binary file handle is released before we try to overwrite it.
    for _ in 0..20 {
        if let Ok(out) = exe_sc(&["query", service_name]) {
            let text = decode_windows_output(&out.stdout);
            if text.contains("STOPPED") || text.contains("1060") {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    log::error!("warning: service did not stop within 10s");
}

fn delete_service(service_name: &str) {
    if let Err(e) = exe_sc(&["delete", service_name]) {
        log::warn!("sc delete failed: {}", e);
    }
}

fn service_status(service_name: &str) -> AppResult {
    let out = exe_sc(&["query", service_name])?;
    let text = decode_windows_output(&out.stdout);
    let display = parse_sc_state(&text);
    log::error!("{}\n", display);
    Ok(())
}

fn is_registered(service_name: &str) -> bool {
    exe_sc(&["query", service_name])
        .map(|o| {
            let stdout = decode_windows_output(&o.stdout);
            parse_sc_registered(o.status.success(), &stdout)
        })
        .unwrap_or(false)
}

fn parse_sc_registered(exit_success: bool, stdout: &str) -> bool {
    if exit_success {
        return true;
    }
    // Error 1060 = "The specified service does not exist as an installed service."
    !stdout.contains("1060")
}

fn parse_sc_state(sc_output: &str) -> String {
    if sc_output.contains("1060") {
        return "Service is not installed.".to_string();
    }
    sc_output
        .lines()
        .find(|l| l.contains("STATE"))
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub struct WindowsInstall {
    //可执行文件名
    pub exe_name: String,
    //配置文件 目录
    pub exe_configs_dirs: Vec<String>,
}

impl WindowsInstall {
    pub fn supervisord() -> Self {
        Self {
            exe_name: "supervisord".into(),
            exe_configs_dirs: vec!["./etc".into()],
        }
    }

    fn get_service_name(&self) -> &str {
        &self.exe_name
    }
}

impl IInstall for WindowsInstall {
    fn install(&self) -> AppResult {
        let service_name = self.get_service_name();
        log::info!("install service: {} start....", service_name);

        if is_registered(service_name) {
            log::info!("stopping existing service...");
            stop_service(service_name);
        }

        let service_exe = copy_service_file(service_name, &self.exe_configs_dirs)?;
        register_service(&service_exe, service_name)?;
        log::info!("register service: {} ok", service_name);

        start_service(service_name)?;
        log::info!("service start ok..");
        log::info!("install success....");
        Ok(())
    }
    fn uninstall(&self) -> AppResult {
        let service_name = self.get_service_name();
        stop_service(service_name);
        delete_service(service_name);
        remove_service_dir(service_name);
        log::info!("uninstalled");
        Ok(())
    }
}
