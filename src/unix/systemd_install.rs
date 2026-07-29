use anyhow::anyhow;
use toolkit_rs::AppResult;

use crate::iface::IInstall;
const SYSTEMD_UNIT: &str = "/etc/systemd/system/supervisord.service";

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

fn cli_exe_path() -> std::path::PathBuf {
    std::path::PathBuf::from("/usr/local/bin/supervisord")
}

fn is_systemd_resolved_active() -> bool {
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

fn copy_binary() -> AppResult<std::path::PathBuf> {
    let src = std::env::current_exe().map_err(|e| anyhow!("current_exe(): {}", e))?;
    if path_world_traversable_linux(&src) {
        return Ok(src);
    }
    let dst = cli_exe_path();
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
    //可执行文件名
    pub exe_name: String,
    //配置文件 目录
    pub exe_configs_dirs: Vec<String>,
}

impl SystemdInstall {
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

impl IInstall for SystemdInstall {
    fn install(&self) -> AppResult {
        let exe = copy_binary()?;
        let unit = include_str!("../../supervisord.service")
            .replace("{{exe_path}}", &exe.to_string_lossy());

        std::fs::write(SYSTEMD_UNIT, unit)
            .map_err(|e| anyhow!("failed to write {}: {}", SYSTEMD_UNIT, e))?;

        exe_systemctl(&["daemon-reload"])?;
        exe_systemctl(&["enable", "supervisord"])?;
        exe_systemctl(&["restart", "supervisord"])?;
        log::info!("install ok..");
        Ok(())
    }

    fn uninstall(&self) -> AppResult {
        if let Err(e) = exe_systemctl(&["stop", "supervisord"]) {
            log::warn!("warning: {}", e);
        }
        if let Err(e) = exe_systemctl(&["disable", "supervisord"]) {
            log::warn!("warning: {}", e);
        }

        if let Err(e) = std::fs::remove_file(SYSTEMD_UNIT) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(anyhow!("failed to remove {}: {}", SYSTEMD_UNIT, e));
            }
        }
        let _ = exe_systemctl(&["daemon-reload"]);
        log::info!("uninstall ok..");
        Ok(())
    }
}
