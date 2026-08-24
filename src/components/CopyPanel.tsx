import { useCallback, useEffect, useState } from "react";
import {
  fmtBytes,
  listDevices,
  listManifests,
  runCopy,
  scanVolumes,
} from "../api";
import type {
  AppConfig,
  CopyTaskReport,
  DeviceRegistry,
  Manifest,
  Project,
  Volume,
} from "../types";

interface Props {
  projectId: string;
  workflow: string;
  project: Project;
  config: AppConfig | null;
}

export default function CopyPanel({ projectId, workflow, project, config }: Props) {
  const [volumes, setVolumes] = useState<Volume[]>([]);
  const [devices, setDevices] = useState<DeviceRegistry | null>(null);
  const [manifests, setManifests] = useState<Manifest[]>([]);
  const [volume, setVolume] = useState("");
  const [cameraCode, setCameraCode] = useState("");
  const [cardLabel, setCardLabel] = useState("");
  const [note, setNote] = useState("");
  const [destinations, setDestinations] = useState<string[]>([]);
  const [resume, setResume] = useState(false);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<CopyTaskReport | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState(false);

  const load = useCallback(() => {
    scanVolumes().then(setVolumes).catch((e) => setErr(String(e)));
    listDevices(projectId).then(setDevices).catch((e) => setErr(String(e)));
    listManifests(projectId).then(setManifests).catch(() => {});
  }, [projectId]);

  useEffect(load, [load]);

  // 默认目的地：按工况自动生成（NAS 主 + 本地备份占位）
  useEffect(() => {
    const cam = cameraCode || "CAM_X";
    const date = project.date || new Date().toISOString().slice(0, 10).replace(/-/g, "");
    const sub = workflow === "A" ? `2. 原始素材/${date}_${cam}` : `1. 待分类/0000上午_${cam}`;
    setDestinations((prev) => (prev.length ? prev : [sub, `备份/${date}_${cam}`]));
  }, [workflow, cameraCode, project.date]);

  async function startCopy() {
    setErr(null);
    setReport(null);
    setBusy(true);
    try {
      const r = await runCopy(
        projectId,
        volume,
        destinations.filter(Boolean),
        cardLabel || "CARD-未登记",
        cameraCode || "CAM_X",
        note,
        resume
      );
      setReport(r);
      load();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  const destPreview = destinations
    .filter(Boolean)
    .map((d) => (config?.nas_root ? `${config.nas_root}/${projectId}/${d}` : `${projectId}/${d}`));

  return (
    <div>
      <div className="notice info">
        拷卡流程（PRD §5.3）：插卡 → 双确认（选相机自动带出编码 + 内容备注）→ 目标路径按规范生成
        → 流式拷贝 + xxHash3-64 校验 → 多目的地并行 → 回读校验 → manifest 落盘 → 提示可格式化。
      </div>

      <div className="card">
        <h3>① 源卷（自动检测可移动卷）</h3>
        <div className="row">
          <select value={volume} onChange={(e) => setVolume(e.target.value)} style={{ flex: 1 }}>
            <option value="">— 选择插入的存储卡 —</option>
            {volumes.map((v) => (
              <option key={v.mount_point} value={v.mount_point}>
                {v.name} · {v.mount_point}（{fmtBytes(v.available_bytes)} 可用）
              </option>
            ))}
          </select>
          <button className="btn ghost" onClick={load}>
            ↻ 重新检测
          </button>
        </div>
        {volumes.length === 0 && (
          <p className="muted" style={{ marginTop: 8 }}>
            未检测到可移动卷。请插入存储卡后点击「重新检测」。
          </p>
        )}
      </div>

      <div className="card">
        <h3>② 双确认：相机 / 存储卡 / 备注</h3>
        <div className="row">
          <div style={{ flex: 1 }}>
            <label>相机（自动带出规范编码，如 DJIRonin4D_B_ZS）</label>
            <select
              value={cameraCode}
              onChange={(e) => setCameraCode(e.target.value)}
            >
              <option value="">— 选择相机 —</option>
              {devices?.cameras.map((c) => (
                <option key={c.id} value={c.code}>
                  {c.code}（{c.model}）
                </option>
              ))}
            </select>
          </div>
          <div style={{ flex: 1 }}>
            <label>存储卡标签</label>
            <select value={cardLabel} onChange={(e) => setCardLabel(e.target.value)}>
              <option value="">— 选择/填写 —</option>
              {devices?.cards.map((c) => (
                <option key={c.id} value={c.label}>
                  {c.label}
                </option>
              ))}
              {cardLabel && !devices?.cards.some((c) => c.label === cardLabel) && (
                <option value={cardLabel}>{cardLabel}（未登记）</option>
              )}
            </select>
          </div>
        </div>
        <label>内容备注（对应「摄影师和 DIT 两方确认」+「适当记录」）</label>
        <textarea
          rows={2}
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="如：上午场花絮，双卡备份"
        />
      </div>

      <div className="card">
        <h3>③ 多目的地（NAS 主 + 本地/移动硬盘备）</h3>
        <label>目的地（相对项目根，可多个，一次读源并行写）</label>
        {destinations.map((d, i) => (
          <div key={i} className="row" style={{ marginBottom: 8 }}>
            <input
              style={{ flex: 1 }}
              value={d}
              onChange={(e) =>
                setDestinations((prev) =>
                  prev.map((x, j) => (j === i ? e.target.value : x))
                )
              }
            />
            {destinations.length > 1 && (
              <button
                className="btn ghost small"
                onClick={() => setDestinations((prev) => prev.filter((_, j) => j !== i))}
              >
                删
              </button>
            )}
          </div>
        ))}
        <div className="row">
          <button
            className="btn ghost small"
            onClick={() => setDestinations((prev) => [...prev, ""])}
          >
            ＋ 添加目的地
          </button>
          <label className="row" style={{ margin: 0 }}>
            <input
              type="checkbox"
              style={{ width: "auto", margin: 0 }}
              checked={resume}
              onChange={(e) => setResume(e.target.checked)}
            />
            断点续传（跳过已校验文件）
          </label>
        </div>
        <div className="muted" style={{ marginTop: 8, fontSize: 12 }}>
          将写入：
          {destPreview.map((d, i) => (
            <div key={i}>· {d}</div>
          ))}
        </div>
      </div>

      <div className="card">
        <h3>④ 开始拷卡（双确认）</h3>
        <label>
          <input
            type="checkbox"
            style={{ width: "auto" }}
            checked={confirmed}
            onChange={(e) => setConfirmed(e.target.checked)}
          />
          我已确认：源卷为 {volume || "待选"}，相机 {cameraCode || "待选"}，目的地 {destinations.filter(Boolean).length} 个。
        </label>
        {err && <div className="notice err">{err}</div>}
        <button className="btn ok" disabled={busy || !confirmed || !volume} onClick={startCopy}>
          {busy ? "拷卡中…（请勿拔出存储卡）" : "开始拷卡"}
        </button>
      </div>

      {report && (
        <div className="card">
          <h3>拷卡报告</h3>
          {report.all_verified ? (
            <div className="notice ok">
              ✅ 全部校验通过（{report.verified_count}/{report.verified_count}），本卡可格式化。
            </div>
          ) : (
            <div className="notice warn">
              ⚠️ 校验未全部通过：成功 {report.verified_count}，失败 {report.failed_count}。可逐文件重试。
            </div>
          )}
          <p className="muted">
            拷贝 {report.copied_count} 个 · 跳过 {report.skipped_count} 个 · 共 {fmtBytes(report.total_bytes)} · 耗时{" "}
            {report.duration_secs.toFixed(1)}s
          </p>
          {report.outcomes.filter((o) => o.errors.length > 0).length > 0 && (
            <div className="notice err">
              失败文件：
              {report.outcomes
                .filter((o) => o.errors.length > 0)
                .slice(0, 5)
                .map((o) => (
                  <div key={o.rel_path}>
                    · {o.rel_path}: {o.errors[0]}
                  </div>
                ))}
            </div>
          )}
        </div>
      )}

      <div className="card">
        <h3>历史 manifest</h3>
        {manifests.length === 0 ? (
          <p className="muted">暂无拷卡记录</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th>时间</th>
                <th>卡</th>
                <th>相机</th>
                <th>操作人</th>
                <th>文件</th>
                <th>校验</th>
              </tr>
            </thead>
            <tbody>
              {manifests.map((m, i) => {
                const vc = m.files.filter((f) => f.verified).length;
                const allOk = m.files.length > 0 && vc === m.files.length;
                return (
                  <tr key={i}>
                    <td>{m.started_at}</td>
                    <td>{m.card_label}</td>
                    <td>{m.camera_code}</td>
                    <td>{m.operator}</td>
                    <td>{m.files.length}</td>
                    <td>
                      {allOk ? (
                        <span className="badge ok">✓ 100%</span>
                      ) : (
                        <span className="badge warn">
                          {vc}/{m.files.length}
                        </span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
