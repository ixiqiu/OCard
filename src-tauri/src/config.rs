//! 工作站配置（PRD §6.5：NAS 根路径按工作站各自配置）
//!
//! 配置文件：`~/.config/ocard/config.json`（各平台标准配置目录由 dirs 语义简化实现）。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    /// NAS 根路径（挂载点或盘符/UNC），项目文件夹创建于此
    pub nas_root: String,
    /// 当前登记的 DIT 操作人
    pub operator: String,
    /// 工作站机器 ID（日志按机器 ID 命名，PRD §6.3）
    pub machine_id: String,
    /// 默认相机编码（拷卡时自动带出）
    pub default_camera_code: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            nas_root: String::new(),
            operator: "DIT".to_string(),
            machine_id: default_machine_id(),
            default_camera_code: None,
        }
    }
}

/// 生成稳定的机器 ID：主机名 + 简短随机后缀
pub fn default_machine_id() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "ws".to_string());
    let rand = format!("{:04x}", rand_u32());
    format!("{}-{rand}", sanitize_host(&host))
}

fn sanitize_host(h: &str) -> String {
    h.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(16)
        .collect()
}

fn rand_u32() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // xorshift 风格，够用即可
    let mut x = nanos ^ 0x9E37_79B9;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// 配置文件目录：`{config_dir}/ocard/config.json`
pub fn config_file_path() -> PathBuf {
    let base = dirs_config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("ocard").join("config.json")
}

#[cfg(target_os = "windows")]
fn dirs_config_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn dirs_config_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn dirs_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

impl AppConfig {
    pub fn load() -> Self {
        Self::load_from(&config_file_path())
    }

    pub fn load_from(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&config_file_path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        let mut cfg = AppConfig::default();
        cfg.nas_root = "/Volumes/NAS".to_string();
        cfg.operator = "张三".to_string();
        cfg.save_to(&p).unwrap();
        let loaded = AppConfig::load_from(&p);
        assert_eq!(loaded, cfg);
        assert_eq!(loaded.nas_root, "/Volumes/NAS");
    }

    #[test]
    fn load_missing_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = AppConfig::load_from(&tmp.path().join("none.json"));
        assert!(!cfg.machine_id.is_empty());
        assert_eq!(cfg.operator, "DIT");
    }

    #[test]
    fn machine_id_stable_and_sanitized() {
        let a = default_machine_id();
        let b = default_machine_id();
        // 主机名相同但随机后缀不同 -> 不相等（除非极端巧合）
        assert_ne!(a, b);
        // 字符集安全
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }
}
