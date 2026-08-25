//! 命名规则 —— 规范的代码化（PRD §5.1/§5.2/§5.8）
//!
//! - 相机编码：`{型号}_{机位}_{使用者代称}`，如 `DJIRonin4D_B_ZS`
//! - 项目文件夹：`YYYYMMDD_项目名`
//! - 工况 A 建夹模板：1. 工程文件 / 2. 原始素材 / 3. 特别素材 / 4. 转码素材 / 5. 文字素材 / 6. 成片
//! - 工况 B 建夹模板：1. 待分类 / 2..n 各分类 / n+1. 精选(内含待修、已修) / n+2. 其他
//! - 时段：`MMDD上午/下午`，如 `0101上午`
//! - 成片命名：`时间日期_片名_分辨率_用途_版本`

use chrono::{Datelike, NaiveDateTime, Timelike};
use regex::Regex;
use std::sync::OnceLock;

/// Windows 非法文件名字符（三平台互通的关键边界，PRD §6.5）
const ILLEGAL_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Windows 保留设备名（简单覆盖常见项）
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 过滤 Windows 非法字符，保证三平台互通（PRD §6.5「文件名边界」）。
/// 同时清理控制字符、尾部点/空格，并规避保留设备名。
pub fn sanitize_filename(raw: &str) -> String {
    let mut s: String = raw
        .chars()
        .filter(|c| !ILLEGAL_CHARS.contains(c) && !c.is_control())
        .collect();
    // 去除首尾空白与尾部点
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    let s = s.trim().to_string();
    if s.is_empty() {
        return "_".to_string();
    }
    // 保留名（无扩展名部分）
    let stem = s.split('.').next().unwrap_or("").to_uppercase();
    if RESERVED_NAMES.contains(&stem.as_str()) {
        return format!("_{s}");
    }
    s
}

/// 相机编码：`{型号}_{机位}_{使用者代称}`，全部大写字母数字（过滤空格等）。
/// 示例：`DJIRonin4D_B_ZS`
pub fn camera_code(model: &str, position: &str, operator: &str) -> String {
    let clean = |s: &str| -> String {
        s.to_uppercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
    };
    let m = clean(model);
    let p = clean(position);
    let o = clean(operator);
    let m = if m.is_empty() { "CAM".to_string() } else { m };
    let p = if p.is_empty() { "X".to_string() } else { p };
    let o = if o.is_empty() { "DIT".to_string() } else { o };
    format!("{m}_{p}_{o}")
}

/// 项目 id：`YYYYMMDD_项目名`（已 sanitize）
pub fn project_id(date: &str, name: &str) -> String {
    let d = if date.len() == 8 && date.chars().all(|c| c.is_ascii_digit()) {
        date.to_string()
    } else {
        chrono::Local::now().format("%Y%m%d").to_string()
    };
    format!("{d}_{}", sanitize_filename(name))
}

/// 工况 A 建夹模板（PRD §5.2，序号已按 PRD 取齐）
pub fn workflow_a_folders() -> Vec<String> {
    vec![
        "1. 工程文件".to_string(),
        "2. 原始素材".to_string(),
        "3. 特别素材".to_string(),
        "4. 转码素材".to_string(),
        "5. 文字素材".to_string(),
        "6. 成片".to_string(),
    ]
}

/// 工况 B 建夹模板：`1. 待分类` + 自定义分类（2..n）+ `n+1. 精选`（含 待修/已修）+ `n+2. 其他`
pub fn workflow_b_folders(categories: &[String]) -> Vec<String> {
    let mut folders = vec!["1. 待分类".to_string()];
    for (i, cat) in categories.iter().enumerate() {
        let name = sanitize_filename(cat);
        folders.push(format!("{}. {}", i + 2, name));
    }
    let n = folders.len(); // 当前已到 n（待分类 + k 个分类 = 1 + k）
    folders.push(format!("{}. 精选", n + 1));
    folders.push(format!("{}. 其他", n + 2));
    folders
}

/// 精选目录内部结构：待修 / 已修
pub fn select_folders() -> Vec<String> {
    vec!["待修".to_string(), "已修".to_string()]
}

/// 推断时段：`MMDD上午/下午`（PRD §5.3 目标命名：`时段_相机编码`）
/// 示例：1 月 1 日上午 -> `0101上午`；下午 -> `0101下午`
pub fn infer_time_slot(dt: &NaiveDateTime) -> String {
    let md = format!("{:02}{:02}", dt.month(), dt.day());
    let half = if dt.hour() < 12 { "上午" } else { "下午" };
    format!("{md}{half}")
}

/// 生成目标子文件夹名：`时段_相机编码`
pub fn time_slot_folder(dt: &NaiveDateTime, cam_code: &str) -> String {
    format!("{}_{}", infer_time_slot(dt), cam_code)
}

/// 成片解析结果
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FinalVideoName {
    pub date: String,
    pub title: String,
    pub resolution: String,
    pub purpose: String,
    pub version: String,
    /// 是否为预览版（720p 或分辨率含 p 且 < 1080）
    pub is_preview: bool,
}

/// 成片命名正则：`时间日期_片名_分辨率_用途_版本`
/// 例：`20260824_活动花絮_4K_交付_V01`、`20260824_花絮_720p_预览_V02`
fn final_video_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{8})_(.+?)_((?:\d{3,4}p)|[248]K|HD|SD|UHD)_(.+?)_(V\d+)$")
            .expect("valid regex")
    })
}

/// 校验成片命名是否符合 `时间日期_片名_分辨率_用途_版本`。
/// 返回 Ok(解析结果) 或 Err(不符合原因)。PRD §5.8。
pub fn validate_final_video_name(name: &str) -> Result<FinalVideoName, String> {
    let stem = name.rsplit('.').next_back().map_or(name, |_| {
        // 去掉扩展名（如 .mp4 / .mov）
        let dot = name.rfind('.');
        match dot {
            Some(i) if i > 0 => &name[..i],
            _ => name,
        }
    });
    let cap = final_video_re()
        .captures(stem)
        .ok_or_else(|| "命名不符合规范：应形如 时间日期_片名_分辨率_用途_版本（如 20260824_花絮_4K_交付_V01）".to_string())?;
    let resolution = cap[3].to_string();
    let is_preview = resolution.ends_with('p')
        && resolution[..resolution.len() - 1]
            .parse::<u32>()
            .map(|v| v < 1080)
            .unwrap_or(false);
    Ok(FinalVideoName {
        date: cap[1].to_string(),
        title: cap[2].to_string(),
        resolution,
        purpose: cap[4].to_string(),
        version: cap[5].to_string(),
        is_preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn sanitize_filters_windows_illegal_chars() {
        // 过滤 < > : " / \ | ? * 后保留其余字符
        assert_eq!(sanitize_filename("a<b>c:d\"e/f\\g|h?i*j"), "abcdefghij");
    }

    #[test]
    fn sanitize_trims_dots_and_spaces() {
        assert_eq!(sanitize_filename("folder.  "), "folder");
        assert_eq!(sanitize_filename("  hello  "), "hello");
    }

    #[test]
    fn sanitize_reserved_names() {
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("con.txt"), "_con.txt");
        assert_eq!(sanitize_filename("NUL"), "_NUL");
        assert_eq!(sanitize_filename("正常文件"), "正常文件");
    }

    #[test]
    fn camera_code_format() {
        assert_eq!(camera_code("DJI Ronin 4D", "B", "ZS"), "DJIRONIN4D_B_ZS");
        // 中文使用者代称会被过滤为空 -> 兜底 DIT
        assert_eq!(camera_code("Sony A7M4", "a", "小李"), "SONYA7M4_A_DIT");
        // 机位缺省 -> 兜底 X
        assert_eq!(camera_code("Sony A7M4", "", "ZS"), "SONYA7M4_X_ZS");
    }

    #[test]
    fn project_id_format() {
        let id = project_id("20260824", "某活动");
        assert_eq!(id, "20260824_某活动");
        // 非法日期兜底为今天
        let id2 = project_id("bad", "x");
        assert_eq!(id2.len(), 8 + 1 + 1);
        assert!(id2.starts_with("2026"));
    }

    #[test]
    fn workflow_a_template_order() {
        let f = workflow_a_folders();
        assert_eq!(f.len(), 6);
        assert_eq!(f[0], "1. 工程文件");
        assert_eq!(f[1], "2. 原始素材");
        assert_eq!(f[2], "3. 特别素材");
        assert_eq!(f[3], "4. 转码素材");
        assert_eq!(f[4], "5. 文字素材");
        assert_eq!(f[5], "6. 成片");
    }

    #[test]
    fn workflow_b_template_with_categories() {
        let f = workflow_b_folders(&["人像".to_string(), "风景".to_string()]);
        assert_eq!(
            f,
            vec![
                "1. 待分类",
                "2. 人像",
                "3. 风景",
                "4. 精选",
                "5. 其他"
            ]
        );
        // 精选含待修/已修
        assert_eq!(select_folders(), vec!["待修", "已修"]);
    }

    #[test]
    fn time_slot_inference() {
        let morning = NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        assert_eq!(infer_time_slot(&morning), "0101上午");
        let afternoon = NaiveDate::from_ymd_opt(2026, 12, 31)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap();
        assert_eq!(infer_time_slot(&afternoon), "1231下午");
        assert_eq!(time_slot_folder(&morning, "DJIRONIN4D_B_ZS"), "0101上午_DJIRONIN4D_B_ZS");
    }

    #[test]
    fn final_video_name_valid() {
        let v = validate_final_video_name("20260824_活动花絮_4K_交付_V01.mp4").unwrap();
        assert_eq!(v.date, "20260824");
        assert_eq!(v.title, "活动花絮");
        assert_eq!(v.resolution, "4K");
        assert_eq!(v.purpose, "交付");
        assert_eq!(v.version, "V01");
        assert!(!v.is_preview);

        let p = validate_final_video_name("20260824_花絮_720p_预览_V02.mov").unwrap();
        assert!(p.is_preview);

        let f = validate_final_video_name("20260824_花絮_1080p_交付_V03.mp4").unwrap();
        assert!(!f.is_preview);
    }

    #[test]
    fn final_video_name_invalid() {
        assert!(validate_final_video_name("随便一个名字.mp4").is_err());
        assert!(validate_final_video_name("20260824_花絮.mp4").is_err());
        assert!(validate_final_video_name("2026_花絮_4K_交付_V01.mp4").is_err());
        assert!(validate_final_video_name("20260824_花絮_4K_交付.mp4").is_err());
    }
}
