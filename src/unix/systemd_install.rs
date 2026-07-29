use crate::iface::IInstall;
use anyhow::anyhow;
use toolkit_rs::AppResult;

fn exe_systemctl(args: &[&str]) -> AppResult {
    let status = std::process::Command::new("systemctl")
        .args(args)
        .status()
        .map_err(|e| anyhow!("systemctl {} failed: {}", args.join(" "), e))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "systemctl {} exited with {}",
            args.join(" "),
            status
        ))
    }
}

fn cli_path(service_name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("/usr/local/bin/{}", service_name))
}

fn _is_systemd_resolved_active() -> bool {
    std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", "systemd-resolved"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/*
home/yaobo/rust/project/target/release/supervisord

---每一级目录都必须设置“其他用户可进入”权限，即 Unix 权限中的 o+x
/home/yaobo/rust/project/target/release
/home/yaobo/rust/project/target
/home/yaobo/rust/project
/home/yaobo/rust
/home/yaobo
/home
*/
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

fn copy_binary(service_name: &str) -> AppResult<std::path::PathBuf> {
    let src = std::env::current_exe().map_err(|e| anyhow!("current_exe(): {}", e))?;
    let dst = cli_path(service_name);

    if path_world_traversable_linux(&src) {
        //copy cli
        std::fs::copy(&src, &dst).map_err(|e| {
            anyhow!(
                "failed to copy {} -> {}: {}",
                src.display(),
                dst.display(),
                e
            )
        })?;

        return Ok(src);
    }
    if src == dst {
        return Ok(dst);
    }

    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create {}: {}", parent.display(), e))?;
    }

    let tmp = dst.with_extension("new");
    std::fs::copy(&src, &tmp).map_err(|e| {
        anyhow!(
            "failed to copy {} -> {}: {}",
            src.display(),
            tmp.display(),
            e
        )
    })?;
    std::fs::rename(&tmp, &dst).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        anyhow!(
            "failed to rename {} -> {}: {}",
            tmp.display(),
            dst.display(),
            e
        )
    })?;
    Ok(dst)
}

pub struct SystemdInstall {
    bin_name: String,
    systemd_unit_path: String,
}

impl SystemdInstall {
    pub fn supervisord() -> Self {
        Self {
            bin_name: "supervisord".into(),
            systemd_unit_path: "/etc/systemd/system/supervisord.service".into(),
        }
    }

    pub fn supervisord_systemd_file(&self) -> &'static str {
        "[Unit]
Description=Supervisord Process Supervisor
After=network.target

[Service]
Type=simple
WorkingDirectory={{WorkingDir}}
ExecStart={{ExePath}} daemon
Restart=on-failure
RestartSec=3s
TimeoutStopSec=30s
KillMode=control-group

[Install]
WantedBy=multi-user.target"
    }
    fn get_service_name(&self) -> &str {
        &self.bin_name
    }
}

impl IInstall for SystemdInstall {
    fn install(&self) -> AppResult {
        let src = std::env::current_exe().map_err(|e| anyhow!("current_exe(): {}", e))?;

        let Some(parent) = src.parent() else {
            return Err(anyhow!("install WorkingDirectory is emtpy"));
        };

        let service_name = self.get_service_name();
        let exe = copy_binary(service_name)?;
        let unit = self
            .supervisord_systemd_file()
            .replace("{{ExePath}}", &exe.to_string_lossy())
            .replace("{{WorkingDir}}", &parent.to_string_lossy());

        std::fs::write(self.systemd_unit_path.as_str(), unit)
            .map_err(|e| anyhow!("failed to write {}: {}", self.systemd_unit_path, e))?;

        exe_systemctl(&["daemon-reload"])?;
        exe_systemctl(&["enable", service_name])?;
        exe_systemctl(&["restart", service_name])?;
        log::info!("install ok..");
        Ok(())
    }

    fn uninstall(&self) -> AppResult {
        let service_name = self.get_service_name();

        if let Err(e) = exe_systemctl(&["stop", service_name]) {
            log::warn!("warning: {}", e);
        }
        if let Err(e) = exe_systemctl(&["disable", service_name]) {
            log::warn!("warning: {}", e);
        }

        if let Err(e) = std::fs::remove_file(&self.systemd_unit_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(anyhow!(
                    "failed to remove {}: {}",
                    self.systemd_unit_path,
                    e
                ));
            }
        }
        let _ = exe_systemctl(&["daemon-reload"]);
        log::info!("uninstall ok..");
        Ok(())
    }
}
