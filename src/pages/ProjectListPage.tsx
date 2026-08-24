import { useState } from "react";
import { createProject, fmtBytes } from "../api";
import type { ProjectSummary } from "../types";

interface Props {
  projects: ProjectSummary[];
  onRefresh: () => void;
  onOpen: (id: string) => void;
  configReady: boolean;
}

export default function ProjectListPage({ projects, onRefresh, onOpen, configReady }: Props) {
  const [showWizard, setShowWizard] = useState(false);
  const [date, setDate] = useState(today());
  const [name, setName] = useState("");
  const [workflow, setWorkflow] = useState<"A" | "B">("A");
  const [categories, setCategories] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  function today() {
    const d = new Date();
    return `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, "0")}${String(
      d.getDate()
    ).padStart(2, "0")}`;
  }

  async function submit() {
    setBusy(true);
    setErr(null);
    try {
      const cats = categories
        .split(/[,，]/)
        .map((s) => s.trim())
        .filter(Boolean);
      await createProject(date, name, workflow, cats);
      setShowWizard(false);
      setName("");
      setCategories("");
      onRefresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <div className="page-title">
        项目列表
        <span className="hint">
          新建项目向导：选日期 + 项目名 → 生成 YYYYMMDD_项目名 项目文件夹于 NAS（PRD §5.2）
        </span>
      </div>

      {!configReady && (
        <div className="notice warn">
          尚未配置 NAS 根路径。请先前往「设置」登记 NAS 挂载路径与操作人。
        </div>
      )}

      <div className="row" style={{ marginBottom: 16 }}>
        <button className="btn" disabled={!configReady} onClick={() => setShowWizard(!showWizard)}>
          {showWizard ? "取消新建" : "＋ 新建项目"}
        </button>
        <button className="btn ghost" onClick={onRefresh}>
          ↻ 刷新
        </button>
      </div>

      {showWizard && (
        <div className="card">
          <h3>新建项目向导</h3>
          <div className="field">
            <label>日期（YYYYMMDD）</label>
            <input value={date} onChange={(e) => setDate(e.target.value)} />
          </div>
          <div className="field">
            <label>项目名</label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="如：某活动"
            />
          </div>
          <div className="field">
            <label>工况</label>
            <select
              value={workflow}
              onChange={(e) => setWorkflow(e.target.value as "A" | "B")}
            >
              <option value="A">工况 A（视频剪辑）</option>
              <option value="B">工况 B（纯拍照）</option>
            </select>
          </div>
          {workflow === "B" && (
            <div className="field">
              <label>自定义分类（逗号分隔，如：人像,风景）</label>
              <input
                value={categories}
                onChange={(e) => setCategories(e.target.value)}
                placeholder="人像, 风景"
              />
            </div>
          )}
          {err && <div className="notice err">{err}</div>}
          <button className="btn ok" disabled={busy || !name} onClick={submit}>
            {busy ? "创建中…" : "创建项目"}
          </button>
        </div>
      )}

      {projects.length === 0 ? (
        <div className="notice info">暂无项目，点击「新建项目」开始。</div>
      ) : (
        <div className="card" style={{ padding: 0 }}>
          <table>
            <thead>
              <tr>
                <th>项目</th>
                <th>工况</th>
                <th>已拷卡</th>
                <th>分类进度</th>
                <th>备份</th>
                <th>交付</th>
              </tr>
            </thead>
            <tbody>
              {projects.map((p) => (
                <tr key={p.id} style={{ cursor: "pointer" }} onClick={() => onOpen(p.id)}>
                  <td>
                    <b>{p.id}</b>
                  </td>
                  <td>{p.workflow === "A" ? "A · 视频" : "B · 拍照"}</td>
                  <td>
                    {p.copied_files} 张 / {fmtBytes(p.copied_bytes)}
                  </td>
                  <td>
                    {p.workflow === "B"
                      ? `${p.classified} 已分类 / ${p.unclassified} 待分类`
                      : "—"}
                  </td>
                  <td>
                    {p.backup_ok ? (
                      <span className="badge ok">✓ 已备份</span>
                    ) : (
                      <span className="badge dim">未备份</span>
                    )}
                  </td>
                  <td>
                    {p.packaged ? (
                      <span className="badge ok">✓ 已打包</span>
                    ) : (
                      <span className="badge dim">未打包</span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
