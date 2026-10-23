import { useEffect, useState } from "react";
import type { Accent, LauncherConfig, Skin, SystemInfo } from "../lib/types";
import { getConfig, inTauri, openPath, setConfig } from "../lib/bridge";
import { Card } from "../components/ui";
import Icon from "../components/Icon";

const ACCENTS: { value: Accent; label: string }[] = [{ value: "green", label: "松针绿" }, { value: "blue", label: "湖水蓝" }, { value: "purple", label: "暮山紫" }, { value: "orange", label: "木屋橙" }];
export default function Settings({ skin, setSkin, system, onSaved }: {
  skin: Skin; setSkin: (patch: Partial<Skin>) => void; system: SystemInfo | null; onSaved: () => void;
}) {
  const [config, setDraft] = useState<LauncherConfig | null>(null);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    let alive = true;
    if (inTauri) void getConfig().then(value => { if (alive) setDraft(value); }).catch(cause => { if (alive) setError(String(cause)); });
    return () => { alive = false; };
  }, []);
  function edit(patch: Partial<LauncherConfig>) { setDraft(old => old ? { ...old, ...patch } : old); setSaved(false); }
  async function save() {
    if (!inTauri || !config || saving) return;
    if (!config.username.trim()) { setError("请填写离线用户名。"); return; }
    setSaving(true); setError(""); setSaved(false);
    try { await setConfig(config); setSaved(true); onSaved(); } catch (cause) { setError(String(cause)); }
    finally { setSaving(false); }
  }
  return <section className="page settings-page">
    <header className="page-head"><div><span className="eyebrow">MAKE IT YOURS</span><h1 id="page-title" tabIndex={-1}>设置</h1><p>让每一次启程，都合乎你的习惯。</p></div></header>
    <div className="settings-layout"><div className="settings-main">
      <Card title="外观" meta="本机保存，即时生效">
        <div className="frow"><span className="lbl">昼与夜</span><div className="val option-group">{([{ value: "dark", label: "深林夜色", icon: "moon" }, { value: "light", label: "林间薄雾", icon: "sun" }] as const).map(item => <button key={item.value} className={`opt ${skin.theme === item.value ? "on" : ""}`} aria-pressed={skin.theme === item.value} onClick={() => setSkin({ theme: item.value })}><Icon name={item.icon} />{item.label}</button>)}</div></div>
        <div className="frow"><span className="lbl">强调色</span><div className="val option-group">{ACCENTS.map(item => <button key={item.value} className={`accent-option ${skin.accent === item.value ? "on" : ""}`} aria-pressed={skin.accent === item.value} onClick={() => setSkin({ accent: item.value })}><i data-color={item.value} />{item.label}</button>)}</div></div>
        <div className="frow"><span className="lbl">圆角</span><div className="val option-group">{([{ value: "sharp", label: "利落" }, { value: "default", label: "适中" }, { value: "soft", label: "柔和" }] as const).map(item => <button key={item.value} className={`opt ${skin.radius === item.value ? "on" : ""}`} aria-pressed={skin.radius === item.value} onClick={() => setSkin({ radius: item.value })}>{item.label}</button>)}</div></div>
        <div className="frow"><span className="lbl">交互动效</span><div className="val option-group">{([{ value: "on", label: "轻柔过渡" }, { value: "off", label: "减少动效" }] as const).map(item => <button key={item.value} className={`opt ${skin.motion === item.value ? "on" : ""}`} aria-pressed={skin.motion === item.value} onClick={() => setSkin({ motion: item.value })}>{item.label}</button>)}<small className="field-note">遵循系统的减少动效偏好。</small></div></div>
      </Card>
      <form onSubmit={event => { event.preventDefault(); void save(); }}>
        <Card title="下载与档案" meta={inTauri ? "修改后保存" : "桌面端可用"}>
          <div className="frow"><label className="lbl" htmlFor="download-source">下载源</label><div className="val"><select id="download-source" className="sel" disabled={!config || saving} value={config?.source || "official"} onChange={event => edit({ source: event.target.value })}><option value="official">官方 CDN</option><option value="bmclapi">BMCLAPI 镜像</option></select><p className="field-note">镜像加速依赖库与资源；版本元数据和客户端仍使用官方来源。</p></div></div>
          <div className="frow"><span className="lbl">游戏目录</span><div className="val path-field"><code>{config?.gameDir || (inTauri ? "尚未读取到目录" : "连接桌面端后读取")}</code><button type="button" className="icon-button" disabled={!config || !inTauri} onClick={() => config && void openPath(config.gameDir)} aria-label="打开游戏目录"><Icon name="folder" /></button></div></div>
          <div className="frow"><label className="lbl" htmlFor="offline-name">离线用户名</label><div className="val"><input id="offline-name" className="text-input" disabled={!config || saving} value={config?.username || ""} placeholder={inTauri ? "填写用户名" : "尚未连接"} onChange={event => edit({ username: event.target.value })} /><p className="field-note">离线档案不代表已验证的 Microsoft 身份。</p></div></div>
          <div className="form-actions"><span role="status">{saved ? "已保存到本机" : !inTauri ? "浏览器不会修改游戏配置" : ""}</span><button className="btn btn-primary" type="submit" disabled={!config || saving || !inTauri}>{saving ? "正在保存…" : "保存设置"}<Icon name="check" /></button></div>
        </Card>
      </form>
      {error && <p className="error-message" role="alert">{error}</p>}
      <Card title="设备信息"><div className="device-info">{system ? <><Icon name="compass" /><div><strong>{system.os}</strong><p>{system.cpuCores} 核 · 总内存 {system.totalMemGB} GB · 可用 {system.freeMemGB} GB</p></div></> : <><Icon name="compass" /><p>连接桌面端后读取真实设备信息。</p></>}</div></Card>
    </div><aside className="settings-aside"><div className="theme-preview"><span>FOREST TALE</span><Icon name="star" /><strong>萤火夜航</strong><small>为每一个小小的世界，留一盏灯。</small></div><p>深林的绿，木屋的暖。<br />给冒险一个安静的开始。</p></aside></div>
  </section>;
}
