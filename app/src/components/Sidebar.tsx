import type { View } from "../App";
import Icon, { type IconName } from "./Icon";

const ITEMS: { id: View; name: string; icon: IconName }[] = [
  { id: "launch", name: "首页", icon: "home" }, { id: "versions", name: "版本管理", icon: "layers" },
  { id: "download", name: "下载中心", icon: "download" }, { id: "mods", name: "模组中心", icon: "mods" },
  { id: "settings", name: "设置", icon: "settings" },
];
export default function Sidebar({ view, onNavigate, onAi, onAccount, onHelp }: {
  view: View; onNavigate: (view: View) => void; onAi: () => void; onAccount: () => void; onHelp: () => void;
}) {
  return <aside className="side">
    <button className="rail-brand" onClick={() => onNavigate("launch")} aria-label="Astral 首页"><Icon name="star" /></button>
    <nav className="rail-nav" aria-label="主导航">{ITEMS.map(item => <button key={item.id} className={`rail-item ${view === item.id ? "on" : ""}`} aria-current={view === item.id ? "page" : undefined} onClick={() => onNavigate(item.id)}><Icon name={item.icon} /><span>{item.name}</span></button>)}</nav>
    <div className="rail-bottom"><button className="rail-item" onClick={onAi}><Icon name="spark" /><span>AI 诊断</span></button><button className={`rail-help icon-button ${view === "more" ? "on" : ""}`} onClick={onHelp} aria-label="帮助与说明"><Icon name="help" /></button><button className="rail-account icon-button" onClick={onAccount} aria-label="查看帐号状态"><Icon name="user" /></button></div>
  </aside>;
}
