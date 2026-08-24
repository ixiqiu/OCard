import { useEffect, useState } from "react";
import { getConfig, saveConfig } from "../api";
import type { AppConfig } from "../types";

interface Props {
  config: AppConfig | null;
  onSaved: (c: AppConfig) => void;
}

export default function SettingsPage({ config, onSaved }: Props) {
  const [nasRoot, setNasRoot] = useState("");
  const [operator, setOperator] = useState("");
  const [machineId, setMachineId] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (config) {
      setNasRoot(config.nas_root);
      setOperator(config.operator);
      setMachineId(config.machine_id);
    }
  }, [config]);

  async function save() {
    setErr(null);
    setMsg(null);
    try {
      const c = await saveConfig(nasRoot, operator);
      onSaved(c);
      setMsg("设置已保存。NAS 根路径按工作站各自配置，项目状态存于 NAS 项目夹 .ocard/（PRD §6.3/§6.5）。");
    } catch (e) {
      setErr(String(e));
    }
  }

  return (
    <div>
      <div className="page-title">
        ⚙️ 设置
        <span className="hint">工作站配置：NAS 根路径 / 操作人 / 机器 ID</span>
      </div>

      <div className="card">
        <h3>工作站</h3>
        <div className="field">
          <label>NAS 根路径（挂载点 / 盘符 / UNC，如 /Volumes/NAS、Z:\NAS、\\server\nas）</label>
          <input value={nasRoot} onChange={(e) => setNasRoot(e.target.value)} placeholder="/Volumes/NAS" />
        </div>
        <div className="field">
          <label>当前登记的 DIT 操作人</label>
          <input value={operator} onChange={(e) => setOperator(e.target.value)} placeholder="张三" />
        </div>
        <div className="field">
          <label>机器 ID（自动生成，用于多工作站日志区分）</label>
          <input value={machineId} readOnly />
        </div>
        {msg && <div className="notice ok">{msg}</div>}
        {err && <div className="notice err">{err}</div>}
        <button className="btn ok" onClick={save}>
          保存设置
        </button>
      </div>

      <div className="card">
        <h3>关于 OCard</h3>
        <p className="muted" style={{ lineHeight: 1.8 }}>
          跨平台 DIT 素材备份管理工具（Tauri 2 · Rust + React）。
          <br />
          · 拷卡：xxHash3-64 校验、多目的地并行、断点续传、manifest 落盘
          <br />
          · 分类：键盘流（数字键 / P 精选 / O 其他 / D 回收站），连拍自动折叠
          <br />
          · 转码：ffmpeg sidecar，NVENC / QSV / AMF / VideoToolbox / VAAPI 自动探测
          <br />
          · 交付：按半天分包 + 交付清单，人工上传百度网盘后勾选状态
          <br />
          · AI 只排序和建议，不自动删除/挪动任何文件（PRD §5.5）
        </p>
      </div>
    </div>
  );
}
