import { useCallback, useEffect, useState } from "react";
import {
  fmtBytes,
  listPackageInputs,
  runPackaging,
  uploadList,
} from "../api";
import type { DeliveryManifest, PackageInput } from "../types";

interface Props {
  projectId: string;
}

export default function PackagingPanel({ projectId }: Props) {
  const [folder, setFolder] = useState("精选");
  const [inputs, setInputs] = useState<PackageInput[]>([]);
  const [manifest, setManifest] = useState<DeliveryManifest | null>(null);
  const [uploadText, setUploadText] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => {
    listPackageInputs(projectId, folder)
      .then(setInputs)
      .catch((e) => setErr(String(e)));
    uploadList(projectId).then(setUploadText).catch(() => setUploadText(""));
  }, [projectId, folder]);

  useEffect(load, [load]);

  async function doPack() {
    setErr(null);
    setBusy(true);
    try {
      const m = await runPackaging(projectId, folder);
      setManifest(m);
      setUploadText(await uploadList(projectId));
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <div className="notice info">
        交付打包（PRD §5.7）：选择已分类文件夹 → 按半天自动分包（依素材拍摄时间），包含精选与其他，
        不压缩，生成包文件夹 + 交付清单。打包完成给出待上传列表，人工上传百度网盘、人工发链接；
        OCard 记录交付状态（已打包/已上传可手动勾）。
      </div>

      <div className="card">
        <h3>选择分类文件夹</h3>
        <div className="row">
          <select value={folder} onChange={(e) => setFolder(e.target.value)} style={{ flex: 1 }}>
            <option value="精选">精选</option>
            <option value="其他">其他</option>
            {manifest?.packages.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name}
              </option>
            ))}
          </select>
          <button className="btn ghost" onClick={load}>
            ↻ 刷新
          </button>
        </div>
        <p className="muted" style={{ marginTop: 8 }}>
          当前 {inputs.length} 个文件，共 {fmtBytes(inputs.reduce((a, b) => a + b.size, 0))}
        </p>
        {err && <div className="notice err">{err}</div>}
        <button className="btn ok" disabled={busy || inputs.length === 0} onClick={doPack}>
          {busy ? "打包中…" : "开始打包"}
        </button>
      </div>

      {manifest && (
        <div className="card">
          <h3>交付清单（{manifest.project_id}）</h3>
          <p className="muted">
            {manifest.operator} · {manifest.created_at} · {manifest.packages.length} 个包 /{" "}
            {manifest.total_files} 个文件 / {fmtBytes(manifest.total_bytes)}
          </p>
          <table>
            <thead>
              <tr>
                <th>包</th>
                <th>张数</th>
                <th>容量</th>
              </tr>
            </thead>
            <tbody>
              {manifest.packages.map((p) => (
                <tr key={p.name}>
                  <td>{p.name}</td>
                  <td>{p.count}</td>
                  <td>{fmtBytes(p.total_bytes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="notice ok" style={{ marginTop: 12 }}>
            ✅ 已打包。请人工上传百度网盘、人工发送分享链接，上传完成后手动勾选交付状态。
          </div>
        </div>
      )}

      {uploadText && (
        <div className="card">
          <h3>待上传列表（人工上传用）</h3>
          <pre className="dump">{uploadText}</pre>
        </div>
      )}
    </div>
  );
}
