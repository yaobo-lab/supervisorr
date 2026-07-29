use crate::iface::IInstall;
use anyhow::anyhow;
use toolkit_rs::AppResult;
pub struct Rk3588Install {
    pub exe_name: String,
    pub exe_configs_dirs: Vec<String>,
}

impl Rk3588Install {
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

impl IInstall for Rk3588Install {
    fn install(&self) -> AppResult {
        Err(anyhow!("unimplemented..."))
    }

    fn uninstall(&self) -> AppResult {
        Err(anyhow!("unimplemented..."))
    }
}
