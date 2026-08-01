use super::IiConfig;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const CONFIG_DIR_NAME: &str = "ii";
pub(crate) const CONFIG_FILE_NAME: &str = "ii.toml";

pub fn default_config_path() -> Result<PathBuf> {
    default_config_path_for(std::env::consts::OS, std::env::current_exe().ok())
}

pub(crate) fn default_config_path_for(os: &str, exe_path: Option<PathBuf>) -> Result<PathBuf> {
    if os == "windows" {
        let exe_path = exe_path.context("find current executable path")?;
        let exe_dir = exe_path
            .parent()
            .context("find current executable directory")?;
        return Ok(exe_dir.join(CONFIG_FILE_NAME));
    }
    Ok(PathBuf::from("/etc")
        .join(CONFIG_DIR_NAME)
        .join(CONFIG_FILE_NAME))
}

pub fn load_config(path: &Path) -> Result<IiConfig> {
    if !path.exists() {
        return Ok(IiConfig::default());
    }
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parse config {}", path.display()))
}

pub fn save_config(path: &Path, config: &IiConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create config dir {}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(config).context("serialize config")?;
    std::fs::write(path, raw).with_context(|| format!("write config {}", path.display()))
}
