//! 转码引擎（PRD §5.6，工况 A 为主）
//!
//! 统一由 ffmpeg sidecar 承担，按平台探测可用硬件编码器：
//! NVENC（NVIDIA）/ QSV（Intel）/ AMF（AMD）/ VideoToolbox（macOS）/ VAAPI（Linux），
//! 全部不可用时回落 x264/x265 软编。
//!
//! - 代理转码：高负载素材（Log、高码率）批量转代理到 `4. 转码素材`，按相机分子夹
//! - 归档转码：可选 HEVC 10-bit 三档压缩（高质量/平衡/高压缩），默认保留原文件

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 转码预设档位
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QualityPreset {
    /// 高质量（归档）
    High,
    /// 平衡
    Balanced,
    /// 高压缩
    Compact,
}

impl QualityPreset {
    pub fn label(&self) -> &'static str {
        match self {
            QualityPreset::High => "高质量",
            QualityPreset::Balanced => "平衡",
            QualityPreset::Compact => "高压缩",
        }
    }

    /// HEVC 归档三档 CRF（10-bit）
    pub fn hevc_crf(&self) -> i32 {
        match self {
            QualityPreset::High => 18,
            QualityPreset::Balanced => 23,
            QualityPreset::Compact => 28,
        }
    }

    /// 代理档 CRF
    pub fn proxy_crf(&self) -> i32 {
        match self {
            QualityPreset::High => 18,
            QualityPreset::Balanced => 23,
            QualityPreset::Compact => 28,
        }
    }
}

/// 转码任务
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscodeJob {
    pub id: String,
    pub input: PathBuf,
    /// 输出目录（通常为 `4. 转码素材/{相机编码}`）
    pub output_dir: PathBuf,
    pub kind: JobKind,
    pub status: JobStatus,
    /// ffmpeg 完整命令（生成后保存，便于排查）
    pub command: Option<Vec<String>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobKind {
    /// 代理转码（h264 低码率）
    Proxy { preset: QualityPreset },
    /// 归档转码（HEVC 10-bit）
    Archive { preset: QualityPreset },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Done,
    Failed,
}

/// 平台可用编码器
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct EncoderInfo {
    pub platform: String,
    pub nvenc: bool,
    pub qsv: bool,
    pub amf: bool,
    pub videotoolbox: bool,
    pub vaapi: bool,
    pub ffmpeg_available: bool,
}

impl EncoderInfo {
    /// 当前首选编码器名（供 ffmpeg `-c:v`），按平台优先级
    pub fn preferred_video_codec(&self) -> String {
        #[cfg(target_os = "macos")]
        {
            if self.videotoolbox {
                return "h264_videotoolbox".to_string();
            }
        }
        #[cfg(target_os = "windows")]
        {
            if self.nvenc {
                return "h264_nvenc".to_string();
            }
            if self.qsv {
                return "h264_qsv".to_string();
            }
            if self.amf {
                return "h264_amf".to_string();
            }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            if self.nvenc {
                return "h264_nvenc".to_string();
            }
            if self.qsv {
                return "h264_qsv".to_string();
            }
            if self.vaapi {
                return "h264_vaapi".to_string();
            }
        }
        "libx264".to_string()
    }

    /// HEVC 首选编码器（归档）
    pub fn preferred_hevc_codec(&self) -> String {
        #[cfg(target_os = "macos")]
        {
            if self.videotoolbox {
                return "hevc_videotoolbox".to_string();
            }
        }
        #[cfg(target_os = "windows")]
        {
            if self.nvenc {
                return "hevc_nvenc".to_string();
            }
            if self.qsv {
                return "hevc_qsv".to_string();
            }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            if self.nvenc {
                return "hevc_nvenc".to_string();
            }
            if self.qsv {
                return "hevc_qsv".to_string();
            }
        }
        "libx265".to_string()
    }
}

/// 探测 ffmpeg 是否可用及硬件编码器列表（调用 `ffmpeg -hide_banner -encoders`）
pub fn probe_ffmpeg() -> EncoderInfo {
    let mut info = EncoderInfo {
        platform: std::env::consts::OS.to_string(),
        ..Default::default()
    };
    let out = match std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return info,
    };
    info.ffmpeg_available = true;
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    info.nvenc = text.contains("nvenc") || text.contains("cuda");
    info.qsv = text.contains("qsv");
    info.amf = text.contains("amf");
    info.videotoolbox = text.contains("videotoolbox");
    info.vaapi = text.contains("vaapi");
    info
}

/// 生成 ffmpeg 命令参数（不实际执行；由队列调度执行）
pub fn build_ffmpeg_command(
    job: &JobKind,
    input: &PathBuf,
    output: &PathBuf,
    encoders: &EncoderInfo,
) -> Vec<String> {
    let mut cmd = vec![
        "ffmpeg".to_string(),
        "-y".to_string(),
        "-i".to_string(),
        input.to_string_lossy().to_string(),
    ];
    match job {
        JobKind::Proxy { preset } => {
            // 代理：h264，缩到 1080p 以内，低码率，适合剪辑
            cmd.push("-vf".to_string());
            cmd.push("scale='min(1920,iw)':-2".to_string());
            cmd.push("-c:v".to_string());
            cmd.push(encoders.preferred_video_codec());
            if encoders.preferred_video_codec() == "libx264" {
                cmd.push("-preset".to_string());
                cmd.push("veryfast".to_string());
            }
            cmd.push("-crf".to_string());
            cmd.push(preset.proxy_crf().to_string());
            cmd.push("-c:a".to_string());
            cmd.push("aac".to_string());
            cmd.push("-b:a".to_string());
            cmd.push("128k".to_string());
            cmd.push("-movflags".to_string());
            cmd.push("+faststart".to_string());
        }
        JobKind::Archive { preset } => {
            // 归档：HEVC 10-bit，保留原分辨率
            cmd.push("-c:v".to_string());
            cmd.push(encoders.preferred_hevc_codec());
            cmd.push("-pix_fmt".to_string());
            cmd.push("yuv420p10le".to_string());
            if encoders.preferred_hevc_codec() == "libx265" {
                cmd.push("-preset".to_string());
                cmd.push("medium".to_string());
            }
            cmd.push("-crf".to_string());
            cmd.push(preset.hevc_crf().to_string());
            cmd.push("-c:a".to_string());
            cmd.push("aac".to_string());
            cmd.push("-b:a".to_string());
            cmd.push("192k".to_string());
            cmd.push("-movflags".to_string());
            cmd.push("+faststart".to_string());
        }
    }
    cmd.push(output.to_string_lossy().to_string());
    cmd
}

/// 输出文件名：输入名 + 后缀（代理/归档）
pub fn output_filename(input_name: &str, kind: &JobKind) -> String {
    let stem = match input_name.rfind('.') {
        Some(i) if i > 0 => &input_name[..i],
        _ => input_name,
    };
    let ext = "mp4";
    match kind {
        JobKind::Proxy { .. } => format!("{stem}_proxy.{ext}"),
        JobKind::Archive { .. } => format!("{stem}_arc.{ext}"),
    }
}

/// 简单转码队列
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TranscodeQueue {
    pub jobs: Vec<TranscodeJob>,
}

impl TranscodeQueue {
    pub fn add(&mut self, input: PathBuf, output_dir: PathBuf, kind: JobKind) -> TranscodeJob {
        let id = format!("tj-{}", self.jobs.len() + 1);
        let job = TranscodeJob {
            id,
            input,
            output_dir,
            kind,
            status: JobStatus::Pending,
            command: None,
            error: None,
        };
        self.jobs.push(job.clone());
        job
    }

    pub fn pending(&self) -> usize {
        self.jobs.iter().filter(|j| j.status == JobStatus::Pending).count()
    }

    pub fn running(&self) -> usize {
        self.jobs.iter().filter(|j| j.status == JobStatus::Running).count()
    }

    pub fn done_count(&self) -> usize {
        self.jobs.iter().filter(|j| j.status == JobStatus::Done).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_labels_and_crf() {
        assert_eq!(QualityPreset::High.label(), "高质量");
        assert_eq!(QualityPreset::High.hevc_crf(), 18);
        assert_eq!(QualityPreset::Balanced.hevc_crf(), 23);
        assert_eq!(QualityPreset::Compact.hevc_crf(), 28);
    }

    #[test]
    fn encoder_preference_falls_back_to_software() {
        let e = EncoderInfo::default();
        assert_eq!(e.preferred_video_codec(), "libx264");
        assert_eq!(e.preferred_hevc_codec(), "libx265");
    }

    #[test]
    fn encoder_preference_prefers_hardware() {
        let e = EncoderInfo {
            platform: "linux".to_string(),
            nvenc: true,
            qsv: true,
            vaapi: true,
            ..Default::default()
        };
        // 非 macOS/Windows 分支：nvenc 优先
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(e.preferred_video_codec(), "h264_nvenc");

        let e_mac = EncoderInfo {
            platform: "macos".to_string(),
            videotoolbox: true,
            ..Default::default()
        };
        #[cfg(target_os = "macos")]
        assert_eq!(e_mac.preferred_video_codec(), "h264_videotoolbox");
    }

    #[test]
    fn build_proxy_command_contains_codec_and_scale() {
        let enc = EncoderInfo::default();
        let cmd = build_ffmpeg_command(
            &JobKind::Proxy { preset: QualityPreset::Balanced },
            &PathBuf::from("/in/LOG_001.mov"),
            &PathBuf::from("/out/LOG_001_proxy.mp4"),
            &enc,
        );
        assert_eq!(cmd[0], "ffmpeg");
        assert!(cmd.contains(&"-c:v".to_string()));
        assert!(cmd.contains(&"libx264".to_string()));
        assert!(cmd.contains(&"-vf".to_string()));
        assert_eq!(cmd.last().unwrap(), "/out/LOG_001_proxy.mp4");
    }

    #[test]
    fn build_archive_command_hevc10bit() {
        let enc = EncoderInfo::default();
        let cmd = build_ffmpeg_command(
            &JobKind::Archive { preset: QualityPreset::High },
            &PathBuf::from("/in/a.mov"),
            &PathBuf::from("/out/a_arc.mp4"),
            &enc,
        );
        assert!(cmd.contains(&"libx265".to_string()));
        assert!(cmd.contains(&"yuv420p10le".to_string()));
        assert!(cmd.contains(&"18".to_string()));
    }

    #[test]
    fn output_filename_appends_suffix() {
        assert_eq!(
            output_filename("A001_C001.mov", &JobKind::Proxy { preset: QualityPreset::Balanced }),
            "A001_C001_proxy.mp4"
        );
        assert_eq!(
            output_filename("A001_C001.mov", &JobKind::Archive { preset: QualityPreset::High }),
            "A001_C001_arc.mp4"
        );
    }

    #[test]
    fn queue_tracks_status() {
        let mut q = TranscodeQueue::default();
        q.add(PathBuf::from("/a.mov"), PathBuf::from("/out"), JobKind::Proxy {
            preset: QualityPreset::Balanced,
        });
        q.add(PathBuf::from("/b.mov"), PathBuf::from("/out"), JobKind::Archive {
            preset: QualityPreset::High,
        });
        assert_eq!(q.pending(), 2);
        q.jobs[0].status = JobStatus::Running;
        assert_eq!(q.running(), 1);
        q.jobs[0].status = JobStatus::Done;
        assert_eq!(q.done_count(), 1);
    }
}
