import { useCallback, useEffect, useState } from "react";
import { getConfig, getProject, getProjectSummary, saveConfig } from "../api";
import type { AppConfig, Project } from "../types";
import CopyPanel from "../components/CopyPanel";
import CullingPanel from "../components/CullingPanel";
import TranscodePanel from "../components/TranscodePanel";
import PackagingPanel from "../components/PackagingPanel";
import DevicesPanel from "../components/DevicesPanel";
import AuditPanel from "../components/AuditPanel";

interface Props {
  projectId: string;
  onRefreshProjects: () => void;
}

type Tab = "copy" | "culling" | "transcode" | "packaging" | "devices" | "audit";

export default function ProjectDetailPage({ projectId, onRefreshProjects }: Props) {
  const [project, setProject] = useState<Project | null>(null);
  const [tab, setTab] = useState<Tab>("copy");
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => {
    getProject(projectId)
      .then(setProject)
      .catch((e) => setErr(String(e)));
    getProjectSummary(projectId).catch(() => {});
    getConfig().then(setConfig).catch(() => {});
  }, [projectId]);

  useEffect(load, [load]);

  const tabs: { id: Tab; label: string }[] = [
    { id: "copy", label: "📥 拷卡" },
    { id: "culling", label: "🗂 分类工作台" },
    { id: "transcode", label: "🎞 转码" },
    { id: "packaging", label: "📦 交付" },
    { id: "devices", label: "🎥 设备登记" },
    { id: "audit", label: "📋 审计日志" },
  ];

  return (
    <div>
      <div className="page-title">
        📁 {projectId}
        <span className="hint">
          {project
            ? `工况 ${project.workflow === "A" ? "A（视频剪辑）" : "B（纯拍照）"}`
            : "加载中…"}
        </span>
        <button className="btn ghost small" onClick={load}>
          ↻
        </button>
      </div>

      {err && <div className="notice err">{err}</div>}

      <div className="tabs">
        {tabs.map((t) => (
          <button
            key={t.id}
            className={tab === t.id ? "active" : ""}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === "copy" && project && (
        <CopyPanel
          projectId={projectId}
          workflow={project.workflow}
          project={project}
          config={config}
        />
      )}
      {tab === "culling" && project && (
        <CullingPanel projectId={projectId} project={project} />
      )}
      {tab === "transcode" && project && (
        <TranscodePanel projectId={projectId} />
      )}
      {tab === "packaging" && (
        <PackagingPanel projectId={projectId} />
      )}
      {tab === "devices" && (
        <DevicesPanel projectId={projectId} onChanged={onRefreshProjects} />
      )}
      {tab === "audit" && <AuditPanel projectId={projectId} />}
    </div>
  );
}
