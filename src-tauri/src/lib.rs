//! OCard —— Tauri 2 桌面应用核心
//!
//! 分层：UI 层（Web/React）↔ Rust 核心（tauri commands + 后台任务）。
//! 模块：copy_engine / hash / project_store / media_indexer / culling /
//!       transcode / packaging / audit_log / volume / device_store / naming / config

mod audit_log;
mod config;
mod copy_engine;
mod culling;
mod device_store;
mod hash;
mod manifest;
mod media_indexer;
mod naming;
mod packaging;
mod project_store;
mod transcode;
mod volume;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use config::AppConfig;

pub struct AppState {
    pub config: Mutex<AppConfig>,
}

/// 解析项目根目录：`{nas_root}/{project_id}`
fn project_root(state: &tauri::State<AppState>, project_id: &str) -> Result<PathBuf, String> {
    let cfg = state.config.lock().unwrap();
    let nas = Path::new(&cfg.nas_root);
    if nas.as_os_str().is_empty() {
        return Err("未配置 NAS 根路径，请先在设置中登记".to_string());
    }
    let root = nas.join(project_id);
    if !root.is_dir() {
        return Err(format!("项目不存在：{project_id}（{root:?}）"));
    }
    Ok(root)
}

// ---------------------------------------------------------------------------
// 配置
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_config(state: tauri::State<'_, AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn save_config(
    state: tauri::State<'_, AppState>,
    nas_root: String,
    operator: String,
) -> Result<AppConfig, String> {
    let mut cfg = state.config.lock().unwrap();
    if !nas_root.trim().is_empty() {
        cfg.nas_root = nas_root.trim().to_string();
    }
    if !operator.trim().is_empty() {
        cfg.operator = operator.trim().to_string();
    }
    let saved = cfg.clone();
    saved.save().map_err(|e| e.to_string())?;
    Ok(saved)
}

// ---------------------------------------------------------------------------
// 项目管理
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_projects(state: tauri::State<'_, AppState>) -> Vec<project_store::ProjectSummary> {
    let cfg = state.config.lock().unwrap();
    let nas = Path::new(&cfg.nas_root);
    if nas.as_os_str().is_empty() || !nas.is_dir() {
        return Vec::new();
    }
    let projects = project_store::Project::list_all(nas);
    projects
        .iter()
        .map(|p| p.summary(&nas.join(&p.id)))
        .collect()
}

#[tauri::command]
fn create_project(
    state: tauri::State<'_, AppState>,
    date: String,
    name: String,
    workflow: String,
    categories: Vec<String>,
) -> Result<project_store::Project, String> {
    let cfg = state.config.lock().unwrap();
    let nas = Path::new(&cfg.nas_root);
    if nas.as_os_str().is_empty() {
        return Err("未配置 NAS 根路径".to_string());
    }
    let project = project_store::Project::create(nas, &date, &name, &workflow, &categories)?;
    // 审计：建项目
    let audit = audit_log::AuditLog::new(
        &nas.join(&project.id).join(".ocard/logs"),
        &cfg.machine_id,
    );
    let _ = audit.log(&cfg.operator, "create_project", &project.id, None);
    Ok(project)
}

#[tauri::command]
fn get_project_summary(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> Result<project_store::ProjectSummary, String> {
    let root = project_root(&state, &project_id)?;
    let project = project_store::Project::load(&root)?;
    Ok(project.summary(&root))
}

#[tauri::command]
fn get_project(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> Result<project_store::Project, String> {
    let root = project_root(&state, &project_id)?;
    project_store::Project::load(&root)
}

// ---------------------------------------------------------------------------
// 设备登记
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_devices(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> Result<device_store::DeviceRegistry, String> {
    let root = project_root(&state, &project_id)?;
    Ok(device_store::DeviceRegistry::load(&root.join(".ocard")))
}

#[tauri::command]
fn add_camera(
    state: tauri::State<'_, AppState>,
    project_id: String,
    model: String,
    position: String,
    operator: String,
    note: Option<String>,
) -> Result<device_store::Camera, String> {
    let root = project_root(&state, &project_id)?;
    let mut reg = device_store::DeviceRegistry::load(&root.join(".ocard"));
    let cam = reg.add_camera(&model, &position, &operator, note)?;
    reg.save(&root.join(".ocard")).map_err(|e| e.to_string())?;
    let cfg = state.config.lock().unwrap();
    let audit = audit_log::AuditLog::new(&root.join(".ocard/logs"), &cfg.machine_id);
    let _ = audit.log(&cfg.operator, "register_camera", &cam.code, None);
    Ok(cam)
}

#[tauri::command]
fn add_card(
    state: tauri::State<'_, AppState>,
    project_id: String,
    label: String,
    camera_id: String,
    note: Option<String>,
) -> Result<device_store::Card, String> {
    let root = project_root(&state, &project_id)?;
    let mut reg = device_store::DeviceRegistry::load(&root.join(".ocard"));
    let card = reg.add_card(&label, &camera_id, note)?;
    reg.save(&root.join(".ocard")).map_err(|e| e.to_string())?;
    let cfg = state.config.lock().unwrap();
    let audit = audit_log::AuditLog::new(&root.join(".ocard/logs"), &cfg.machine_id);
    let _ = audit.log(&cfg.operator, "register_card", &card.label, None);
    Ok(card)
}

// ---------------------------------------------------------------------------
// 可移动卷
// ---------------------------------------------------------------------------

#[tauri::command]
fn scan_volumes() -> Vec<volume::Volume> {
    volume::scan_removable_volumes()
}

// ---------------------------------------------------------------------------
// 拷卡引擎
// ---------------------------------------------------------------------------

/// 拷卡：前端传入源卷、目的地（相对项目根的列表）、相机编码等。
/// 后端自动拼上项目根路径，执行拷贝 + 校验 + manifest 落盘 + 审计日志。
#[tauri::command]
fn run_copy(
    state: tauri::State<'_, AppState>,
    project_id: String,
    source_volume: String,
    dest_rel_paths: Vec<String>,
    card_label: String,
    camera_code: String,
    note: String,
    resume: bool,
) -> Result<copy_engine::CopyTaskReport, String> {
    let root = project_root(&state, &project_id)?;
    let cfg = state.config.lock().unwrap();
    let source_root = PathBuf::from(&source_volume);
    if !source_root.is_dir() {
        return Err(format!("源卷不存在：{source_volume}"));
    }
    if dest_rel_paths.is_empty() {
        return Err("至少需要一个目的地".to_string());
    }
    let dest_roots: Vec<PathBuf> = dest_rel_paths
        .iter()
        .map(|rel| root.join(rel))
        .collect();

    // 断点续传：加载最近一个未全部完成的 manifest
    let resume_manifest = if resume {
        load_latest_manifest(&root)
    } else {
        None
    };

    let opts = copy_engine::CopyTaskOptions {
        source_root,
        dest_roots,
        rel_files: vec![],
        project_id: project_id.clone(),
        card_label: card_label.clone(),
        camera_code: camera_code.clone(),
        operator: cfg.operator.clone(),
        note: note.clone(),
        source_volume: source_volume.clone(),
    };

    let report = copy_engine::run_copy_task(&opts, resume_manifest.as_ref())?;

    // manifest 落盘
    let manifest_dir = root.join(".ocard/manifests");
    std::fs::create_dir_all(&manifest_dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let mpath = manifest_dir.join(format!("copy_{}_{}.json", stamp, sanitize_slug(&card_label)));
    std::fs::write(&mpath, report.manifest.to_json().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    // 审计
    let audit = audit_log::AuditLog::new(&root.join(".ocard/logs"), &cfg.machine_id);
    let _ = audit.log(
        &cfg.operator,
        "copy",
        &card_label,
        Some(format!(
            "源={source_volume} 相机={camera_code} 文件={} 校验={} 失败={}",
            report.copied_count, report.verified_count, report.failed_count
        )),
    );
    if report.all_verified {
        let _ = audit.log(&cfg.operator, "verify", &card_label, Some("本卡可格式化".to_string()));
    }
    Ok(report)
}

fn load_latest_manifest(project_root: &Path) -> Option<manifest::Manifest> {
    let dir = project_root.join(".ocard/manifests");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    let latest = files.last()?;
    let s = std::fs::read_to_string(latest).ok()?;
    manifest::Manifest::from_json(&s).ok()
}

fn sanitize_slug(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect()
}

#[tauri::command]
fn list_manifests(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> Result<Vec<manifest::Manifest>, String> {
    let root = project_root(&state, &project_id)?;
    let dir = root.join(".ocard/manifests");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if let Ok(m) = manifest::Manifest::from_json(&s) {
                        out.push(m);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(out)
}

// ---------------------------------------------------------------------------
// 媒体索引 / 分类工作台
// ---------------------------------------------------------------------------

#[tauri::command]
fn scan_media_folder(
    state: tauri::State<'_, AppState>,
    project_id: String,
    folder_rel: String,
) -> Result<Vec<media_indexer::MediaItem>, String> {
    let root = project_root(&state, &project_id)?;
    let dir = root.join(&folder_rel);
    if !dir.is_dir() {
        return Err(format!("文件夹不存在：{folder_rel}"));
    }
    Ok(media_indexer::scan_media(&dir))
}

#[tauri::command]
fn make_thumbnail(
    state: tauri::State<'_, AppState>,
    project_id: String,
    file_path: String,
) -> Result<String, String> {
    let root = project_root(&state, &project_id)?;
    let src = PathBuf::from(&file_path);
    if !src.is_absolute() {
        return Err("需要绝对路径".to_string());
    }
    let out = media_indexer::make_thumbnail(&src, &root.join(".ocard"), 320)?;
    Ok(out.to_string_lossy().to_string())
}

/// 分析文件夹下所有图像（清晰度/曝光；人脸闭眼由后端能力决定）
#[tauri::command]
fn analyze_folder(
    state: tauri::State<'_, AppState>,
    project_id: String,
    folder_rel: String,
) -> Result<Vec<culling::AnalyzeResult>, String> {
    let root = project_root(&state, &project_id)?;
    let dir = root.join(&folder_rel);
    if !dir.is_dir() {
        return Err(format!("文件夹不存在：{folder_rel}"));
    }
    let items = media_indexer::scan_media(&dir);
    let backend = culling::create_backend();
    let mut results = Vec::new();
    for item in items {
        let p = PathBuf::from(&item.path);
        let mut r = culling::analyze_file(&p, Some(item.datetime_original.clone()));
        if !item.is_video {
            r.eyes_closed = backend.detect_face_and_eyes(&p);
        }
        results.push(r);
    }
    Ok(results)
}

/// 连拍聚类（供前端折叠显示）
#[tauri::command]
fn burst_groups(
    state: tauri::State<'_, AppState>,
    project_id: String,
    folder_rel: String,
) -> Result<Vec<culling::BurstGroup>, String> {
    let root = project_root(&state, &project_id)?;
    let dir = root.join(&folder_rel);
    let items = media_indexer::scan_media(&dir);
    let results: Vec<culling::AnalyzeResult> = items
        .iter()
        .map(|it| culling::analyze_file(&PathBuf::from(&it.path), Some(it.datetime_original.clone())))
        .collect();
    Ok(culling::build_burst_groups(&results, 5, 2))
}

/// 把文件移到项目回收站（`.ocard/trash`，两段式删除的第一步标记/第二步执行）
#[tauri::command]
fn move_to_trash(
    state: tauri::State<'_, AppState>,
    project_id: String,
    file_path: String,
) -> Result<bool, String> {
    let root = project_root(&state, &project_id)?;
    let src = PathBuf::from(&file_path);
    if !src.is_absolute() {
        return Err("需要绝对路径".to_string());
    }
    let trash = root.join(".ocard/trash");
    std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let dest = trash.join(name);
    std::fs::rename(&src, &dest).map_err(|e| e.to_string())?;
    let cfg = state.config.lock().unwrap();
    let audit = audit_log::AuditLog::new(&root.join(".ocard/logs"), &cfg.machine_id);
    let _ = audit.log(&cfg.operator, "trash", &file_path, None);
    Ok(true)
}

/// 从回收站恢复（可恢复，PRD §5.4）
#[tauri::command]
fn restore_from_trash(
    state: tauri::State<'_, AppState>,
    project_id: String,
    file_path: String,
) -> Result<bool, String> {
    let root = project_root(&state, &project_id)?;
    let src = PathBuf::from(&file_path);
    if !src.is_absolute() {
        return Err("需要绝对路径".to_string());
    }
    // 恢复位置：回收站文件原名所在相对目录 -> 由文件名反查（简化：恢复回项目根下的待分类）
    let pending = root.join("1. 待分类");
    std::fs::create_dir_all(&pending).map_err(|e| e.to_string())?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let dest = pending.join(name);
    std::fs::rename(&src, &dest).map_err(|e| e.to_string())?;
    let cfg = state.config.lock().unwrap();
    let audit = audit_log::AuditLog::new(&root.join(".ocard/logs"), &cfg.machine_id);
    let _ = audit.log(&cfg.operator, "restore", &file_path, None);
    Ok(true)
}

/// 分类操作：把文件移动（或复制，P 标精选）到项目内的分类目录。
/// category 可为分类名（如「人像」「精选」「其他」），自动匹配带编号的文件夹。
#[tauri::command]
fn move_to_category(
    state: tauri::State<'_, AppState>,
    project_id: String,
    file_path: String,
    category: String,
    copy: bool,
) -> Result<bool, String> {
    let root = project_root(&state, &project_id)?;
    let src = PathBuf::from(&file_path);
    if !src.is_absolute() {
        return Err("需要绝对路径".to_string());
    }
    let project = project_store::Project::load(&root)?;
    let folder = project
        .folder_template()
        .iter()
        .find(|f| f.contains(&category) || f.ends_with(&category) || f.starts_with(&category))
        .cloned()
        .ok_or_else(|| format!("未找到分类目录：{category}"))?;
    // 精选 -> 自动进「待修」子目录
    let dest_dir = if folder.contains("精选") {
        root.join(&folder).join("待修")
    } else {
        root.join(&folder)
    };
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let dest = unique_dest(dest_dir.join(name));
    if copy {
        std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    } else {
        std::fs::rename(&src, &dest).map_err(|e| e.to_string())?;
    }
    let cfg = state.config.lock().unwrap();
    let audit = audit_log::AuditLog::new(&root.join(".ocard/logs"), &cfg.machine_id);
    let _ = audit.log(
        &cfg.operator,
        if copy { "select" } else { "classify" },
        &file_path,
        Some(format!("-> {}", dest.display())),
    );
    Ok(true)
}

/// 目标同名冲突时追加序号
fn unique_dest(mut dest: PathBuf) -> PathBuf {
    if !dest.exists() {
        return dest;
    }
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let (base, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.clone(), String::new()),
    };
    let mut n = 1;
    loop {
        let candidate = dest.with_file_name(format!("{base}_{n}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

// ---------------------------------------------------------------------------
// 成片命名校验
// ---------------------------------------------------------------------------

#[tauri::command]
fn validate_final_name(name: String) -> Result<naming::FinalVideoName, String> {
    naming::validate_final_video_name(&name)
}

// ---------------------------------------------------------------------------
// 转码引擎
// ---------------------------------------------------------------------------

#[tauri::command]
fn probe_ffmpeg() -> transcode::EncoderInfo {
    transcode::probe_ffmpeg()
}

#[tauri::command]
fn list_transcode_jobs(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> Result<transcode::TranscodeQueue, String> {
    let root = project_root(&state, &project_id)?;
    let path = root.join(".ocard/transcode_queue.json");
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        Err(_) => Ok(transcode::TranscodeQueue::default()),
    }
}

fn save_queue(root: &Path, queue: &transcode::TranscodeQueue) -> Result<(), String> {
    let path = root.join(".ocard/transcode_queue.json");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(queue).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_transcode_job(
    state: tauri::State<'_, AppState>,
    project_id: String,
    input_path: String,
    output_rel_dir: String,
    kind: transcode::JobKind,
) -> Result<transcode::TranscodeJob, String> {
    let root = project_root(&state, &project_id)?;
    let mut queue = list_transcode_jobs(state.clone(), project_id.clone())?;
    let output_dir = root.join(&output_rel_dir);
    std::fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;
    let job = queue.add(PathBuf::from(&input_path), output_dir, kind);
    save_queue(&root, &queue)?;
    Ok(job)
}

#[tauri::command]
fn run_transcode_job(
    state: tauri::State<'_, AppState>,
    project_id: String,
    job_id: String,
) -> Result<transcode::TranscodeJob, String> {
    let root = project_root(&state, &project_id)?;
    let mut queue = list_transcode_jobs(state.clone(), project_id.clone())?;
    // 准备：标记 Running，生成输出路径与完整命令
    let (input, output, cmd) = {
        let job = queue
            .jobs
            .iter_mut()
            .find(|j| j.id == job_id)
            .ok_or_else(|| "任务不存在".to_string())?;
        if job.status == transcode::JobStatus::Running {
            return Err("任务运行中".to_string());
        }
        let encoders = transcode::probe_ffmpeg();
        if !encoders.ffmpeg_available {
            return Err("未找到 ffmpeg，请安装后重试".to_string());
        }
        let input = job.input.clone();
        let output_dir = job.output_dir.clone();
        let kind = job.kind.clone();
        let out_name = transcode::output_filename(
            &input
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            &kind,
        );
        let output = output_dir.join(out_name);
        let cmd = transcode::build_ffmpeg_command(&kind, &input, &output, &encoders);
        job.status = transcode::JobStatus::Running;
        job.command = Some(cmd.clone());
        (input, output, cmd)
    };
    save_queue(&root, &queue)?;

    // 执行 ffmpeg（sidecar）
    let result = std::process::Command::new(&cmd[0]).args(&cmd[1..]).output();

    let mut queue2 = list_transcode_jobs(state.clone(), project_id.clone())?;
    let result_job = {
        let job2 = queue2
            .jobs
            .iter_mut()
            .find(|j| j.id == job_id)
            .ok_or_else(|| "任务不存在".to_string())?;
        match result {
            Ok(out) if out.status.success() => {
                job2.status = transcode::JobStatus::Done;
                let cfg = state.config.lock().unwrap();
                let audit = audit_log::AuditLog::new(&root.join(".ocard/logs"), &cfg.machine_id);
                let _ = audit.log(
                    &cfg.operator,
                    "transcode",
                    &input.to_string_lossy(),
                    Some(format!("输出={output:?}")),
                );
            }
            Ok(out) => {
                job2.status = transcode::JobStatus::Failed;
                job2.error = Some(String::from_utf8_lossy(&out.stderr).chars().take(500).collect());
            }
            Err(e) => {
                job2.status = transcode::JobStatus::Failed;
                job2.error = Some(e.to_string());
            }
        }
        job2.clone()
    };
    save_queue(&root, &queue2)?;
    Ok(result_job)
}

// ---------------------------------------------------------------------------
// 交付打包
// ---------------------------------------------------------------------------

/// 扫描已分类文件夹，构造打包输入（含拍摄时间）
#[tauri::command]
fn list_package_inputs(
    state: tauri::State<'_, AppState>,
    project_id: String,
    folder_rel: String,
) -> Result<Vec<packaging::PackageInput>, String> {
    let root = project_root(&state, &project_id)?;
    let dir = root.join(&folder_rel);
    if !dir.is_dir() {
        return Err(format!("文件夹不存在：{folder_rel}"));
    }
    let items = media_indexer::scan_media(&dir);
    let mut inputs = Vec::new();
    for item in items {
        let datetime = chrono::NaiveDateTime::parse_from_str(&item.datetime_original, "%Y-%m-%d %H:%M:%S").ok();
        inputs.push(packaging::PackageInput {
            path: PathBuf::from(&item.path),
            name: item.name,
            size: item.size,
            datetime,
        });
    }
    Ok(inputs)
}

#[tauri::command]
fn run_packaging(
    state: tauri::State<'_, AppState>,
    project_id: String,
    folder_rel: String,
) -> Result<packaging::DeliveryManifest, String> {
    let root = project_root(&state, &project_id)?;
    let inputs = list_package_inputs(state.clone(), project_id.clone(), folder_rel.clone())?;
    let cfg = state.config.lock().unwrap();
    let (_, manifest) = packaging::run_packaging(&root, &inputs, &cfg.operator)?;
    let audit = audit_log::AuditLog::new(&root.join(".ocard/logs"), &cfg.machine_id);
    let _ = audit.log(
        &cfg.operator,
        "package",
        &folder_rel,
        Some(format!("包数={} 文件={}", manifest.packages.len(), manifest.total_files)),
    );
    Ok(manifest)
}

#[tauri::command]
fn upload_list(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> Result<String, String> {
    let root = project_root(&state, &project_id)?;
    let path = root.join(".ocard/deliveries/latest.json");
    let s = std::fs::read_to_string(&path).map_err(|_| "尚未打包".to_string())?;
    let manifest: packaging::DeliveryManifest =
        serde_json::from_str(&s).map_err(|e| e.to_string())?;
    Ok(packaging::upload_list_text(&manifest))
}

// ---------------------------------------------------------------------------
// 审计日志
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_audit_log(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> Result<Vec<audit_log::AuditEntry>, String> {
    let root = project_root(&state, &project_id)?;
    let cfg = state.config.lock().unwrap();
    let audit = audit_log::AuditLog::new(&root.join(".ocard/logs"), &cfg.machine_id);
    Ok(audit.read_all())
}

// ---------------------------------------------------------------------------
// 应用入口
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState {
        config: Mutex::new(AppConfig::load()),
    };
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            list_projects,
            create_project,
            get_project_summary,
            get_project,
            list_devices,
            add_camera,
            add_card,
            scan_volumes,
            run_copy,
            list_manifests,
            scan_media_folder,
            make_thumbnail,
            analyze_folder,
            burst_groups,
            move_to_trash,
            restore_from_trash,
            move_to_category,
            validate_final_name,
            probe_ffmpeg,
            list_transcode_jobs,
            add_transcode_job,
            run_transcode_job,
            list_package_inputs,
            run_packaging,
            upload_list,
            get_audit_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
