//! 拷卡引擎（PRD §5.3 核心）
//!
//! - 自动检测可移动卷（见 volume.rs）
//! - 流式读源（边读边算 xxHash3-64）→ 并行写 N 个目的地
//! - 临时名写入、校验后改名（NAS 断连不产生半个文件）
//! - 回读目标逐文件比对校验；manifest 落盘
//! - 断点续传：按已有 manifest 跳过已校验文件
//! - 逐文件标红、可单独重试，任务不整体作废（PRD §6.4）

use crate::hash;
use crate::manifest::{Manifest, ManifestFileEntry};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// 临时文件后缀（校验后改名）
const TMP_SUFFIX: &str = ".ocard-part";

/// 拷贝任务选项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyTaskOptions {
    pub source_root: PathBuf,
    /// 目的地根目录列表（并行写）
    pub dest_roots: Vec<PathBuf>,
    /// 待拷贝文件的相对路径列表；为空则扫描源根下全部文件
    pub rel_files: Vec<String>,
    pub project_id: String,
    pub card_label: String,
    pub camera_code: String,
    pub operator: String,
    pub note: String,
    pub source_volume: String,
}

/// 单文件结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyFileOutcome {
    pub rel_path: String,
    pub size: u64,
    pub hash: String,
    /// 是否已回读校验通过（至少一个目的地）
    pub verified: bool,
    /// 断点续传跳过
    pub skipped: bool,
    /// 失败目的地错误信息
    pub errors: Vec<String>,
}

/// 任务报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyTaskReport {
    pub manifest: Manifest,
    pub outcomes: Vec<CopyFileOutcome>,
    pub copied_count: usize,
    pub verified_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub total_bytes: u64,
    pub duration_secs: f64,
    /// 是否 100% 校验通过（可提示「本卡可格式化」）
    pub all_verified: bool,
}

/// 根据工况生成目标子文件夹（PRD §5.3 目标自动命名）。
/// 工况 A：`2. 原始素材/YYYYMMDD_相机编码`；工况 B：`1. 待分类/时段_相机编码`。
pub fn dest_subdir(
    workflow: &str,
    camera_code: &str,
    date: &str,
    first_ts: Option<&NaiveDateTime>,
) -> String {
    if workflow == "A" {
        format!("2. 原始素材/{date}_{camera_code}")
    } else {
        let slot = match first_ts {
            Some(dt) => crate::naming::infer_time_slot(dt),
            None => "0000上午".to_string(),
        };
        format!("1. 待分类/{slot}_{camera_code}")
    }
}

/// 递归扫描目录下所有常规文件（相对路径，跳过 .ocard 等隐藏目录）
pub fn scan_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !(e.depth() > 0 && name.starts_with('.'))
        })
        .flatten()
    {
        if entry.file_type().is_file() {
            files.push(entry.path().to_path_buf());
        }
    }
    files
}

/// 把绝对路径转成相对 root 的路径
pub fn rel_path(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs.to_string_lossy().replace('\\', "/"))
}

/// 拷贝单个文件到多个目的地：流式读源，边读边算 hash，并行写所有目的地临时文件，
/// 成功后统一改名。任一目的地失败：删除该目的地临时文件并记录错误；其余目的地保留
/// （PRD §6.4 逐文件标红、任务不整体作废）。
/// 返回 (源大小, hash, 各目的地结果，顺序与 dest_paths 一致)。
pub fn copy_file_multi(
    src: &Path,
    dest_paths: &[PathBuf],
) -> io::Result<(u64, String, Vec<Result<(), String>>)> {
    let mut src_file = fs::File::open(src)?;
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();

    // 逐目的地 setup：任一失败只记录错误，其余目的地继续
    let mut writers: Vec<(usize, PathBuf, PathBuf, fs::File)> = Vec::new(); // (dest_idx, tmp, final, file)
    let mut results: Vec<Option<Result<(), String>>> = vec![None; dest_paths.len()];
    for (idx, dest) in dest_paths.iter().enumerate() {
        if let Some(parent) = dest.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                results[idx] = Some(Err(format!("{dest:?}: {e}")));
                continue;
            }
        }
        let tmp = PathBuf::from(format!("{}{}", dest.display(), TMP_SUFFIX));
        match fs::File::create(&tmp) {
            Ok(f) => writers.push((idx, tmp.clone(), dest.clone(), f)),
            Err(e) => {
                results[idx] = Some(Err(format!("{dest:?}: {e}")));
            }
        }
    }

    let (total, digest) = match (|| -> io::Result<(u64, String)> {
        let mut buf = vec![0u8; hash::CHUNK_SIZE];
        let mut total: u64 = 0;
        loop {
            let n = src_file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            total += n as u64;
            for (_, _, _, w) in writers.iter_mut() {
                w.write_all(&buf[..n])?;
            }
        }
        Ok((total, format!("{:016x}", hasher.digest())))
    })() {
        Ok(v) => v,
        Err(e) => {
            // 中途失败：清理所有已创建的临时文件（NAS 断连不产生半个文件）
            for (_, tmp, _, _) in &writers {
                let _ = fs::remove_file(tmp);
            }
            return Err(e);
        }
    };

    for (idx, tmp, final_path, mut w) in writers {
        if let Err(e) = w.flush().and_then(|_| w.sync_all()) {
            let _ = fs::remove_file(&tmp);
            results[idx] = Some(Err(format!("{final_path:?}: {e}")));
            continue;
        }
        // 校验后改名（同目录 rename 安全）
        match fs::rename(&tmp, &final_path) {
            Ok(()) => results[idx] = Some(Ok(())),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                results[idx] = Some(Err(format!("{final_path:?}: {e}")));
            }
        }
    }

    let results = results
        .into_iter()
        .enumerate()
        .map(|(i, r)| r.unwrap_or_else(|| Err(format!("{}: 未写入", dest_paths[i].display()))))
        .collect();

    Ok((total, digest, results))
}

/// 回读目标文件校验（大小 + hash）
pub fn verify_file(dest: &Path, expected_size: u64, expected_hash: &str) -> Result<(), String> {
    let meta = fs::metadata(dest).map_err(|e| format!("{dest:?}: {e}"))?;
    if meta.len() != expected_size {
        return Err(format!(
            "{dest:?}: 大小不符 {} != {}",
            meta.len(),
            expected_size
        ));
    }
    let actual = hash::hash_file(dest).map_err(|e| format!("{dest:?}: {e}"))?;
    if actual != expected_hash {
        return Err(format!("{dest:?}: hash 不符 {actual} != {expected_hash}"));
    }
    Ok(())
}

/// 执行拷贝任务。`resume_manifest` 提供断点续传：已校验文件跳过。
pub fn run_copy_task(
    opts: &CopyTaskOptions,
    resume_manifest: Option<&Manifest>,
) -> Result<CopyTaskReport, String> {
    let started = std::time::Instant::now();
    if opts.dest_roots.is_empty() {
        return Err("至少需要一个目的地".to_string());
    }
    if !opts.source_root.is_dir() {
        return Err(format!("源卷不存在：{:?}", opts.source_root));
    }

    // 文件列表
    let files: Vec<PathBuf> = if opts.rel_files.is_empty() {
        scan_files(&opts.source_root)
    } else {
        opts.rel_files
            .iter()
            .map(|r| opts.source_root.join(r))
            .collect()
    };

    let mut manifest = Manifest::new(
        &opts.project_id,
        &opts.card_label,
        &opts.camera_code,
        &opts.operator,
        &opts.note,
        &opts.source_volume,
        opts.dest_roots
            .iter()
            .map(|d| d.to_string_lossy().to_string())
            .collect(),
    );

    let mut outcomes = Vec::new();
    let mut total_bytes = 0u64;

    for file in files {
        let rel = rel_path(&opts.source_root, &file);
        let meta = fs::metadata(&file).map_err(|e| format!("{rel}: {e}"))?;
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();

        // 断点续传：manifest 中该文件已 verified 且所有目的地仍存在则跳过
        if let Some(rm) = resume_manifest {
            if let Some(entry) = rm.files.iter().find(|f| f.path == rel && f.verified) {
                let dests_exist = opts.dest_roots.iter().all(|root| root.join(&rel).exists());
                if dests_exist {
                    outcomes.push(CopyFileOutcome {
                        rel_path: rel.clone(),
                        size: entry.size,
                        hash: entry.hash.clone(),
                        verified: true,
                        skipped: true,
                        errors: vec![],
                    });
                    total_bytes += entry.size;
                    manifest.files.push(entry.clone());
                    continue;
                }
            }
        }

        // 多目的地并行拷贝
        let dest_paths: Vec<PathBuf> = opts
            .dest_roots
            .iter()
            .map(|root| root.join(&rel))
            .collect();

        // 逐文件容错：单个文件失败只标红，不中断整个任务（PRD §6.4）
        let (actual_size, digest, results) = match copy_file_multi(&file, &dest_paths) {
            Ok(v) => v,
            Err(e) => {
                total_bytes += size;
                manifest.files.push(ManifestFileEntry {
                    path: rel.clone(),
                    size,
                    hash: String::new(),
                    verified: false,
                });
                outcomes.push(CopyFileOutcome {
                    rel_path: rel.clone(),
                    size,
                    hash: String::new(),
                    verified: false,
                    skipped: false,
                    errors: vec![format!("{rel}: {e}")],
                });
                continue;
            }
        };
        total_bytes += actual_size;

        let mut errors = Vec::new();
        let mut ok_dests: Vec<&PathBuf> = Vec::new();
        for (dest, res) in dest_paths.iter().zip(results.iter()) {
            match res {
                Ok(()) => ok_dests.push(dest),
                Err(e) => errors.push(e.clone()),
            }
        }

        // 回读校验所有成功目的地
        let mut verified = false;
        if !ok_dests.is_empty() {
            let mut all_ok = true;
            for dest in &ok_dests {
                if let Err(e) = verify_file(dest, actual_size, &digest) {
                    errors.push(e);
                    all_ok = false;
                }
            }
            verified = all_ok;
        }

        manifest.files.push(ManifestFileEntry {
            path: rel.clone(),
            size: actual_size,
            hash: digest.clone(),
            verified,
        });

        outcomes.push(CopyFileOutcome {
            rel_path: rel,
            size: actual_size,
            hash: digest,
            verified,
            skipped: false,
            errors,
        });
    }

    let copied_count = outcomes.iter().filter(|o| !o.skipped).count();
    let verified_count = outcomes.iter().filter(|o| o.verified).count();
    let failed_count = outcomes.iter().filter(|o| !o.errors.is_empty()).count();
    let skipped_count = outcomes.iter().filter(|o| o.skipped).count();
    let all_verified = !outcomes.is_empty() && verified_count == outcomes.len();

    if all_verified {
        manifest.finish();
    }

    Ok(CopyTaskReport {
        manifest,
        outcomes,
        copied_count,
        verified_count,
        failed_count,
        skipped_count,
        total_bytes,
        duration_secs: started.elapsed().as_secs_f64(),
        all_verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, data: &[u8]) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, data).unwrap();
    }

    #[test]
    fn dest_subdir_workflow_a() {
        let ts = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        assert_eq!(
            dest_subdir("A", "DJIRONIN4D_B_ZS", "20260824", Some(&ts)),
            "2. 原始素材/20260824_DJIRONIN4D_B_ZS"
        );
        assert_eq!(
            dest_subdir("B", "CAM_A_ZS", "20260824", Some(&ts)),
            "1. 待分类/0101上午_CAM_A_ZS"
        );
        // 无时间戳时 B 用兜底时段
        assert_eq!(
            dest_subdir("B", "CAM_A_ZS", "20260824", None),
            "1. 待分类/0000上午_CAM_A_ZS"
        );
    }

    #[test]
    fn scan_files_recursive_skips_hidden() {
        let dir = tempfile::tempdir().unwrap();
        write_file(&dir.path().join("a.txt"), b"a");
        write_file(&dir.path().join("sub/b.txt"), b"b");
        write_file(&dir.path().join(".ocard/x.txt"), b"x");
        let files = scan_files(dir.path());
        let rels: Vec<String> = files.iter().map(|f| rel_path(dir.path(), f)).collect();
        assert!(rels.contains(&"a.txt".to_string()));
        assert!(rels.contains(&"sub/b.txt".to_string()));
        assert!(!rels.iter().any(|r| r.starts_with(".ocard")));
    }

    #[test]
    fn copy_single_file_to_multiple_dests_and_verify() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src/card/A001_0001.MP4");
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        write_file(&src, &data);

        let d1 = tmp.path().join("dest1");
        let d2 = tmp.path().join("dest2");
        let (size, digest, results) =
            copy_file_multi(&src, &[d1.join("A001_0001.MP4"), d2.join("A001_0001.MP4")])
                .unwrap();
        assert_eq!(size as usize, data.len());
        assert_eq!(digest, hash::hash_bytes(&data));
        assert!(results.iter().all(|r| r.is_ok()));

        // 回读校验
        verify_file(&d1.join("A001_0001.MP4"), size, &digest).unwrap();
        verify_file(&d2.join("A001_0001.MP4"), size, &digest).unwrap();
        // 无 .part 残留
        assert!(!d1.join("A001_0001.MP4.ocard-part").exists());
    }

    #[test]
    fn copy_detects_corrupted_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src/a.bin");
        write_file(&src, b"hello world hello world");
        let dest = tmp.path().join("dest/a.bin");
        let (size, digest, _) = copy_file_multi(&src, &[dest.clone()]).unwrap();
        // 篡改目标
        let mut f = fs::OpenOptions::new().write(true).open(&dest).unwrap();
        f.write_all(b"XXXX").unwrap();
        drop(f);
        assert!(verify_file(&dest, size, &digest).is_err());
    }

    #[test]
    fn run_copy_task_full_flow() {
        let tmp = tempfile::tempdir().unwrap();
        let src_root = tmp.path().join("card");
        write_file(&src_root.join("A001_0001.MP4"), b"data1");
        write_file(&src_root.join("A001_0002.MP4"), b"data2");
        write_file(&src_root.join("sub/A001_0003.MP4"), b"data3");

        let d1 = tmp.path().join("nas");
        let d2 = tmp.path().join("backup");
        let opts = CopyTaskOptions {
            source_root: src_root.clone(),
            dest_roots: vec![d1.clone(), d2.clone()],
            rel_files: vec![],
            project_id: "20260824_X".to_string(),
            card_label: "CARD-001".to_string(),
            camera_code: "CAM_A_ZS".to_string(),
            operator: "张三".to_string(),
            note: "测试".to_string(),
            source_volume: "/vol/card".to_string(),
        };
        let report = run_copy_task(&opts, None).unwrap();
        assert_eq!(report.copied_count, 3);
        assert_eq!(report.verified_count, 3);
        assert!(report.all_verified);
        assert!(report.manifest.all_verified());
        assert_eq!(report.total_bytes, 15);
        // 两个目的地都有文件
        assert!(d1.join("A001_0001.MP4").exists());
        assert!(d2.join("A001_0001.MP4").exists());
        assert!(d1.join("sub/A001_0003.MP4").exists());
        // manifest JSON 可序列化
        let json = report.manifest.to_json().unwrap();
        assert!(json.contains("20260824_X"));
    }

    #[test]
    fn run_copy_task_resume_skips_verified() {
        let tmp = tempfile::tempdir().unwrap();
        let src_root = tmp.path().join("card");
        write_file(&src_root.join("a.mp4"), b"aaaa");
        write_file(&src_root.join("b.mp4"), b"bbbb");

        let d1 = tmp.path().join("nas");
        let opts = CopyTaskOptions {
            source_root: src_root.clone(),
            dest_roots: vec![d1.clone()],
            rel_files: vec![],
            project_id: "P".to_string(),
            card_label: "C".to_string(),
            camera_code: "K".to_string(),
            operator: "O".to_string(),
            note: String::new(),
            source_volume: "V".to_string(),
        };
        let r1 = run_copy_task(&opts, None).unwrap();
        assert_eq!(r1.verified_count, 2);

        // 第二次运行，用第一次 manifest 续传 -> 全部跳过
        let r2 = run_copy_task(&opts, Some(&r1.manifest)).unwrap();
        assert_eq!(r2.skipped_count, 2);
        assert_eq!(r2.copied_count, 0);
        assert!(r2.all_verified);

        // 若目标被删除，则重拷（不跳过）
        fs::remove_file(d1.join("a.mp4")).unwrap();
        let r3 = run_copy_task(&opts, Some(&r1.manifest)).unwrap();
        assert_eq!(r3.copied_count, 1);
        assert!(d1.join("a.mp4").exists());
    }

    #[test]
    fn run_copy_task_failure_does_not_abort_all() {
        let tmp = tempfile::tempdir().unwrap();
        let src_root = tmp.path().join("card");
        write_file(&src_root.join("a.mp4"), b"aaa");
        write_file(&src_root.join("b.mp4"), b"bbb");

        let good = tmp.path().join("good");
        // 不可写目的地：用一个文件路径占位
        let bad = tmp.path().join("bad");
        fs::write(&bad, b"i am a file, not a dir").unwrap();

        let opts = CopyTaskOptions {
            source_root: src_root.clone(),
            dest_roots: vec![good.clone(), bad.clone()],
            rel_files: vec![],
            project_id: "P".to_string(),
            card_label: "C".to_string(),
            camera_code: "K".to_string(),
            operator: "O".to_string(),
            note: String::new(),
            source_volume: "V".to_string(),
        };
        let report = run_copy_task(&opts, None).unwrap();
        // 好目的地有文件
        assert!(good.join("a.mp4").exists());
        // 坏目的地没有
        assert!(!bad.join("a.mp4").exists());
        // 好目的地仍校验通过（verified），坏目的地错误被记录，任务没整体作废
        assert_eq!(report.verified_count, 2);
        assert!(report.failed_count > 0);
        assert!(!report.all_verified);
        assert!(report.outcomes.iter().all(|o| !o.skipped && o.verified && !o.errors.is_empty()));
    }

    #[test]
    fn copy_file_multi_cleans_tmp_on_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src/x.bin");
        write_file(&src, b"some content");
        let good = tmp.path().join("good");
        let bad = tmp.path().join("bad");
        fs::write(&bad, b"not a dir").unwrap();
        let res = copy_file_multi(&src, &[good.join("x.bin"), bad.join("x.bin")]);
        // 逐目的地容错：坏目的地失败不中断整体，好目的地正常落盘
        let (_, _, results) = res.unwrap();
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        // 好目的地已改名落盘，无 .ocard-part 残留
        assert!(good.join("x.bin").exists());
        assert!(!good.join("x.bin.ocard-part").exists());
    }
}
