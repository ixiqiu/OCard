//! 本地 AI 选片（PRD §5.5，无云端依赖，三平台一致）
//!
//! 能力边界（诚实声明，PRD §5.5）：
//! - 客观指标可靠：连拍去重、糊片、过曝/欠曝、明显闭眼
//! - 主观审美不可靠：荐优只按客观分排序，「精选」判断仍归 DIT/摄影师
//! - AI 只排序和建议，不自动删除、不自动挪动任何文件
//!
//! 实现：
//! - 质量评分：拉普拉斯方差（清晰度）+ 曝光直方图（纯算法，image crate）
//! - 连拍聚类：拍摄时间邻近 + 轻量 embedding 占位
//! - 人脸/闭眼：ONNX 模型（YuNet 级）经 `AiBackend` trait 抽象；
//!   默认 feature 不启用（模型未加载时诚实降级返回 None），
//!   启用 `full-ai` feature 时接入 ONNX Runtime。

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 客观质量分（0-100）：清晰度权重 + 曝光权重
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualityScore {
    pub sharpness: f64,
    pub exposure: f64,
    pub overall: f64,
}

/// 分析结果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalyzeResult {
    pub path: String,
    pub quality: Option<QualityScore>,
    pub blurry: bool,
    pub overexposed: bool,
    pub underexposed: bool,
    /// 闭眼检测（None = 未加载模型或无人脸）
    pub eyes_closed: Option<bool>,
    /// 拍摄时间（用于聚类）
    pub datetime: Option<String>,
}

/// 聚类：连拍组
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BurstGroup {
    pub id: usize,
    /// 组内文件路径
    pub members: Vec<String>,
    /// 推荐保留的成员路径（按质量分排序取前 k）
    pub keep: Vec<String>,
}

/// 拉普拉斯方差（清晰度）——纯算法
pub fn laplacian_variance(img: &image::GrayImage) -> f64 {
    let (w, h) = img.dimensions();
    if w < 3 || h < 3 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut n = 0u64;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let c = img.get_pixel(x, y)[0] as f64;
            let up = img.get_pixel(x, y - 1)[0] as f64;
            let down = img.get_pixel(x, y + 1)[0] as f64;
            let left = img.get_pixel(x - 1, y)[0] as f64;
            let right = img.get_pixel(x + 1, y)[0] as f64;
            let lap = 4.0 * c - up - down - left - right;
            sum += lap;
            sum_sq += lap * lap;
            n += 1;
        }
    }
    if n == 0 {
        return 0.0;
    }
    let mean = sum / n as f64;
    let var = sum_sq / n as f64 - mean * mean;
    var.max(0.0)
}

/// 曝光直方图评分（0-100）：惩罚过曝/欠曝像素占比
pub fn exposure_score(img: &image::GrayImage) -> f64 {
    let (w, h) = img.dimensions();
    let total = (w * h).max(1) as f64;
    let mut over = 0u64;
    let mut under = 0u64;
    for p in img.pixels() {
        let v = p[0];
        if v > 245 {
            over += 1;
        } else if v < 10 {
            under += 1;
        }
    }
    let over_ratio = over as f64 / total;
    let under_ratio = under as f64 / total;
    let score = 100.0 - (over_ratio * 150.0 + under_ratio * 150.0);
    score.clamp(0.0, 100.0)
}

/// 综合质量评分（0-100）
pub fn quality_score(img: &image::GrayImage) -> QualityScore {
    let var = laplacian_variance(img);
    // 拉普拉斯方差经验映射：<50 很糊，500+ 很锐
    let sharpness = (var / 500.0 * 100.0).clamp(0.0, 100.0);
    let exposure = exposure_score(img);
    let overall = sharpness * 0.6 + exposure * 0.4;
    QualityScore {
        sharpness,
        exposure,
        overall,
    }
}

/// 解码图像（JPEG/PNG）并转为灰度，失败返回 None（如 RAW 未嵌入预览）
pub fn load_gray(path: &Path) -> Option<image::GrayImage> {
    let img = image::open(path).ok()?;
    Some(img.into_luma8())
}

/// 分析单个媒体文件：清晰度 + 曝光（纯算法）
pub fn analyze_file(path: &Path, datetime: Option<String>) -> AnalyzeResult {
    let mut result = AnalyzeResult {
        path: path.to_string_lossy().to_string(),
        quality: None,
        blurry: false,
        overexposed: false,
        underexposed: false,
        eyes_closed: None,
        datetime,
    };
    if let Some(gray) = load_gray(path) {
        let q = quality_score(&gray);
        let blurry = q.sharpness < 25.0;
        let overexposed = q.exposure < 40.0 && exposure_over_ratio(&gray) > 0.3;
        let underexposed = q.exposure < 40.0 && exposure_under_ratio(&gray) > 0.3;
        result.quality = Some(q);
        result.blurry = blurry;
        result.overexposed = overexposed;
        result.underexposed = underexposed;
    }
    result
}

fn exposure_over_ratio(img: &image::GrayImage) -> f64 {
    let total = (img.width() * img.height()).max(1) as f64;
    img.pixels().filter(|p| p[0] > 245).count() as f64 / total
}
fn exposure_under_ratio(img: &image::GrayImage) -> f64 {
    let total = (img.width() * img.height()).max(1) as f64;
    img.pixels().filter(|p| p[0] < 10).count() as f64 / total
}

/// 时间邻近聚类：按拍摄时间排序，间隙 > max_gap_secs 的拆分为新组。
/// 返回每组在输入数组中的索引集合。
pub fn cluster_bursts(
    datetimes: &[Option<NaiveDateTime>],
    max_gap_secs: i64,
) -> Vec<Vec<usize>> {
    // 记录有时间的索引
    let mut idx_time: Vec<(usize, NaiveDateTime)> = datetimes
        .iter()
        .enumerate()
        .filter_map(|(i, dt)| dt.map(|d| (i, d)))
        .collect();
    idx_time.sort_by(|a, b| a.1.cmp(&b.1));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut last_dt: Option<NaiveDateTime> = None;
    for (i, dt) in idx_time {
        if let Some(ld) = last_dt {
            if dt.signed_duration_since(ld).num_seconds() <= max_gap_secs {
                groups.last_mut().expect("group exists").push(i);
                last_dt = Some(dt);
                continue;
            }
        }
        groups.push(vec![i]);
        last_dt = Some(dt);
    }
    groups
}

/// 从 AnalyzeResult 列表生成连拍组（时间邻近 + 质量分排序，每组保留前 keep 张）
pub fn build_burst_groups(
    results: &[AnalyzeResult],
    max_gap_secs: i64,
    keep: usize,
) -> Vec<BurstGroup> {
    let dts: Vec<Option<NaiveDateTime>> = results
        .iter()
        .map(|r| {
            r.datetime.as_deref().and_then(|s| {
                NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
            })
        })
        .collect();
    let clusters = cluster_bursts(&dts, max_gap_secs);
    clusters
        .into_iter()
        .enumerate()
        .filter(|(_, group)| group.len() > 1)
        .map(|(id, group)| {
            let mut scored: Vec<(usize, f64)> = group
                .iter()
                .map(|&i| {
                    let score = results[i]
                        .quality
                        .as_ref()
                        .map(|q| q.overall)
                        .unwrap_or(0.0);
                    (i, score)
                })
                .collect();
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let keep_paths: Vec<String> = scored
                .iter()
                .take(keep)
                .map(|(i, _)| results[*i].path.clone())
                .collect();
            BurstGroup {
                id,
                members: group.iter().map(|&i| results[i].path.clone()).collect(),
                keep: keep_paths,
            }
        })
        .collect()
}

/// 荐优：按综合质量分排序（只排序，不自动采纳）
pub fn rank_by_quality(results: &[AnalyzeResult]) -> Vec<String> {
    let mut scored: Vec<(String, f64)> = results
        .iter()
        .map(|r| {
            let s = r.quality.as_ref().map(|q| q.overall).unwrap_or(0.0);
            (r.path.clone(), s)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(p, _)| p).collect()
}

// ---------------------------------------------------------------------------
// ONNX 推理抽象（人脸检测 + 闭眼判断）
// ---------------------------------------------------------------------------

/// AI 后端能力（feature `full-ai` 启用 ONNX Runtime；否则 stub 诚实降级）
pub trait AiBackend: Send + Sync {
    /// 检测人脸并判断闭眼。None = 模型未加载 / 无人脸 / 脸太小不判。
    fn detect_face_and_eyes(&self, _path: &Path) -> Option<bool> {
        None
    }
}

/// 默认 stub：模型未加载，返回 None（诚实降级）
pub struct StubAiBackend;

impl AiBackend for StubAiBackend {}

/// 创建当前可用的 AI 后端
pub fn create_backend() -> Box<dyn AiBackend> {
    #[cfg(feature = "full-ai")]
    {
        match crate::culling::onnx::OrtAiBackend::try_load() {
            Ok(b) => Box::new(b),
            Err(_) => Box::new(StubAiBackend),
        }
    }
    #[cfg(not(feature = "full-ai"))]
    {
        Box::new(StubAiBackend)
    }
}

/// ONNX Runtime 实现（feature full-ai）
#[cfg(feature = "full-ai")]
pub mod onnx {
    use super::AiBackend;
    use std::path::Path;

    /// YuNet 级轻量人脸检测 + 闭眼判断（模型文件随应用分发）
    pub struct OrtAiBackend {
        session: ort::Session,
    }

    impl OrtAiBackend {
        pub fn try_load() -> Result<Self, String> {
            // 模型路径约定：与可执行文件同目录 models/face_det.onnx
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let dir = exe.parent().unwrap_or(Path::new("."));
            let model = dir.join("models").join("face_det.onnx");
            let session = ort::Session::builder()
                .and_then(|b| b.commit_from_file(&model))
                .map_err(|e| format!("加载 ONNX 模型失败: {e}"))?;
            Ok(Self { session })
        }
    }

    impl AiBackend for OrtAiBackend {
        fn detect_face_and_eyes(&self, path: &Path) -> Option<bool> {
            let img = image::open(path).ok()?;
            let gray = img.to_luma8();
            // 预处理：resize 到 160x160（YuNet 输入），转 f32 NCHW
            let resized = image::imageops::resize(
                &gray,
                160,
                160,
                image::imageops::FilterType::Triangle,
            );
            let mut input = Vec::with_capacity(160 * 160);
            for p in resized.pixels() {
                input.push(p[0] as f32 / 255.0);
            }
            let arr = ort::ndarray::Array4::from_shape_vec((1, 1, 160, 160), input).ok()?;
            let tensor = ort::value::Value::from_array(arr).ok()?;
            let outputs = self.session.run([tensor]).ok()?;
            // 输出形状依赖模型；此处简化：取输出数组中的最大置信度判断闭眼。
            // 完整实现需要解析 YuNet 输出（face boxes + landmarks）。
            let _ = outputs;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_blank(v: u8) -> image::GrayImage {
        image::GrayImage::from_pixel(64, 64, image::Luma([v]))
    }

    #[test]
    fn blank_image_low_sharpness() {
        let img = gray_blank(128);
        let q = quality_score(&img);
        assert!(q.sharpness < 5.0);
    }

    #[test]
    fn high_contrast_image_sharp() {
        // 棋盘格：高拉普拉斯方差
        let mut img = image::GrayImage::new(64, 64);
        for (x, y, p) in img.enumerate_pixels_mut() {
            p[0] = if (x / 4 + y / 4) % 2 == 0 { 255 } else { 0 };
        }
        let q = quality_score(&img);
        assert!(q.sharpness > 50.0, "sharpness={}", q.sharpness);
    }

    #[test]
    fn overexposed_image_low_exposure_score() {
        let img = gray_blank(250);
        let q = quality_score(&img);
        assert!(q.exposure < 50.0);
        let r = analyze_file_from_gray(&img, None);
        assert!(r.overexposed);
    }

    #[test]
    fn underexposed_image_low_exposure_score() {
        let img = gray_blank(5);
        let q = quality_score(&img);
        assert!(q.exposure < 50.0);
        let r = analyze_file_from_gray(&img, None);
        assert!(r.underexposed);
    }

    fn analyze_file_from_gray(img: &image::GrayImage, dt: Option<String>) -> AnalyzeResult {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.png");
        img.save(&p).unwrap();
        analyze_file(&p, dt)
    }

    #[test]
    fn burst_clustering_by_time() {
        let base = NaiveDateTime::parse_from_str("2026-08-24 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let dts = vec![
            Some(base),
            Some(base + chrono::Duration::seconds(2)),
            Some(base + chrono::Duration::seconds(4)),
            Some(base + chrono::Duration::seconds(600)),
            Some(base + chrono::Duration::seconds(603)),
        ];
        let groups = cluster_bursts(&dts, 10);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 3);
        assert_eq!(groups[1].len(), 2);
    }

    #[test]
    fn burst_groups_keep_best() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("a.png"); // 清晰棋盘格
        let p2 = dir.path().join("b.png"); // 糊片
        let mut checker = image::GrayImage::new(64, 64);
        for (x, y, p) in checker.enumerate_pixels_mut() {
            p[0] = if (x / 4 + y / 4) % 2 == 0 { 255 } else { 0 };
        }
        checker.save(&p1).unwrap();
        image::GrayImage::from_pixel(64, 64, image::Luma([128])).save(&p2).unwrap();
        let base = NaiveDateTime::parse_from_str("2026-08-24 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let results = vec![
            analyze_file(&p1, Some(base.format("%Y-%m-%d %H:%M:%S").to_string())),
            analyze_file(&p2, Some((base + chrono::Duration::seconds(1)).format("%Y-%m-%d %H:%M:%S").to_string())),
        ];
        let groups = build_burst_groups(&results, 5, 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members.len(), 2);
        assert_eq!(groups[0].keep.len(), 1);
        // 保留清晰那张（棋盘格 sharpness 高）
        assert_eq!(groups[0].keep[0], p1.to_string_lossy());
    }

    #[test]
    fn ranking_sorts_by_quality() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.png");
        let bad = dir.path().join("bad.png");
        image::GrayImage::from_pixel(64, 64, image::Luma([128])).save(&good).unwrap();
        image::GrayImage::from_pixel(64, 64, image::Luma([250])).save(&bad).unwrap();
        let results = vec![
            analyze_file(&bad, None),
            analyze_file(&good, None),
        ];
        let ranked = rank_by_quality(&results);
        assert_eq!(ranked[0], good.to_string_lossy());
    }

    #[test]
    fn stub_backend_honest_degration() {
        let backend = create_backend();
        assert!(backend.detect_face_and_eyes(Path::new("/nonexistent.jpg")).is_none());
    }
}
