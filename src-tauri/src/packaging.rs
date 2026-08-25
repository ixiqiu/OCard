//! 交付打包（PRD §5.7，工况 B）
//!
//! 选择已分类文件夹 → 按半天自动分包（依素材拍摄时间），包含精选与其他，
//! 不压缩，生成包文件夹 + 交付清单（每包内容、张数、容量）。
//! 打包完成给出待上传列表，人工上传百度网盘、人工发链接；OCard 记录交付状态。

use chrono::{NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 半天分桶：上午 / 下午
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HalfDay {
    Am,
    Pm,
}

impl HalfDay {
    pub fn from_dt(dt: &NaiveDateTime) -> Self {
        if dt.hour() < 12 {
            HalfDay::Am
        } else {
            HalfDay::Pm
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            HalfDay::Am => "上午",
            HalfDay::Pm => "下午",
        }
    }
}

/// 待打包文件
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageInput {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    /// 拍摄时间（可空，空则按修改时间/未知兜底桶）
    pub datetime: Option<NaiveDateTime>,
}

/// 交付包
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Package {
    pub name: String,
    /// 包内文件相对路径
    pub files: Vec<String>,
    pub count: u64,
    pub total_bytes: u64,
}

/// 交付清单
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeliveryManifest {
    pub project_id: String,
    pub created_at: String,
    pub operator: String,
    pub packages: Vec<Package>,
    pub total_files: u64,
    pub total_bytes: u64,
}

/// 按半天分桶：返回 BTreeMap<(日期, 半天), 文件索引>，桶名如 `20260824_上午`
pub fn bucket_by_half_day(inputs: &[PackageInput]) -> BTreeMap<(String, HalfDay), Vec<usize>> {
    let mut map: BTreeMap<(String, HalfDay), Vec<usize>> = BTreeMap::new();
    for (i, item) in inputs.iter().enumerate() {
        let (date, half) = match &item.datetime {
            Some(dt) => (dt.format("%Y%m%d").to_string(), HalfDay::from_dt(dt)),
            None => ("00000000".to_string(), HalfDay::Am),
        };
        map.entry((date, half)).or_default().push(i);
    }
    map
}

/// 生成交付包列表：按半天分组，包名 `交付_{日期}_{上午|下午}`
pub fn build_packages(inputs: &[PackageInput]) -> Vec<Package> {
    let buckets = bucket_by_half_day(inputs);
    let mut packages = Vec::new();
    for ((date, half), idxs) in buckets {
        let files: Vec<String> = idxs
            .iter()
            .map(|&i| inputs[i].path.to_string_lossy().to_string())
            .collect();
        let count = idxs.len() as u64;
        let total_bytes = idxs.iter().map(|&i| inputs[i].size).sum();
        packages.push(Package {
            name: format!("交付_{date}_{}", half.label()),
            files,
            count,
            total_bytes,
        });
    }
    packages
}

/// 执行打包：把源文件复制（不压缩）到交付包文件夹，生成交付清单 JSON。
/// 返回 (包文件夹根, DeliveryManifest)。
pub fn run_packaging(
    project_root: &Path,
    inputs: &[PackageInput],
    operator: &str,
) -> Result<(PathBuf, DeliveryManifest), String> {
    if inputs.is_empty() {
        return Err("没有可打包的文件".to_string());
    }
    let packages = build_packages(inputs);
    let delivery_root = project_root.join("交付包");
    // 清空旧交付包目录，重新生成（打包是确定性操作）
    if delivery_root.exists() {
        fs::remove_dir_all(&delivery_root).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&delivery_root).map_err(|e| e.to_string())?;

    for pkg in &packages {
        let pkg_dir = delivery_root.join(&pkg.name);
        fs::create_dir_all(&pkg_dir).map_err(|e| e.to_string())?;
        for f in &pkg.files {
            let src = PathBuf::from(f);
            let name = src
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let dest = pkg_dir.join(&name);
            // 同名冲突：加序号
            let dest = if dest.exists() {
                let (base, ext) = match name.rfind('.') {
                    Some(i) if i > 0 => (&name[..i], Some(&name[i + 1..])),
                    _ => (name.as_str(), None),
                };
                let mut n = 1u32;
                let make_candidate = |n: u32| match ext {
                    Some(e) => pkg_dir.join(format!("{base}_{n}.{e}")),
                    None => pkg_dir.join(format!("{base}_{n}")),
                };
                let mut candidate = make_candidate(n);
                while candidate.exists() {
                    n += 1;
                    candidate = make_candidate(n);
                }
                candidate
            } else {
                dest
            };
            fs::copy(&src, &dest).map_err(|e| format!("复制失败 {src:?}: {e}"))?;
        }
    }

    let manifest = DeliveryManifest {
        project_id: project_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        created_at: chrono::Local::now().to_rfc3339(),
        operator: operator.to_string(),
        packages,
        total_files: inputs.len() as u64,
        total_bytes: inputs.iter().map(|i| i.size).sum(),
    };

    // 交付清单写到 .ocard/deliveries/latest.json（供项目状态汇总）
    let meta_dir = project_root.join(".ocard").join("deliveries");
    fs::create_dir_all(&meta_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(meta_dir.join("latest.json"), json).map_err(|e| e.to_string())?;

    Ok((delivery_root, manifest))
}

/// 把待上传列表写成文本（人工上传百度网盘后勾选，PRD §5.7）
pub fn upload_list_text(manifest: &DeliveryManifest) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "交付清单 - {}（{}）\n共 {} 个包，{} 个文件，{} 字节\n\n",
        manifest.project_id,
        manifest.created_at,
        manifest.packages.len(),
        manifest.total_files,
        manifest.total_bytes
    ));
    for pkg in &manifest.packages {
        s.push_str(&format!(
            "📦 {}（{} 张，{} 字节）\n",
            pkg.name, pkg.count, pkg.total_bytes
        ));
        for f in &pkg.files {
            s.push_str(&format!("  - {}\n", f));
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(path: &str, size: u64, dt: Option<&str>) -> PackageInput {
        PackageInput {
            path: PathBuf::from(path),
            name: PathBuf::from(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            size,
            datetime: dt.and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()),
        }
    }

    fn ts(s: &str) -> Option<NaiveDateTime> {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
    }

    #[test]
    fn half_day_from_time() {
        let am = ts("2026-08-24 09:00:00").unwrap();
        let pm = ts("2026-08-24 13:00:00").unwrap();
        assert_eq!(HalfDay::from_dt(&am), HalfDay::Am);
        assert_eq!(HalfDay::from_dt(&pm), HalfDay::Pm);
        assert_eq!(HalfDay::Am.label(), "上午");
        assert_eq!(HalfDay::Pm.label(), "下午");
    }

    #[test]
    fn bucket_splits_by_date_and_half() {
        let inputs = vec![
            input("/x/1.jpg", 10, Some("2026-08-24 09:00:00")),
            input("/x/2.jpg", 20, Some("2026-08-24 10:00:00")),
            input("/x/3.jpg", 30, Some("2026-08-24 14:00:00")),
            input("/x/4.jpg", 40, Some("2026-08-25 08:00:00")),
            input("/x/5.jpg", 50, None),
        ];
        let buckets = bucket_by_half_day(&inputs);
        // (20260824,Am) (20260824,Pm) (20260825,Am) (00000000,Am) 共 4 桶
        assert_eq!(buckets.len(), 4);
        // 20260824 上午：2 个
        assert_eq!(buckets[&("20260824".to_string(), HalfDay::Am)].len(), 2);
        // 20260824 下午：1 个
        assert_eq!(buckets[&("20260824".to_string(), HalfDay::Pm)].len(), 1);
        // 20260825 上午：1 个
        assert_eq!(buckets[&("20260825".to_string(), HalfDay::Am)].len(), 1);
        // 无时间戳进兜底桶
        assert_eq!(buckets[&("00000000".to_string(), HalfDay::Am)].len(), 1);
    }

    #[test]
    fn build_packages_names_and_sizes() {
        let inputs = vec![
            input("/x/a.jpg", 100, Some("2026-08-24 09:00:00")),
            input("/x/b.jpg", 200, Some("2026-08-24 11:00:00")),
            input("/x/c.jpg", 400, Some("2026-08-24 14:00:00")),
        ];
        let packages = build_packages(&inputs);
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "交付_20260824_上午");
        assert_eq!(packages[0].count, 2);
        assert_eq!(packages[0].total_bytes, 300);
        assert_eq!(packages[1].name, "交付_20260824_下午");
        assert_eq!(packages[1].count, 1);
    }

    #[test]
    fn run_packaging_copies_and_writes_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("20260824_活动");
        let src = proj.join("精选");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("s1.jpg"), b"data-1").unwrap();
        fs::write(src.join("s2.jpg"), b"data-22").unwrap();

        let inputs = vec![
            input(src.join("s1.jpg").to_str().unwrap(), 6, Some("2026-08-24 09:00:00")),
            input(src.join("s2.jpg").to_str().unwrap(), 7, Some("2026-08-24 14:00:00")),
        ];
        let (root, manifest) = run_packaging(&proj, &inputs, "张三").unwrap();
        assert!(root.join("交付_20260824_上午/s1.jpg").exists());
        assert!(root.join("交付_20260824_下午/s2.jpg").exists());
        assert_eq!(manifest.packages.len(), 2);
        assert_eq!(manifest.total_files, 2);
        assert_eq!(manifest.operator, "张三");
        // 清单落盘
        assert!(proj.join(".ocard/deliveries/latest.json").exists());
        // 上传列表文本
        let text = upload_list_text(&manifest);
        assert!(text.contains("交付_20260824_上午"));
        assert!(text.contains("s1.jpg"));
    }

    #[test]
    fn run_packaging_requires_inputs() {
        let dir = tempfile::tempdir().unwrap();
        assert!(run_packaging(dir.path(), &[], "张三").is_err());
    }

    #[test]
    fn run_packaging_handles_name_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("P");
        let src = proj.join("精选");
        fs::create_dir_all(src.join("sub1")).unwrap();
        fs::write(src.join("x.jpg"), b"1").unwrap();
        fs::write(src.join("sub1/x.jpg"), b"2").unwrap();

        let inputs = vec![
            input(src.join("x.jpg").to_str().unwrap(), 1, Some("2026-08-24 09:00:00")),
            input(src.join("sub1/x.jpg").to_str().unwrap(), 1, Some("2026-08-24 09:30:00")),
        ];
        let (root, _m) = run_packaging(&proj, &inputs, "张三").unwrap();
        let pkg = root.join("交付_20260824_上午");
        let entries: Vec<String> = fs::read_dir(&pkg)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries.len(), 2, "同名文件应去重：{entries:?}");
        assert!(entries.contains(&"x.jpg".to_string()));
        assert!(entries.iter().any(|e| e.starts_with("x_1.")));
    }
}
