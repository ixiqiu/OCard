//! manifest —— 拷卡校验结果落盘格式（PRD §5.3/§6.2）
//!
//! 每次拷卡任务完成生成一个 manifest 文件，存于项目 `.ocard/manifests/`。
//! 断点续传时读取已有 manifest，跳过已校验文件。

use serde::{Deserialize, Serialize};

pub const MANIFEST_VERSION: u32 = 1;
pub const HASH_ALGO: &str = "xxh3-64";

/// manifest 中的单个文件条目
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestFileEntry {
    /// 相对源卡根目录的路径
    pub path: String,
    /// 文件大小（字节）
    pub size: u64,
    /// xxHash3-64 hex
    pub hash: String,
    /// 是否已在目标回读校验通过
    pub verified: bool,
}

/// 一次拷卡任务的 manifest
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub version: u32,
    pub hash_algo: String,
    pub project_id: String,
    pub card_label: String,
    pub camera_code: String,
    pub operator: String,
    pub note: String,
    /// 源卡卷标识（挂载点/盘符）
    pub source_volume: String,
    /// 目的地列表（相对项目根）
    pub destinations: Vec<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub files: Vec<ManifestFileEntry>,
}

impl Manifest {
    pub fn new(
        project_id: &str,
        card_label: &str,
        camera_code: &str,
        operator: &str,
        note: &str,
        source_volume: &str,
        destinations: Vec<String>,
    ) -> Self {
        Self {
            version: MANIFEST_VERSION,
            hash_algo: HASH_ALGO.to_string(),
            project_id: project_id.to_string(),
            card_label: card_label.to_string(),
            camera_code: camera_code.to_string(),
            operator: operator.to_string(),
            note: note.to_string(),
            source_volume: source_volume.to_string(),
            destinations,
            started_at: chrono::Local::now().to_rfc3339(),
            completed_at: None,
            files: Vec::new(),
        }
    }

    /// 标记已完成
    pub fn finish(&mut self) {
        self.completed_at = Some(chrono::Local::now().to_rfc3339());
    }

    /// 已校验文件数
    pub fn verified_count(&self) -> usize {
        self.files.iter().filter(|f| f.verified).count()
    }

    /// 校验通过率（0.0 - 1.0）
    pub fn verify_ratio(&self) -> f64 {
        if self.files.is_empty() {
            return 0.0;
        }
        self.verified_count() as f64 / self.files.len() as f64
    }

    /// 全部通过？
    pub fn all_verified(&self) -> bool {
        !self.files.is_empty() && self.verified_count() == self.files.len()
    }

    /// 总字节数
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// 序列化为 JSON 字符串（pretty）
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 从 JSON 解析
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// 是否有某个相对路径已校验通过（断点续传判断）
    pub fn has_verified(&self, rel_path: &str, size: u64, hash: &str) -> bool {
        self.files
            .iter()
            .any(|f| f.path == rel_path && f.size == size && f.hash == hash && f.verified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        let mut m = Manifest::new(
            "20260824_某活动",
            "CARD-001",
            "DJIRONIN4D_B_ZS",
            "DIT-张三",
            "双卡备份",
            "E:",
            vec!["2. 原始素材".to_string(), "backup".to_string()],
        );
        m.files.push(ManifestFileEntry {
            path: "A001_0001.MP4".to_string(),
            size: 1024,
            hash: "abc".to_string(),
            verified: true,
        });
        m.files.push(ManifestFileEntry {
            path: "A001_0002.MP4".to_string(),
            size: 2048,
            hash: "def".to_string(),
            verified: false,
        });
        m
    }

    #[test]
    fn manifest_roundtrip_json() {
        let m = sample();
        let json = m.to_json().unwrap();
        let back = Manifest::from_json(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn manifest_stats() {
        let m = sample();
        assert_eq!(m.verified_count(), 1);
        assert_eq!(m.total_bytes(), 1024 + 2048);
        assert!(!m.all_verified());
        assert!((m.verify_ratio() - 0.5).abs() < 1e-9);

        let mut done = sample();
        done.files[1].verified = true;
        assert!(done.all_verified());
        assert_eq!(done.verify_ratio(), 1.0);
    }

    #[test]
    fn manifest_resume_skip() {
        let m = sample();
        assert!(m.has_verified("A001_0001.MP4", 1024, "abc"));
        assert!(!m.has_verified("A001_0001.MP4", 1024, "xyz"));
        assert!(!m.has_verified("A001_0001.MP4", 9999, "abc"));
        assert!(!m.has_verified("A001_0002.MP4", 2048, "def"));
    }

    #[test]
    fn empty_manifest_not_all_verified() {
        let m = Manifest::new("p", "c", "k", "o", "n", "s", vec![]);
        assert!(!m.all_verified());
        assert_eq!(m.verify_ratio(), 0.0);
    }
}
