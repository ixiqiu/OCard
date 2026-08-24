//! 审计日志 —— 追加式操作日志（PRD §5.9/§6.3）
//!
//! 关键操作（拷卡、校验、删除确认、打包等）追加写入项目 `.ocard/logs/{machine_id}.log`。
//! 多工作站各自写自己的日志文件（以机器 ID 命名），读取时合并所有工作站记录并按时
//! 间排序 —— 无服务端、无文件锁（SMB 跨平台锁不可靠，PRD §6.3）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// 审计日志条目
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditEntry {
    /// RFC3339 时间
    pub timestamp: String,
    /// 工作站机器 ID
    pub machine_id: String,
    /// 操作人（当前登记的 DIT）
    pub operator: String,
    /// 动作：copy / verify / delete / restore / package / transcode / create_project / ...
    pub action: String,
    /// 对象（文件/文件夹/卡）
    pub target: String,
    /// 补充说明（可选）
    pub detail: Option<String>,
}

impl AuditEntry {
    pub fn new(
        machine_id: &str,
        operator: &str,
        action: &str,
        target: &str,
        detail: Option<String>,
    ) -> Self {
        Self {
            timestamp: chrono::Local::now().to_rfc3339(),
            machine_id: machine_id.to_string(),
            operator: operator.to_string(),
            action: action.to_string(),
            target: target.to_string(),
            detail,
        }
    }

    /// JSON Lines 行格式
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// 日志目录布局：`{logs_dir}/{machine_id}.log`（每行一个 JSON）
pub struct AuditLog {
    logs_dir: PathBuf,
    machine_id: String,
}

impl AuditLog {
    pub fn new(logs_dir: &Path, machine_id: &str) -> Self {
        Self {
            logs_dir: logs_dir.to_path_buf(),
            machine_id: machine_id.to_string(),
        }
    }

    /// 追加写入本机日志（append-only，PRD §5.9）
    pub fn append(&self, entry: &AuditEntry) -> std::io::Result<()> {
        fs::create_dir_all(&self.logs_dir)?;
        let path = self.logs_dir.join(format!("{}.log", self.machine_id));
        let mut f = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(f, "{}", entry.to_line())?;
        Ok(())
    }

    /// 便捷追加：自动补 machine_id
    pub fn log(
        &self,
        operator: &str,
        action: &str,
        target: &str,
        detail: Option<String>,
    ) -> std::io::Result<()> {
        self.append(&AuditEntry::new(
            &self.machine_id,
            operator,
            action,
            target,
            detail,
        ))
    }

    /// 读取本机日志（按追加顺序）
    pub fn read_own(&self) -> Vec<AuditEntry> {
        let path = self.logs_dir.join(format!("{}.log", self.machine_id));
        match fs::File::open(path) {
            Ok(f) => parse_lines(BufReader::new(f)),
            Err(_) => Vec::new(),
        }
    }

    /// 合并所有工作站的日志并按时间排序（PRD §6.3：读取时合并所有工作站的记录）
    pub fn read_all(&self) -> Vec<AuditEntry> {
        let mut entries = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.logs_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("log") {
                    if let Ok(f) = fs::File::open(&path) {
                        entries.extend(parse_lines(BufReader::new(f)));
                    }
                }
            }
        }
        entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        entries
    }

    /// 列出已登记的工作站（机器 ID 集合）
    pub fn known_machines(&self) -> Vec<String> {
        let mut set = BTreeMap::new();
        for e in self.read_all() {
            set.insert(e.machine_id.clone(), ());
        }
        set.keys().cloned().collect()
    }
}

fn parse_lines<R: BufRead>(reader: R) -> Vec<AuditEntry> {
    reader
        .lines()
        .filter_map(|l| l.ok())
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<AuditEntry>(&l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read_own() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::new(dir.path(), "ws-001");
        log.log("张三", "copy", "CARD-001", Some("2. 原始素材".to_string()))
            .unwrap();
        log.log("张三", "verify", "CARD-001", None).unwrap();
        let entries = log.read_own();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].action, "copy");
        assert_eq!(entries[0].operator, "张三");
        assert_eq!(entries[1].action, "verify");
    }

    #[test]
    fn merge_multiple_workstations_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let log_a = AuditLog::new(dir.path(), "ws-a");
        let log_b = AuditLog::new(dir.path(), "ws-b");
        log_a.log("张三", "copy", "CARD-001", None).unwrap();
        // 手工插入一个更晚时间戳的 b 记录
        log_b
            .append(&AuditEntry {
                timestamp: "2099-01-01T00:00:00+00:00".to_string(),
                machine_id: "ws-b".to_string(),
                operator: "李四".to_string(),
                action: "package".to_string(),
                target: "精选".to_string(),
                detail: None,
            })
            .unwrap();
        let all = log_a.read_all();
        assert_eq!(all.len(), 2);
        // 按时间升序：a 的记录在前
        assert_eq!(all[0].machine_id, "ws-a");
        assert_eq!(all[1].machine_id, "ws-b");
        let machines = log_a.known_machines();
        assert!(machines.contains(&"ws-a".to_string()));
        assert!(machines.contains(&"ws-b".to_string()));
    }

    #[test]
    fn missing_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::new(&dir.path().join("nope"), "ws-x");
        assert!(log.read_own().is_empty());
        assert!(log.read_all().is_empty());
    }

    #[test]
    fn malformed_lines_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::new(dir.path(), "ws-001");
        let path = dir.path().join("ws-001.log");
        fs::write(
            &path,
            "{\"timestamp\":\"t\",\"machine_id\":\"ws-001\",\"operator\":\"o\",\"action\":\"a\",\"target\":\"x\",\"detail\":null}\nnot-json\n",
        )
        .unwrap();
        let entries = log.read_own();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action, "a");
    }
}
