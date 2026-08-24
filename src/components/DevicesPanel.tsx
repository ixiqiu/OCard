import { useCallback, useEffect, useState } from "react";
import { addCamera, addCard, listDevices } from "../api";
import type { DeviceRegistry } from "../types";

interface Props {
  projectId: string;
  onChanged: () => void;
}

export default function DevicesPanel({ projectId, onChanged }: Props) {
  const [reg, setReg] = useState<DeviceRegistry | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  // 相机表单
  const [model, setModel] = useState("");
  const [position, setPosition] = useState("A");
  const [operator, setOperator] = useState("");
  // 卡表单
  const [label, setLabel] = useState("");
  const [cameraId, setCameraId] = useState("");

  const load = useCallback(() => {
    listDevices(projectId).then(setReg).catch((e) => setErr(String(e)));
  }, [projectId]);

  useEffect(load, [load]);

  async function submitCamera() {
    setErr(null);
    setMsg(null);
    try {
      const cam = await addCamera(projectId, model, position, operator, null);
      setMsg(`已登记相机，自动生成编码：${cam.code}`);
      setModel("");
      setOperator("");
      load();
      onChanged();
    } catch (e) {
      setErr(String(e));
    }
  }

  async function submitCard() {
    setErr(null);
    setMsg(null);
    try {
      const card = await addCard(projectId, label, cameraId, null);
      setMsg(`已登记存储卡：${card.label}`);
      setLabel("");
      load();
      onChanged();
    } catch (e) {
      setErr(String(e));
    }
  }

  return (
    <div>
      <div className="notice info">
        设备与存储卡登记（PRD §5.1）：登记相机（型号、机位 A–Z、使用者代称）自动生成规范编码
        （如 DJIRonin4D_B_ZS）；登记存储卡并与相机关联（一卡一机），支持打印/显示辨识标签。
        登记表全项目共享，拷卡时自动带出编码用于命名。
      </div>

      {err && <div className="notice err">{err}</div>}
      {msg && <div className="notice ok">{msg}</div>}

      <div className="card">
        <h3>登记相机</h3>
        <div className="row">
          <div style={{ flex: 2 }}>
            <label>型号</label>
            <input value={model} onChange={(e) => setModel(e.target.value)} placeholder="DJI Ronin 4D" />
          </div>
          <div style={{ flex: 1 }}>
            <label>机位（A–Z）</label>
            <input value={position} onChange={(e) => setPosition(e.target.value.toUpperCase())} maxLength={1} />
          </div>
          <div style={{ flex: 1 }}>
            <label>使用者代称</label>
            <input value={operator} onChange={(e) => setOperator(e.target.value)} placeholder="ZS" />
          </div>
        </div>
        <button className="btn" onClick={submitCamera}>
          登记相机
        </button>
      </div>

      <div className="card">
        <h3>登记存储卡（一卡一机）</h3>
        <div className="row">
          <div style={{ flex: 1 }}>
            <label>辨识标签</label>
            <input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="CARD-001" />
          </div>
          <div style={{ flex: 1 }}>
            <label>关联相机</label>
            <select value={cameraId} onChange={(e) => setCameraId(e.target.value)}>
              <option value="">— 选择相机 —</option>
              {reg?.cameras.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.code}
                </option>
              ))}
            </select>
          </div>
        </div>
        <button className="btn" onClick={submitCard}>
          登记存储卡
        </button>
      </div>

      <div className="card" style={{ padding: 0 }}>
        <table>
          <thead>
            <tr>
              <th>相机编码</th>
              <th>型号</th>
              <th>机位</th>
              <th>使用者</th>
              <th>关联卡</th>
            </tr>
          </thead>
          <tbody>
            {reg?.cameras.map((c) => {
              const cards = reg.cards.filter((card) => card.camera_id === c.id);
              return (
                <tr key={c.id}>
                  <td>
                    <b>{c.code}</b>
                  </td>
                  <td>{c.model}</td>
                  <td>{c.position}</td>
                  <td>{c.operator}</td>
                  <td>{cards.length ? cards.map((card) => card.label).join(", ") : "—"}</td>
                </tr>
              );
            })}
            {reg && reg.cameras.length === 0 && (
              <tr>
                <td colSpan={5} className="muted">
                  尚未登记相机
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
