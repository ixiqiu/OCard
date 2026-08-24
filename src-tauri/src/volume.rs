//! 可移动卷检测（PRD §6.5）
//!
//! 平台适配：Windows 用盘符探测、macOS 用 /Volumes、Linux 用 /media|/run/media 挂载表。
//! 统一抽象为 Rust 层「卷事件」接口；兜底方案为轮询挂载表（PRD 明确允许）。
//! 这里提供 `scan_removable_volumes()` 轮询实现 + `VolumeEvent` 事件模型，
//! 上层通过定时轮询对比前后快照生成 insert/remove 事件。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 可移动卷
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Volume {
    /// 卷标/显示名
    pub name: String,
    /// 挂载点（Windows 为盘符，如 `E:\`）
    pub mount_point: PathBuf,
    /// 设备节点（如 /dev/sdb1），可能为空
    pub device: Option<String>,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// 卷事件（插入/移除），用于 UI 提示
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VolumeEvent {
    Inserted(Volume),
    Removed { name: String, mount_point: String },
}

/// 扫描当前可移动卷
pub fn scan_removable_volumes() -> Vec<Volume> {
    #[cfg(target_os = "windows")]
    {
        scan_windows()
    }
    #[cfg(target_os = "macos")]
    {
        scan_macos()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        scan_linux()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        Vec::new()
    }
}

/// 对比前后快照生成事件
pub fn diff_volumes(before: &[Volume], after: &[Volume]) -> Vec<VolumeEvent> {
    let mut events = Vec::new();
    for v in after {
        if !before.iter().any(|b| b.mount_point == v.mount_point) {
            events.push(VolumeEvent::Inserted(v.clone()));
        }
    }
    for b in before {
        if !after.iter().any(|a| a.mount_point == b.mount_point) {
            events.push(VolumeEvent::Removed {
                name: b.name.clone(),
                mount_point: b.mount_point.display().to_string(),
            });
        }
    }
    events
}

#[cfg(target_os = "windows")]
fn scan_windows() -> Vec<Volume> {
    // 用 PowerShell 查询可移动卷（避免 Win32 API feature 组合的编译风险）
    let script = "Get-CimInstance Win32_LogicalDisk -Filter \"DriveType=2\" | ForEach-Object { \"$($_.DeviceID)|$($_.VolumeName)|$($_.Size)|$($_.FreeSpace)\" }";
    let mut volumes = Vec::new();
    if let Ok(out) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 2 {
                    let name = if parts[1].is_empty() {
                        parts[0].to_string()
                    } else {
                        parts[1].to_string()
                    };
                    volumes.push(Volume {
                        name,
                        mount_point: PathBuf::from(parts[0].trim_end_matches('\\')),
                        device: Some(parts[0].to_string()),
                        total_bytes: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
                        available_bytes: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
                    });
                }
            }
        }
    }
    volumes
}

#[cfg(target_os = "macos")]
fn scan_macos() -> Vec<Volume> {
    // 解析 `mount` 输出，过滤 /Volumes 下的非系统卷
    let mut volumes = Vec::new();
    if let Ok(out) = std::process::Command::new("mount").output() {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let lower = line.to_lowercase();
            if !line.contains("/Volumes/") || lower.contains("/volumes/macos") || lower.contains("/volumes/recove") {
                continue;
            }
            let mp = line.split(" on ").nth(1).and_then(|s| s.split(" (").next());
            let dev = line.split(' ').next();
            if let Some(mp) = mp {
                let name = PathBuf::from(mp)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| mp.to_string());
                let (total, avail) = statvfs(mp);
                volumes.push(Volume {
                    name,
                    mount_point: PathBuf::from(mp),
                    device: dev.map(|d| d.to_string()),
                    total_bytes: total,
                    available_bytes: avail,
                });
            }
        }
    }
    volumes
}

#[cfg(all(unix, not(target_os = "macos")))]
fn scan_linux() -> Vec<Volume> {
    // 解析 /proc/mounts，过滤 /media 与 /run/media 下的设备挂载
    let mut volumes = Vec::new();
    if let Ok(content) = std::fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let dev = parts[0];
            let mp = parts[1];
            if mp.starts_with("/media/") || mp.starts_with("/run/media/") || mp.starts_with("/mnt/") {
                // 跳过伪设备
                if dev.starts_with('/') && !dev.starts_with("/dev/loop") {
                    let name = PathBuf::from(mp)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| mp.to_string());
                    let (total, avail) = statvfs(mp);
                    volumes.push(Volume {
                        name,
                        mount_point: PathBuf::from(mp),
                        device: Some(dev.to_string()),
                        total_bytes: total,
                        available_bytes: avail,
                    });
                }
            }
        }
    }
    volumes
}

/// statvfs 封装（非 Windows），字段统一转 u64 规避平台类型差异
#[cfg(unix)]
fn statvfs(path: &str) -> (u64, u64) {
    use std::mem::MaybeUninit;
    unsafe {
        let mut buf = MaybeUninit::<libc::statvfs>::uninit();
        let p = std::ffi::CString::new(path).unwrap_or_default();
        if libc::statvfs(p.as_ptr(), buf.as_mut_ptr()) == 0 {
            let s = buf.assume_init();
            let total = (s.f_blocks as u64).saturating_mul(s.f_frsize as u64);
            let avail = (s.f_bavail as u64).saturating_mul(s.f_frsize as u64);
            (total, avail)
        } else {
            (0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_detects_insert_and_remove() {
        let v1 = Volume {
            name: "A".to_string(),
            mount_point: PathBuf::from("/vol/A"),
            device: None,
            total_bytes: 0,
            available_bytes: 0,
        };
        let v2 = Volume {
            name: "B".to_string(),
            mount_point: PathBuf::from("/vol/B"),
            device: None,
            total_bytes: 0,
            available_bytes: 0,
        };
        let events = diff_volumes(&[], &[v1.clone()]);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], VolumeEvent::Inserted(_)));

        let events = diff_volumes(&[v1.clone()], &[v2]);
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|e| matches!(e, VolumeEvent::Removed { .. })));
        assert!(events.iter().any(|e| matches!(e, VolumeEvent::Inserted(_))));

        let events = diff_volumes(&[v1.clone()], &[v1]);
        assert!(events.is_empty());
    }
}
