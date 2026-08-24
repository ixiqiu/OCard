import { useCallback, useEffect, useState } from "react";
import { getConfig, listProjects } from "./api";
import type { AppConfig, ProjectSummary } from "./types";
import ProjectListPage from "./pages/ProjectListPage";
import ProjectDetailPage from "./pages/ProjectDetailPage";
import SettingsPage from "./pages/SettingsPage";

type View = { kind: "home" } | { kind: "settings" } | { kind: "project"; id: string };

export default function App() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [view, setView] = useState<View>({ kind: "home" });
  const [error, setError] = useState<string | null>(null);

  const refreshProjects = useCallback(() => {
    listProjects()
      .then(setProjects)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    getConfig()
      .then((c) => setConfig(c))
      .catch((e) => setError(String(e)));
    refreshProjects();
  }, [refreshProjects]);

  const openProject = (id: string) => {
    setView({ kind: "project", id });
  };

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          📀 OCard
          <small>DIT 素材管理</small>
        </div>
        <nav>
          <button
            className={view.kind === "home" ? "active" : ""}
            onClick={() => setView({ kind: "home" })}
          >
            🏠 项目列表
          </button>
          <button
            className={view.kind === "settings" ? "active" : ""}
            onClick={() => setView({ kind: "settings" })}
          >
            ⚙️ 设置
          </button>
          <div className="project-list">
            <div className="pl-title">最近项目</div>
            {projects.slice(0, 8).map((p) => (
              <button
                key={p.id}
                className={
                  view.kind === "project" && view.id === p.id ? "active" : ""
                }
                onClick={() => openProject(p.id)}
              >
                📁 {p.id}
              </button>
            ))}
          </div>
        </nav>
        <div className="footer">
          {config ? `操作人：${config.operator}` : "加载中…"}
          <br />
          {config?.nas_root ? config.nas_root : "⚠️ 未配置 NAS 路径"}
        </div>
      </aside>

      <main className="main">
        {error && (
          <div className="notice err" onClick={() => setError(null)}>
            {error}
          </div>
        )}
        {view.kind === "home" && (
          <ProjectListPage
            projects={projects}
            onRefresh={refreshProjects}
            onOpen={openProject}
            configReady={!!config?.nas_root}
          />
        )}
        {view.kind === "settings" && (
          <SettingsPage config={config} onSaved={(c) => setConfig(c)} />
        )}
        {view.kind === "project" && (
          <ProjectDetailPage
            projectId={view.id}
            onRefreshProjects={refreshProjects}
          />
        )}
      </main>
    </div>
  );
}
