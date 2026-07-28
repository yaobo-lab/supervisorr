#![allow(dead_code)]
use anyhow::anyhow;
use toolkit_rs::AppResult;

pub fn install_data_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".into()),
    )
    .join("supervisord")
}

fn install_exe_path() -> std::path::PathBuf {
    install_data_dir().join("bin").join("supervisord.exe")
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
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

fn exe_sc(args: &[&str]) -> AppResult<std::process::Output> {
    let out = std::process::Command::new("sc")
        .args(args)
        .output()
        .map_err(|e| anyhow!("failed to run sc {}: {}", args.first().unwrap_or(&""), e))?;
    Ok(out)
}

fn install_service_binary() -> AppResult<std::path::PathBuf> {
    let src = std::env::current_exe().map_err(|e| anyhow!("current_exe(): {}", e))?;
    let dst = install_exe_path();
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create {}: {}", parent.display(), e))?;
    }

    // Copy only if source and destination differ; running the binary from
    // its install location is a supported (re-install) case.
    if src != dst {
        std::fs::copy(&src, &dst).map_err(|e| {
            anyhow!(
                "failed to copy {} -> {}: {}",
                src.display(),
                dst.display(),
                e
            )
        })?;
    }
    Ok(dst)
}

fn remove_service_binary() {
    let _ = std::fs::remove_file(install_exe_path());
}

fn register_service_scm(exe: &std::path::Path) -> AppResult {
    let bin_path = format!("\"{}\" --service", exe.display());
    let name = super::service::SERVICE_NAME;

    // sc.exe uses a leading space as its `name= value` delimiter; the space
    // after `=` is mandatory. `depend= Dnscache` closes the boot-order race
    // where numa starts before the resolver Dnscache routes queries to it.
    let create = exe_sc(&[
        "create",
        name,
        "binPath=",
        &bin_path,
        "DisplayName=",
        "Supervisord",
        "start=",
        "auto",
        "obj=",
        "LocalSystem",
        "depend=",
        "Dnscache",
    ])?;
    if !create.status.success() {
        let out = String::from_utf8_lossy(&create.stdout);
        // "service already exists" is 1073 — treat as idempotent success.
        if !out.contains("1073") {
            return Err(anyhow!("sc create failed: {}", out.trim()));
        }
    }

    let _ = exe_sc(&[
        "description",
        name,
        "Self-sovereign DNS resolver (ad blocking, DoH/DoT, local zones).",
    ]);

    // Restart on crash: 5s, 5s, 10s; reset failure counter after 60s.
    let _ = exe_sc(&[
        "failure",
        name,
        "reset=",
        "60",
        "actions=",
        "restart/5000/restart/5000/restart/10000",
    ]);

    eprintln!("  Registered service '{}' (boot-time).", name);
    Ok(())
}

fn start_service_scm() -> AppResult {
    let out = exe_sc(&["start", super::service::SERVICE_NAME])?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout);
        // already running
        if text.contains("1056") {
            return Ok(());
        }
        return Err(anyhow!("sc start failed: {}", text.trim()));
    }
    Ok(())
}

fn stop_service_scm() {
    let name = super::service::SERVICE_NAME;
    let _ = exe_sc(&["stop", name]);
    // Wait up to 10s for the service to reach STOPPED state so the
    // binary file handle is released before we try to overwrite it.
    for _ in 0..20 {
        if let Ok(out) = exe_sc(&["query", name]) {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.contains("STOPPED") || text.contains("1060") {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    eprintln!(" warning: service did not stop within 10s");
}

fn delete_service_scm() {
    if let Err(e) = exe_sc(&["delete", super::service::SERVICE_NAME]) {
        log::warn!("sc delete failed: {}", e);
    }
}

fn service_status_windows() -> AppResult {
    let out = exe_sc(&["query", super::service::SERVICE_NAME])?;
    let text = String::from_utf8_lossy(&out.stdout);
    let display = parse_sc_state(&text);
    eprintln!("  {}\n", display);
    Ok(())
}

fn is_registered() -> bool {
    exe_sc(&["query", super::service::SERVICE_NAME])
        .map(|o| parse_sc_registered(o.status.success(), &String::from_utf8_lossy(&o.stdout)))
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

//
fn uninstall() -> AppResult {
    stop_service_scm();
    delete_service_scm();
    remove_service_binary();
    eprintln!("uninstalled.\n");
    Ok(())
}

//
fn install() -> AppResult {
    if is_registered() {
        eprintln!("stopping existing service...");
        stop_service_scm();
    }

    let service_exe = install_exe_path();
    register_service_scm(&service_exe)?;
    match start_service_scm() {
        Ok(_) => eprintln!("service started."),
        Err(e) => eprintln!("warning: service registered but could not start now: {}", e),
    }
    Ok(())
}
