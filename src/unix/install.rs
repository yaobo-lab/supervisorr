const SYSTEMD_UNIT: &str = "/etc/systemd/system/supervisord.service";

fn run_systemctl(args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new("systemctl")
        .args(args)
        .status()
        .map_err(|e| format!("systemctl {} failed: {}", args.join(" "), e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "systemctl {} exited with {}",
            args.join(" "),
            status
        ))
    }
}

fn path_world_traversable_linux(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let mut current = p;
    while let Some(parent) = current.parent() {
        if parent.as_os_str().is_empty() || parent == std::path::Path::new("/") {
            break;
        }
        match std::fs::metadata(parent) {
            Ok(m) if m.permissions().mode() & 0o001 != 0 => {}
            _ => return false,
        }
        current = parent;
    }
    true
}

fn install(skip_system_dns: bool) -> Result<(), String> {
    let exe = install_service_binary_linux()?;
    let unit =
        include_str!("../../supervisord.service").replace("{{exe_path}}", &exe.to_string_lossy());

    std::fs::write(SYSTEMD_UNIT, unit)
        .map_err(|e| format!("failed to write {}: {}", SYSTEMD_UNIT, e))?;

    run_systemctl(&["daemon-reload"])?;
    run_systemctl(&["enable", "numa"])?;

    // Configure system DNS before starting numa so resolved releases port 53 first
    if !skip_system_dns {
        if let Err(e) = install_linux() {
            eprintln!("  warning: failed to configure system DNS: {}", e);
        }
    }

    // restart, not start: on re-install the service is already running
    // the previous binary; restart picks up the new one.
    run_systemctl(&["restart", "numa"])?;

    eprintln!("  Service installed and started (auto-starts on boot, restarts if killed)");
    eprintln!("  Logs: journalctl -u numa -f");
    Ok(())
}

fn install_service_binary_linux() -> Result<std::path::PathBuf, String> {
    let src = std::env::current_exe().map_err(|e| format!("current_exe(): {}", e))?;
    if path_world_traversable_linux(&src) {
        return Ok(src);
    }
    let dst = linux_service_exe_path();
    if src == dst {
        return Ok(dst);
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
    }
    // Atomic replace via temp + rename. Plain copy fails with ETXTBSY when
    // re-installing while the service is running the previous binary —
    // rename swaps the path while the running process keeps the old inode.
    let tmp = dst.with_extension("new");
    std::fs::copy(&src, &tmp).map_err(|e| {
        format!(
            "failed to copy {} -> {}: {}",
            src.display(),
            tmp.display(),
            e
        )
    })?;
    std::fs::rename(&tmp, &dst).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!(
            "failed to rename {} -> {}: {}",
            tmp.display(),
            dst.display(),
            e
        )
    })?;
    Ok(dst)
}
