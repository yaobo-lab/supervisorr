use toolkit_rs::AppResult;
pub trait IInstall {
    fn install(&self) -> AppResult;
    fn uninstall(&self) -> AppResult;
}
