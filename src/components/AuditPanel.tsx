import { useCallback, useEffect, useState } from "react";
import { getAuditLog } from "../api";
import type { AuditEntry } from "../types";

interface Props {
  projectId: string;
}

export default function AuditPanel({ projectId }: Props) {
  const [entries, setEntries] = useState<AuditEntry[]>([]);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => {
    getAuditLog(projectId)
      .then(setEntries)
      .catch((e) => setErr(String(e)));
  }, [projectId]);

  useEffect(load, [load]);

  const actionLabel: Record<string, string> = {
    copy: "拷卡",
    verify: "校验",
    trash: "移入回收站",
    restore: "恢复",
    classify: "分类",
    select: "标精选",
    package: "打包",
    transcode: "转码",
    create_project: "建项目",
    register_camera: "登记相机",
    register_card: "登记卡",
  };

  return (
    <div>
      <div className="notice info">
        审计日志（PRD §5.9/§6.3）：拷卡、校验、删除确认、打包等关键操作追加写入项目内日志
        （操作人 = 当前登记的 DIT、时间、动作、对象），支撑「双岗互相监督」。每台工作站写自己的
        日志文件（以机器 ID 命名），读取时合并所有工作站记录。
      </div>

      {err && <div className="notice err">{err}</div>}
      <div className="card" style={{ padding: 0 }}>
        <table>
          <thead>
            <tr>
              <th>时间</th>
              <th>工作站</th>
              <th>操作人</th>
              <th>动作</th>
              <th>对象</th>
              <th>详情</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e, i) => (
              <tr key={i}>
                <td style={{ whiteSpace: "nowrap" }}>{e.timestamp}</td>
                <td>{e.machine_id}</td>
                <td>{e.operator}</td>
                <td>
                  <span className="badge dim">{actionLabel[e.action] ?? e.action}</span>
                </td>
                <td style={{ maxWidth: 260, overflow: "hidden", textOverflow: "ellipsis" }}>{e.target}</td>
                <td className="muted">{e.detail ?? "—"}</td>
              </tr>
            ))}
            {entries.length === 0 && (
              <tr>
                <td colSpan={6} className="muted">
                  暂无日志
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
