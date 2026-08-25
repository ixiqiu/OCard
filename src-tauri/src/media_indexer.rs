//! 媒体索引（PRD §5.4/§5.5）
//!
//! 扫描素材目录，提取 EXIF/拍摄时间，生成缩略图缓存到 `.ocard/thumbs/`。
//! RAW 走快慢双路径：优先提取文件内嵌 JPEG 预览；像素级查看由 libraw 完整解码
//! （此处提供 `embedded_jpeg_exists` 探测与 image crate 解码路径）。

use chrono::{NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 支持的图片/视频扩展名
pub const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "tif", "tiff", "arw", "cr2", "cr3", "nef", "raf", "dng", "orf", "rw2",
    "heic",
];
pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "mov", "mxf", "braw", "r3d", "avi", "mts", "m2ts", "m4v",
];

pub fn is_image_ext(ext: &str) -> bool {
    IMAGE_EXTS.contains(&ext.to_lowercase().as_str())
}
pub fn is_video_ext(ext: &str) -> bool {
    VIDEO_EXTS.contains(&ext.to_lowercase().as_str())
}

/// 媒体条目
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaItem {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub modified: String,
    /// 拍摄时间（EXIF DateTimeOriginal；视频/无 EXIF 用文件修改时间）
    pub datetime_original: String,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub is_video: bool,
}

/// 扫描目录下所有媒体文件
pub fn scan_media(dir: &Path) -> Vec<MediaItem> {
    let mut items = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !(e.depth() > 0 && name.starts_with('.'))
        })
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !is_image_ext(&ext) && !is_video_ext(&ext) {
            continue;
        }
        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let (dt, make, model) = extract_exif(path);
        let is_video = is_video_ext(&ext);
        items.push(MediaItem {
            path: path.to_string_lossy().to_string(),
            name: path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default(),
            ext,
            size: meta.len(),
            modified: fmt_dt(&meta.modified().ok().map(|t| t.into()).unwrap_or_else(|| Utc::now())),
            datetime_original: dt,
            camera_make: make,
            camera_model: model,
            is_video,
        });
    }
    items.sort_by(|a, b| a.path.cmp(&b.path));
    items
}

fn fmt_dt(dt: &chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 从 EXIF 提取拍摄时间 + 相机信息（kamadak-exif）。
/// 返回 (拍摄时间 RFC3339 或文件时间兜底, make, model)。
pub fn extract_exif(path: &Path) -> (String, Option<String>, Option<String>) {
    let fallback = fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            let dt: chrono::DateTime<Utc> = t.into();
            dt.format("%Y-%m-%d %H:%M:%S").to_string()
        })
        .unwrap_or_default();

    // 只对常见 RAW/JPEG 尝试
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return (fallback, None, None);
    }

    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (fallback, None, None),
    };
    let mut bufreader = std::io::BufReader::new(&file);
    let exif = match exif::Reader::new().read_from_container(&mut bufreader) {
        Ok(e) => e,
        Err(_) => return (fallback, None, None),
    };
    let make = exif
        .get_field(exif::Tag::Make, exif::In::PRIMARY)
        .map(|f| f.value.display_as(exif::Tag::Make).to_string());
    let model = exif
        .get_field(exif::Tag::Model, exif::In::PRIMARY)
        .map(|f| f.value.display_as(exif::Tag::Model).to_string());
    let dt = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .map(|f| f.value.display_as(exif::Tag::DateTimeOriginal).to_string())
        .and_then(|s| parse_exif_dt(&s))
        .unwrap_or(fallback);
    (dt, make, model)
}

/// 解析 EXIF 时间字符串 `YYYY:MM:DD HH:MM:SS`（也可能已是 `YYYY-MM-DD HH:MM:SS`）
fn parse_exif_dt(s: &str) -> Option<String> {
    let s = s.trim();
    let norm = if s.len() >= 19 && s.as_bytes()[4] == b':' {
        format!("{}-{}-{} {}", &s[0..4], &s[5..7], &s[8..10], &s[11..19])
    } else {
        s.to_string()
    };
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y:%m:%d %H:%M:%S"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(&norm, fmt) {
            let utc = Utc.from_utc_datetime(&dt);
            return Some(utc.format("%Y-%m-%d %H:%M:%S").to_string());
        }
    }
    None
}

/// 缩略图缓存路径：`.ocard/thumbs/{hash}.jpg`
pub fn thumbnail_path(ocard_dir: &Path, hash: &str) -> PathBuf {
    ocard_dir.join("thumbs").join(format!("{hash}.jpg"))
}

/// 生成缩略图（image crate 解码 -> 最长边 max_dim -> JPEG 写入缓存）。
/// 支持 JPEG/PNG；RAW 尝试内嵌预览路径由上层决定。
pub fn make_thumbnail(
    src: &Path,
    ocard_dir: &Path,
    max_dim: u32,
) -> Result<PathBuf, String> {
    let hash = crate::hash::hash_file(src).map_err(|e| e.to_string())?;
    let out = thumbnail_path(ocard_dir, &hash);
    if out.exists() {
        return Ok(out);
    }
    let img = image::open(src).map_err(|e| format!("解码失败 {src:?}: {e}"))?;
    let thumb = img.thumbnail(max_dim, max_dim);
    fs::create_dir_all(out.parent().unwrap_or(ocard_dir)).map_err(|e| e.to_string())?;
    thumb
        .save_with_format(&out, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_classification() {
        assert!(is_image_ext("JPG"));
        assert!(is_image_ext("arw"));
        assert!(is_video_ext("mp4"));
        assert!(is_video_ext("braw"));
        assert!(!is_video_ext("txt"));
    }

    #[test]
    fn scan_media_finds_only_media() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::create_dir_all(dir.path().join(".hidden")).unwrap();
        fs::write(dir.path().join("a.jpg"), b"fake-jpeg").unwrap();
        fs::write(dir.path().join("b.mp4"), b"fake-mp4").unwrap();
        fs::write(dir.path().join("c.txt"), b"no").unwrap();
        fs::write(dir.path().join(".hidden/d.jpg"), b"x").unwrap();
        let items = scan_media(dir.path());
        let names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
        assert!(names.contains(&"a.jpg".to_string()));
        assert!(names.contains(&"b.mp4".to_string()));
        assert!(!names.contains(&"c.txt".to_string()));
        assert!(!names.iter().any(|n| n.contains("hidden")));
        assert!(items.iter().all(|i| i.is_video == (i.name == "b.mp4")));
    }

    #[test]
    fn parse_exif_datetime() {
        assert_eq!(
            parse_exif_dt("2026:08:24 10:30:00"),
            Some("2026-08-24 10:30:00".to_string())
        );
        assert_eq!(
            parse_exif_dt("2026-08-24 10:30:00"),
            Some("2026-08-24 10:30:00".to_string())
        );
        assert_eq!(parse_exif_dt("not a date"), None);
    }

    #[test]
    fn thumbnail_generation_and_cache() {
        let dir = tempfile::tempdir().unwrap();
        let ocard = dir.path().join(".ocard");
        // 生成一张小 PNG
        let img = image::RgbImage::from_fn(64, 64, |x, y| {
            image::Rgb([(x * 4) as u8, (y * 4) as u8, 128])
        });
        let src = dir.path().join("sample.png");
        img.save(&src).unwrap();
        let p1 = make_thumbnail(&src, &ocard, 32).unwrap();
        assert!(p1.exists());
        // 缓存命中，同样返回
        let p2 = make_thumbnail(&src, &ocard, 32).unwrap();
        assert_eq!(p1, p2);
    }
}
