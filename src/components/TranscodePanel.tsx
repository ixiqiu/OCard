import { useCallback, useEffect, useState } from "react";
import {
  addTranscodeJob,
  listTranscodeJobs,
  probeFfmpeg,
  runTranscodeJob,
} from "../api";
import type { EncoderInfo, JobKind, QualityPreset, TranscodeQueue } from "../types";

interface Props {
  projectId: string;
}

export default function TranscodePanel({ projectId }: Props) {
  const [encoders, setEncoders] = useState<EncoderInfo | null>(null);
  const [queue, setQueue] = useState<TranscodeQueue | null>(null);
  const [inputPath, setInputPath] = useState("");
  const [outputDir, setOutputDir] = useState("4. 转码素材");
  const [kind, setKind] = useState<"proxy" | "archive">("proxy");
  const [preset, setPreset] = useState<QualityPreset>("balanced");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(() => {
    probeFfmpeg().then(setEncoders).catch(() => {});
    listTranscodeJobs(projectId).then(setQueue).catch((e) => setErr(String(e)));
  }, [projectId]);

  useEffect(load, [load]);

  async function addJob() {
    setErr(null);
    setMsg(null);
    setBusy(true);
    try {
      const k: JobKind = kind === "proxy" ? { type: "proxy", preset } : { type: "archive", preset };
      const job = await addTranscodeJob(projectId, inputPath, outputDir, k);
      setMsg(`已加入任务 ${job.id}`);
      setInputPath("");
      load();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function runJob(id: string) {
    setErr(null);
    setBusy(true);
    try {
      await runTranscodeJob(projectId, id);
      load();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  const presetLabel: Record<QualityPreset, string> = {
    high: "高质量（CRF 18）",
    balanced: "平衡（CRF 23）",
    compact: "高压缩（CRF 28）",
  };

  return (
    <div>
      <div className="notice info">
        转码引擎（PRD §5.6）：ffmpeg sidecar 按平台探测硬件编码器 —— NVENC / QSV / AMF /
        VideoToolbox / VAAPI，全部不可用回落 x264/x265 软编。代理转码入「4. 转码素材」，归档可选
        HEVC 10-bit 三档压缩，默认保留原文件。
      </div>

      <div className="card">
        <h3>ffmpeg 探测</h3>
        {!encoders ? (
          <p className="muted">探测中…</p>
        ) : !encoders.ffmpeg_available ? (
          <div className="notice warn">未找到 ffmpeg。请安装 ffmpeg 并确保在 PATH 中。</div>
        ) : (
          <div className="row">
            <span className="badge ok">ffmpeg 可用</span>
            {encoders.nvenc && <span className="badge ok">NVENC</span>}
            {encoders.qsv && <span className="badge ok">QSV</span>}
            {encoders.amf && <span className="badge ok">AMF</span>}
            {encoders.videotoolbox && <span className="badge ok">VideoToolbox</span>}
            {encoders.vaapi && <span className="badge ok">VAAPI</span>}
            {!encoders.nvenc && !encoders.qsv && !encoders.amf && !encoders.videotoolbox && !encoders.vaapi && (
              <span className="badge warn">无硬件编码器，将回落软编</span>
            )}
          </div>
        )}
      </div>

      <div className="card">
        <h3>添加转码任务</h3>
        <div className="field">
          <label>输入文件（绝对路径）</label>
          <input value={inputPath} onChange={(e) => setInputPath(e.target.value)} placeholder="/Volumes/NAS/项目/2. 原始素材/xxx.mov" />
        </div>
        <div className="field">
          <label>输出目录（相对项目根）</label>
          <input value={outputDir} onChange={(e) => setOutputDir(e.target.value)} />
        </div>
        <div className="row">
          <div style={{ flex: 1 }}>
            <label>任务类型</label>
            <select value={kind} onChange={(e) => setKind(e.target.value as "proxy" | "archive")}>
              <option value="proxy">代理转码（h264，剪辑用）</option>
              <option value="archive">归档转码（HEVC 10-bit）</option>
            </select>
          </div>
          <div style={{ flex: 1 }}>
            <label>档位</label>
            <select value={preset} onChange={(e) => setPreset(e.target.value as QualityPreset)}>
              <option value="high">{presetLabel.high}</option>
              <option value="balanced">{presetLabel.balanced}</option>
              <option value="compact">{presetLabel.compact}</option>
            </select>
          </div>
        </div>
        {err && <div className="notice err">{err}</div>}
        {msg && <div className="notice ok">{msg}</div>}
        <button className="btn ok" disabled={busy || !inputPath} onClick={addJob}>
          加入队列
        </button>
      </div>

      <div className="card">
        <h3>转码队列</h3>
        {!queue || queue.jobs.length === 0 ? (
          <p className="muted">暂无任务</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th>ID</th>
                <th>输入</th>
                <th>类型</th>
                <th>状态</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              {queue.jobs.map((j) => (
                <tr key={j.id}>
                  <td>{j.id}</td>
                  <td style={{ maxWidth: 300, overflow: "hidden", textOverflow: "ellipsis" }}>
                    {j.input}
                  </td>
                  <td>
                    {j.kind.type === "proxy" ? `代理 · ${presetLabel[j.kind.preset]}` : `归档 · ${presetLabel[j.kind.preset]}`}
                  </td>
                  <td>
                    {j.status === "done" && <span className="badge ok">完成</span>}
                    {j.status === "running" && <span className="badge warn">运行中</span>}
                    {j.status === "failed" && (
                      <span className="badge err" title={j.error ?? ""}>
                        失败
                      </span>
                    )}
                    {j.status === "pending" && <span className="badge dim">排队</span>}
                  </td>
                  <td>
                    {j.status === "pending" && (
                      <button className="btn small" disabled={busy} onClick={() => runJob(j.id)}>
                        执行
                      </button>
                    )}
                    {j.status === "failed" && (
                      <button className="btn small" disabled={busy} onClick={() => runJob(j.id)}>
                        重试
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
