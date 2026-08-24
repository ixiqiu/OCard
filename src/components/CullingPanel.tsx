import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  analyzeFolder,
  burstGroups,
  fmtBytes,
  makeThumbnail,
  moveToCategory,
  moveToTrash,
  scanMediaFolder,
  toFileUrl,
} from "../api";
import type { AnalyzeResult, BurstGroup, MediaItem, Project } from "../types";

interface Props {
  projectId: string;
  project: Project;
}

const CATEGORY_COLORS = ["#4098ff", "#3ecf8e", "#f2c94c", "#f59e0b", "#ef5350", "#ab47bc", "#26c6da", "#8d6e63", "#78909c"];

export default function CullingPanel({ projectId, project }: Props) {
  const [items, setItems] = useState<MediaItem[]>([]);
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const [analysis, setAnalysis] = useState<Record<string, AnalyzeResult>>({});
  const [groups, setGroups] = useState<BurstGroup[]>([]);
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const [selected, setSelected] = useState(0);
  const [trashPending, setTrashPending] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [showTrashConfirm, setShowTrashConfirm] = useState(false);
  const gridRef = useRef<HTMLDivElement>(null);

  const FOLDER = "1. 待分类";
  const cats = project.categories;

  const load = useCallback(async () => {
    setErr(null);
    setBusy(true);
    try {
      const media = await scanMediaFolder(projectId, FOLDER);
      setItems(media);
      setSelected(0);
      // 缩略图（缓存）
      const t: Record<string, string> = {};
      await Promise.all(
        media.slice(0, 200).map(async (m) => {
          try {
            t[m.path] = await makeThumbnail(projectId, m.path);
          } catch {
            /* 视频等无法解码的跳过 */
          }
        })
      );
      setThumbs(t);
      // AI 分析（客观指标）
      try {
        const res = await analyzeFolder(projectId, FOLDER);
        const map: Record<string, AnalyzeResult> = {};
        res.forEach((r) => (map[r.path] = r));
        setAnalysis(map);
      } catch {
        /* 分析失败不影响浏览 */
      }
      // 连拍组
      try {
        setGroups(await burstGroups(projectId, FOLDER));
      } catch {
        setGroups([]);
      }
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, [projectId]);

  useEffect(() => {
    load();
  }, [load]);

  // 折叠组内成员（只显示保留项）
  const visibleItems = useMemo(() => {
    const hidden = new Set<string>();
    groups.forEach((g) => {
      if (collapsed.has(g.id)) {
        g.members.forEach((m) => {
          if (!g.keep.includes(m)) hidden.add(m);
        });
      }
    });
    return items.filter((m) => !hidden.has(m.path));
  }, [items, groups, collapsed]);

  const visibleIndex = useMemo(() => {
    return visibleItems.findIndex((m) => m.path === items[selected]?.path);
  }, [visibleItems, items, selected]);

  // 键盘流：数字键分类、P 精选、O 其他、D 标记删除、方向键选择
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      const current = visibleItems[Math.max(0, visibleIndex)];
      if (!current) return;
      const path = current.path;

      const doAction = (fn: () => Promise<unknown>, tip: string) => {
        fn().then(() => {
          setMsg(`${tip}: ${current.name}`);
          setTimeout(() => setMsg(null), 1500);
          load();
        });
      };

      if (e.key >= "1" && e.key <= "9") {
        const idx = Number(e.key) - 1;
        if (idx < cats.length) {
          doAction(() => moveToCategory(projectId, path, cats[idx], false), `已分到「${cats[idx]}」`);
        }
      } else if (e.key === "p" || e.key === "P") {
        doAction(() => moveToCategory(projectId, path, "精选", true), "已标精选（复制到 精选/待修）");
      } else if (e.key === "o" || e.key === "O") {
        doAction(() => moveToCategory(projectId, path, "其他", false), "已移到「其他」");
      } else if (e.key === "d" || e.key === "D") {
        // 两段式：先标记
        setTrashPending((prev) => {
          const next = new Set(prev);
          if (next.has(path)) next.delete(path);
          else next.add(path);
          return next;
        });
      } else if (e.key === "ArrowRight") {
        setSelected((s) => Math.min(items.length - 1, s + 1));
      } else if (e.key === "ArrowLeft") {
        setSelected((s) => Math.max(0, s - 1));
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [visibleItems, visibleIndex, items.length, cats, projectId, load]);

  async function confirmTrash() {
    setBusy(true);
    setErr(null);
    try {
      for (const p of trashPending) {
        await moveToTrash(projectId, p);
      }
      setMsg(`已移入回收站 ${trashPending.size} 个文件（.ocard/trash，可恢复）`);
      setTrashPending(new Set());
      setShowTrashConfirm(false);
      load();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  function toggleGroup(id: number) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  const groupOf = (path: string) => groups.find((g) => g.members.includes(path));

  return (
    <div>
      <div className="notice info">
        分类工作台（PRD §5.4）：方向键 ←→ 选择 · 数字键 1-9 分到分类夹 ·{" "}
        <span className="kbd">P</span> 标精选（复制进 精选/待修）· <span className="kbd">O</span>{" "}
        其他 · <span className="kbd">D</span> 标记删除（两段式，确认后移入回收站，可恢复）。
        连拍组自动折叠，一组保留 1-2 张。
      </div>

      <div className="row spread" style={{ marginBottom: 12 }}>
        <div className="row">
          <button className="btn ghost" onClick={load} disabled={busy}>
            {busy ? "加载中…" : "↻ 刷新"}
          </button>
          <span className="muted">
            {items.length} 个素材 · 已选：{items[selected]?.name ?? "—"}
          </span>
        </div>
        <div className="row">
          {cats.map((c, i) => (
            <span key={c} className="badge dim">
              <span style={{ color: CATEGORY_COLORS[i % CATEGORY_COLORS.length] }}>{i + 1}</span> {c}
            </span>
          ))}
          <span className="badge select">P 精选</span>
          <span className="badge dim">O 其他</span>
          <span className="badge err">D 删除</span>
        </div>
      </div>

      {msg && <div className="notice ok">{msg}</div>}
      {err && <div className="notice err">{err}</div>}
      {trashPending.size > 0 && (
        <div className="notice warn">
          ⚠️ 待删除（标记）{trashPending.size} 个文件。删除永远两段式：标记 → 汇总人工确认 →
          移入回收站，不直接物理删除。
          <div className="row" style={{ marginTop: 8 }}>
            <button className="btn danger small" disabled={busy} onClick={() => setShowTrashConfirm(true)}>
              确认移入回收站
            </button>
            <button className="btn ghost small" onClick={() => setTrashPending(new Set())}>
              取消标记
            </button>
          </div>
        </div>
      )}

      {showTrashConfirm && (
        <div className="card" style={{ borderColor: "var(--err)" }}>
          <h3>确认删除（第二次确认）</h3>
          <p className="muted">
            以下 {trashPending.size} 个文件将移入项目回收站 .ocard/trash，仍可恢复：
          </p>
          <pre className="dump">
            {Array.from(trashPending).map((p) => p.split("/").pop()).join("\n")}
          </pre>
          <div className="row">
            <button className="btn danger" onClick={confirmTrash} disabled={busy}>
              确认移入回收站
            </button>
            <button className="btn ghost" onClick={() => setShowTrashConfirm(false)}>
              再想想
            </button>
          </div>
        </div>
      )}

      {groups.length > 0 && (
        <div className="row" style={{ marginBottom: 8 }}>
          <span className="muted">连拍组：</span>
          {groups.map((g) => (
            <button
              key={g.id}
              className="btn ghost small"
              onClick={() => toggleGroup(g.id)}
            >
              {collapsed.has(g.id) ? "展开" : "折叠"} 组{g.id + 1}（{g.members.length} 张）
            </button>
          ))}
        </div>
      )}

      <div className="grid" ref={gridRef}>
        {visibleItems.map((m, idx) => {
          const a = analysis[m.path];
          const g = groupOf(m.path);
          const isSel = items[selected]?.path === m.path;
          const isPending = trashPending.has(m.path);
          const isKeep = g?.keep.includes(m.path);
          return (
            <div
              key={m.path}
              className={`grid-item ${isSel ? "selected" : ""}`}
              style={isPending ? { borderColor: "var(--err)", opacity: 0.6 } : {}}
              onClick={() => {
                const realIdx = items.findIndex((x) => x.path === m.path);
                if (realIdx >= 0) setSelected(realIdx);
              }}
            >
              {thumbs[m.path] ? (
                <img src={toFileUrl(thumbs[m.path])} alt={m.name} loading="lazy" />
              ) : (
                <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%", color: "var(--text-dim)", fontSize: 11 }}>
                  {m.is_video ? "🎞 视频" : "无法预览"}
                </div>
              )}
              {a?.blurry && <span className="flag blurry">糊</span>}
              {a?.overexposed && <span className="flag over">过曝</span>}
              {a?.underexposed && <span className="flag over">欠曝</span>}
              {a?.eyes_closed === true && <span className="flag eyes">闭眼</span>}
              {isKeep && <span className="flag" style={{ background: "var(--select)", color: "#000" }}>荐</span>}
              {isPending && <span className="flag" style={{ background: "var(--err)", color: "#fff" }}>删</span>}
              {g && (
                <span className="flag" style={{ top: 4, left: 4, background: "rgba(0,0,0,0.6)", color: "#fff" }}>
                  {g.id + 1}#{g.members.indexOf(m.path) + 1}
                </span>
              )}
              <div className="meta">
                {m.name} · {fmtBytes(m.size)}
                {a?.quality ? ` · ${Math.round(a.quality.overall)}分` : ""}
              </div>
            </div>
          );
        })}
      </div>

      {!busy && items.length === 0 && (
        <div className="notice info">「1. 待分类」暂无素材。先拷卡或手动放入文件。</div>
      )}
    </div>
  );
}
