//! 设备与存储卡登记（PRD §5.1）
//!
//! 登记相机：型号、机位（A–Z）、使用者代称，自动生成规范编码（如 DJIRonin4D_B_ZS）。
//! 登记存储卡并与相机关联（一卡一机）。登记表全项目共享，存于 `.ocard/devices.json`。

use crate::naming;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Camera {
    pub id: String,
    pub model: String,
    /// 机位 A–Z
    pub position: String,
    /// 使用者代称（如 ZS）
    pub operator: String,
    /// 自动生成的规范编码，如 DJIRonin4D_B_ZS
    pub code: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Card {
    pub id: String,
    /// 辨识标签（打印/显示用），如 CARD-001
    pub label: String,
    pub camera_id: String,
    pub note: Option<String>,
}

/// 全项目共享的设备登记表
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DeviceRegistry {
    pub cameras: Vec<Camera>,
    pub cards: Vec<Card>,
}

impl DeviceRegistry {
    /// 登记表路径：`.ocard/devices.json`
    pub fn path(ocard_dir: &Path) -> PathBuf {
        ocard_dir.join("devices.json")
    }

    pub fn load(ocard_dir: &Path) -> Self {
        let p = Self::path(ocard_dir);
        match fs::read_to_string(&p) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, ocard_dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(ocard_dir)?;
        let p = Self::path(ocard_dir);
        let json = serde_json::to_string_pretty(self)?;
        fs::write(p, json)
    }

    /// 新增相机，自动生成编码。position 只允许 A–Z（单个字母）。
    pub fn add_camera(
        &mut self,
        model: &str,
        position: &str,
        operator: &str,
        note: Option<String>,
    ) -> Result<Camera, String> {
        let pos = position.trim().to_uppercase();
        if pos.len() != 1 || !pos.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err("机位必须是单个字母 A–Z".to_string());
        }
        let model_clean = model.trim();
        if model_clean.is_empty() {
            return Err("型号不能为空".to_string());
        }
        let op_clean = operator.trim();
        if op_clean.is_empty() {
            return Err("使用者代称不能为空".to_string());
        }
        let code = naming::camera_code(model_clean, &pos, op_clean);
        if self.cameras.iter().any(|c| c.code == code) {
            return Err(format!("相机编码已存在：{code}"));
        }
        let camera = Camera {
            id: format!("cam-{}", self.cameras.len() + 1),
            model: model_clean.to_string(),
            position: pos,
            operator: op_clean.to_string(),
            code,
            note,
        };
        self.cameras.push(camera.clone());
        Ok(camera)
    }

    /// 新增存储卡（一卡一机）
    pub fn add_card(
        &mut self,
        label: &str,
        camera_id: &str,
        note: Option<String>,
    ) -> Result<Card, String> {
        let label = label.trim();
        if label.is_empty() {
            return Err("标签不能为空".to_string());
        }
        if !self.cameras.iter().any(|c| c.id == camera_id) {
            return Err("关联相机不存在".to_string());
        }
        if self.cards.iter().any(|c| c.label == label) {
            return Err(format!("存储卡标签已存在：{label}"));
        }
        let card = Card {
            id: format!("card-{}", self.cards.len() + 1),
            label: label.to_string(),
            camera_id: camera_id.to_string(),
            note,
        };
        self.cards.push(card.clone());
        Ok(card)
    }

    /// 按相机编码查相机
    pub fn camera_by_code(&self, code: &str) -> Option<&Camera> {
        self.cameras.iter().find(|c| c.code == code)
    }

    /// 按卡标签查卡
    pub fn card_by_label(&self, label: &str) -> Option<&Card> {
        self.cards.iter().find(|c| c.label == label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn add_camera_auto_code() {
        let mut reg = DeviceRegistry::default();
        let cam = reg
            .add_camera("DJI Ronin 4D", "b", "ZS", None)
            .unwrap();
        assert_eq!(cam.code, "DJIRONIN4D_B_ZS");
        assert_eq!(reg.cameras.len(), 1);
    }

    #[test]
    fn add_camera_rejects_bad_position() {
        let mut reg = DeviceRegistry::default();
        assert!(reg.add_camera("X", "AB", "ZS", None).is_err());
        assert!(reg.add_camera("X", "1", "ZS", None).is_err());
        assert!(reg.add_camera("", "A", "ZS", None).is_err());
    }

    #[test]
    fn add_camera_rejects_duplicate_code() {
        let mut reg = DeviceRegistry::default();
        reg.add_camera("Sony A7M4", "A", "ZS", None).unwrap();
        let r = reg.add_camera("Sony A7M4", "a", "ZS", None);
        assert!(r.is_err());
    }

    #[test]
    fn card_linked_to_camera() {
        let mut reg = DeviceRegistry::default();
        let cam = reg.add_camera("Sony A7M4", "A", "ZS", None).unwrap();
        let card = reg.add_card("CARD-001", &cam.id, None).unwrap();
        assert_eq!(card.camera_id, cam.id);
        assert_eq!(reg.card_by_label("CARD-001").unwrap().label, "CARD-001");
    }

    #[test]
    fn card_rejects_unknown_camera_and_duplicate() {
        let mut reg = DeviceRegistry::default();
        assert!(reg.add_card("CARD-001", "nope", None).is_err());
        let cam = reg.add_camera("Sony A7M4", "A", "ZS", None).unwrap();
        reg.add_card("CARD-001", &cam.id, None).unwrap();
        assert!(reg.add_card("CARD-001", &cam.id, None).is_err());
    }

    #[test]
    fn registry_persists() {
        let tmp = dir();
        let mut reg = DeviceRegistry::default();
        reg.add_camera("Canon R5", "C", "WY", None).unwrap();
        reg.save(tmp.path()).unwrap();
        let loaded = DeviceRegistry::load(tmp.path());
        assert_eq!(loaded, reg);
        assert_eq!(loaded.cameras[0].code, "CANONR5_C_WY");
    }

    #[test]
    fn load_missing_returns_default() {
        let tmp = dir();
        let reg = DeviceRegistry::load(tmp.path());
        assert!(reg.cameras.is_empty());
    }
}
