//! 项目管理（PRD §5.2）
//!
//! 新建项目向导：选日期 + 项目名 -> 生成 `YYYYMMDD_项目名` 项目文件夹于 NAS。
//! 项目状态全部存于 NAS 项目夹 `.ocard/`，两台工作站打开同一项目即共享登记表、
//! 拷卡记录、分类进度（PRD §6.3）。

use crate::naming;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 项目元数据（存于 `.ocard/project.json`）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// YYYYMMDD
    pub date: String,
    /// "A"（视频剪辑）或 "B"（纯拍照）
    pub workflow: String,
    /// 工况 B 自定义分类
    pub categories: Vec<String>,
    pub created_at: String,
}

/// 项目状态汇总（项目列表页展示，PRD §5.2）
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub workflow: String,
    /// 已拷卡张数
    pub copied_files: u64,
    /// 已拷容量（字节）
    pub copied_bytes: u64,
    /// 分类进度：已分类张数 / 待分类张数
    pub classified: u64,
    pub unclassified: u64,
    /// 备份状态：true 表示至少一个 manifest 全部校验通过
    pub backup_ok: bool,
    /// 交付状态：true 表示已打包
    pub packaged: bool,
}

impl Project {
    /// 建夹模板
    pub fn folder_template(&self) -> Vec<String> {
        if self.workflow == "A" {
            naming::workflow_a_folders()
        } else {
            naming::workflow_b_folders(&self.categories)
        }
    }

    /// 创建项目（在 nas_root 下建 `YYYYMMDD_项目名` 目录 + .ocard + 模板夹）
    pub fn create(
        nas_root: &Path,
        date: &str,
        name: &str,
        workflow: &str,
        categories: &[String],
    ) -> Result<Project, String> {
        let wf = workflow.trim();
        if wf != "A" && wf != "B" {
            return Err("工况必须为 A 或 B".to_string());
        }
        let id = naming::project_id(date, name);
        if !date.chars().all(|c| c.is_ascii_digit()) || date.len() != 8 {
            return Err("日期必须为 YYYYMMDD".to_string());
        }
        let root = nas_root.join(&id);
        if root.exists() {
            return Err(format!("项目已存在：{id}"));
        }
        let project = Project {
            id: id.clone(),
            name: naming::sanitize_filename(name),
            date: date.to_string(),
            workflow: wf.to_string(),
            categories: categories
                .iter()
                .map(|c| naming::sanitize_filename(c))
                .collect(),
            created_at: chrono::Local::now().to_rfc3339(),
        };
        // 建夹
        fs::create_dir_all(root.join(".ocard")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join(".ocard/manifests")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join(".ocard/logs")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join(".ocard/thumbs")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join(".ocard/trash")).map_err(|e| e.to_string())?;
        for f in project.folder_template() {
            fs::create_dir_all(root.join(&f)).map_err(|e| e.to_string())?;
        }
        // 工况 B 的精选内含 待修/已修（精选目录带编号，如 `4. 精选`）
        if wf == "B" {
            if let Some(sel_folder) = project.folder_template().iter().find(|f| f.contains("精选")) {
                for sel in naming::select_folders() {
                    let sel_dir = root.join(sel_folder).join(sel);
                    fs::create_dir_all(sel_dir).map_err(|e| e.to_string())?;
                }
            }
        }
        project.save(&root)?;
        Ok(project)
    }

    /// 保存元数据到 `.ocard/project.json`
    pub fn save(&self, root: &Path) -> Result<(), String> {
        let p = root.join(".ocard").join("project.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(p, json).map_err(|e| e.to_string())
    }

    /// 从项目根目录加载
    pub fn load(root: &Path) -> Result<Project, String> {
        let p = root.join(".ocard").join("project.json");
        let s = fs::read_to_string(&p).map_err(|e| format!("读取项目失败 {root:?}: {e}"))?;
        serde_json::from_str(&s).map_err(|e| e.to_string())
    }

    /// 扫描 NAS 根目录下的所有项目
    pub fn list_all(nas_root: &Path) -> Vec<Project> {
        let mut projects = Vec::new();
        if let Ok(rd) = fs::read_dir(nas_root) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Ok(p) = Project::load(&path) {
                        projects.push(p);
                    }
                }
            }
        }
        projects.sort_by(|a, b| b.id.cmp(&a.id));
        projects
    }

    /// 汇总状态：扫描 manifests 与媒体文件统计（PRD §5.2 项目列表页）
    pub fn summary(&self, root: &Path) -> ProjectSummary {
        let mut sum = ProjectSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            workflow: self.workflow.clone(),
            ..Default::default()
        };
        // manifests
        let manifest_dir = root.join(".ocard/manifests");
        if let Ok(rd) = fs::read_dir(&manifest_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(s) = fs::read_to_string(&path) {
                        if let Ok(m) = serde_json::from_str::<crate::manifest::Manifest>(&s) {
                            sum.copied_files += m.files.len() as u64;
                            sum.copied_bytes += m.total_bytes();
                            if m.all_verified() {
                                sum.backup_ok = true;
                            }
                        }
                    }
                }
            }
        }
        // 分类进度（工况 B）：待分类 vs 其他已分类
        if self.workflow == "B" {
            let pending_dir = root.join("1. 待分类");
            sum.unclassified = count_media_files(&pending_dir);
            sum.classified = sum.copied_files.saturating_sub(sum.unclassified);
        }
        // 交付状态：存在交付清单
        let delivery = root.join(".ocard").join("deliveries");
        if let Ok(rd) = fs::read_dir(&delivery) {
            sum.packaged = rd.flatten().next().is_some();
        }
        sum
    }
}

/// 统计目录下媒体文件数量（按扩展名）
pub fn count_media_files(dir: &Path) -> u64 {
    const EXTS: &[&str] = &[
        "jpg", "jpeg", "png", "tif", "tiff", "arw", "cr2", "cr3", "nef", "raf", "dng", "orf",
        "rw2", "mp4", "mov", "mxf", "braw", "r3d", "avi", "mts", "m2ts",
    ];
    let mut count = 0;
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_media_files(&path);
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if EXTS.contains(&ext.to_lowercase().as_str()) {
                    count += 1;
                }
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn create_workflow_a_project() {
        let tmp = setup();
        let p = Project::create(tmp.path(), "20260824", "某活动", "A", &[]).unwrap();
        assert_eq!(p.id, "20260824_某活动");
        let root = tmp.path().join(&p.id);
        for f in ["1. 工程文件", "2. 原始素材", "3. 特别素材", "4. 转码素材", "5. 文字素材", "6. 成片"] {
            assert!(root.join(f).is_dir(), "缺少 {f}");
        }
        assert!(root.join(".ocard/manifests").is_dir());
        assert!(root.join(".ocard/logs").is_dir());
        assert!(root.join(".ocard/trash").is_dir());
        // reload
        let loaded = Project::load(&root).unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn create_workflow_b_project_with_categories() {
        let tmp = setup();
        let p = Project::create(
            tmp.path(),
            "20260824",
            "人像拍摄",
            "B",
            &["人像".to_string(), "风景".to_string()],
        )
        .unwrap();
        let root = tmp.path().join(&p.id);
        assert!(root.join("1. 待分类").is_dir());
        assert!(root.join("2. 人像").is_dir());
        assert!(root.join("3. 风景").is_dir());
        assert!(root.join("4. 精选/待修").is_dir());
        assert!(root.join("4. 精选/已修").is_dir());
        assert!(root.join("5. 其他").is_dir());
    }

    #[test]
    fn create_rejects_duplicate_and_bad_input() {
        let tmp = setup();
        Project::create(tmp.path(), "20260824", "X", "A", &[]).unwrap();
        let dup = Project::create(tmp.path(), "20260824", "X", "A", &[]);
        assert!(dup.is_err());
        assert!(Project::create(tmp.path(), "2026", "Y", "A", &[]).is_err());
        assert!(Project::create(tmp.path(), "20260101", "Z", "C", &[]).is_err());
    }

    #[test]
    fn list_all_projects() {
        let tmp = setup();
        Project::create(tmp.path(), "20260824", "A项目", "A", &[]).unwrap();
        Project::create(tmp.path(), "20260823", "B项目", "B", &[]).unwrap();
        let projects = Project::list_all(tmp.path());
        assert_eq!(projects.len(), 2);
        // 新项目在前
        assert_eq!(projects[0].id, "20260824_A项目");
    }

    #[test]
    fn summary_counts_manifests() {
        let tmp = setup();
        let p = Project::create(tmp.path(), "20260824", "X", "A", &[]).unwrap();
        let root = tmp.path().join(&p.id);
        // 造一个 manifest
        let mut m = crate::manifest::Manifest::new(
            &p.id, "CARD-1", "CAM_A", "张三", "note", "E:", vec!["2. 原始素材".to_string()],
        );
        m.files.push(crate::manifest::ManifestFileEntry {
            path: "a.mp4".to_string(),
            size: 100,
            hash: "h1".to_string(),
            verified: true,
        });
        m.finish();
        fs::write(root.join(".ocard/manifests/task1.json"), m.to_json().unwrap()).unwrap();
        let sum = p.summary(&root);
        assert_eq!(sum.copied_files, 1);
        assert_eq!(sum.copied_bytes, 100);
        assert!(sum.backup_ok);
    }
}
