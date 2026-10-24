import AstralV2 from "./v2/App";
import { useEffect, useRef, useState } from "react";
import TitleBar from "./components/TitleBar";
import Sidebar from "./components/Sidebar";
import AiDrawer from "./components/AiDrawer";
import Icon from "./components/Icon";
import Modal from "./components/Modal";
import Launch from "./pages/Launch";
import Versions from "./pages/Versions";
import Download from "./pages/Download";
import Resources from "./pages/Resources";
import Settings from "./pages/Settings";
import More from "./pages/More";
import { getSystemInfo, inTauri } from "./lib/bridge";
import type { Skin, SystemInfo } from "./lib/types";
import { useLocalLauncher } from "./hooks/useLocalLauncher";

export type Section = "vanilla" | "mod" | "modpack" | "datapack" | "resourcepack" | "shaderpack";
export type View = "launch" | "versions" | "download" | "mods" | "settings" | "more";
const VIEWS: View[] = ["launch", "versions", "download", "mods", "settings", "more"];
const RESOURCE_SECTIONS: { id: Section; label: string }[] = [{ id: "vanilla", label: "游戏版本" }, { id: "modpack", label: "整合包" }, { id: "resourcepack", label: "资源包" }, { id: "shaderpack", label: "光影" }, { id: "datapack", label: "数据包" }];
const DEFAULTS: Skin = { theme: "dark", accent: "green", radius: "soft", motion: "on" };
function choose<T extends string>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.includes(value as T) ? value as T : fallback;
}
function loadSkin(): Skin {
  let stored: Partial<Skin> = {};
  try { const raw: unknown = JSON.parse(localStorage.getItem("astral.skin") || "{}"); if (raw && typeof raw === "object") stored = raw; } catch { /* Defaults are safe in private browsing. */ }
  const q = new URLSearchParams(location.search);
  return {
    theme: choose(q.get("theme") ?? stored.theme, ["dark", "light"] as const, DEFAULTS.theme),
    accent: choose(q.get("accent") ?? stored.accent, ["green", "blue", "purple", "orange"] as const, DEFAULTS.accent),
    radius: choose(q.get("radius") ?? stored.radius, ["sharp", "default", "soft"] as const, DEFAULTS.radius),
    motion: choose(q.get("motion") ?? stored.motion, ["on", "off"] as const, matchMedia("(prefers-reduced-motion: reduce)").matches ? "off" : DEFAULTS.motion),
  };
}
function initialView(): View { return choose(location.hash.slice(1), VIEWS, "launch"); }

export function LegacyApp() {
  const [view, setView] = useState<View>(initialView);
  const [downloadsVisited, setDownloadsVisited] = useState(initialView() === "download");
  const [modsVisited, setModsVisited] = useState(initialView() === "mods");
  const [section, setSection] = useState<Section>(() => choose(new URLSearchParams(location.search).get("section"), ["vanilla", "mod", "modpack", "datapack", "resourcepack", "shaderpack"] as const, "vanilla"));
  const [skin, setSkinState] = useState<Skin>(loadSkin);
  const [aiOpen, setAiOpen] = useState(false);
  const [accountOpen, setAccountOpen] = useState(false);
  const [windowError, setWindowError] = useState("");
  const [system, setSystem] = useState<SystemInfo | null>(null);
  const content = useRef<HTMLElement>(null);
  const launcher = useLocalLauncher();
  function navigate(next: View) {
    if (next === view) return;
    if (next === "download") setDownloadsVisited(true);
    if (next === "mods") setModsVisited(true);
    history.pushState(null, "", `#${next}`); setView(next);
  }
  function setSkin(patch: Partial<Skin>) {
    setSkinState(old => {
      const next = { ...old, ...patch };
      try { localStorage.setItem("astral.skin", JSON.stringify(next)); } catch { /* Session-only appearance remains usable. */ }
      return next;
    });
  }
  useEffect(() => {
    const onHistory = () => { const next = initialView(); setView(next); if (next === "download") setDownloadsVisited(true); if (next === "mods") setModsVisited(true); };
    window.addEventListener("popstate", onHistory); window.addEventListener("hashchange", onHistory);
    return () => { window.removeEventListener("popstate", onHistory); window.removeEventListener("hashchange", onHistory); };
  }, []);
  useEffect(() => {
    const el = document.documentElement;
    el.dataset.theme = skin.theme; el.dataset.accent = skin.accent; el.dataset.radius = skin.radius; el.dataset.motion = skin.motion;
  }, [skin]);
  useEffect(() => {
    let alive = true;
    if (inTauri && (launcher.mode === "core" || launcher.mode === "legacy")) void getSystemInfo().then(info => { if (alive && !info.demo) setSystem(info); }).catch(() => {});
    return () => { alive = false; };
  }, [launcher.mode]);
  useEffect(() => { content.current?.scrollTo(0, 0); [...(content.current?.querySelectorAll<HTMLElement>("h1") || [])].find(heading => heading.getClientRects().length > 0)?.focus({ preventScroll: true }); }, [view]);

  return <div className={`app ${view === "launch" ? "home-view" : "interior-view"}`}>
    <a className="skip-link" href="#main-content" onClick={event => { event.preventDefault(); content.current?.focus(); }}>跳到页面内容</a>
    <TitleBar theme={skin.theme} onTheme={() => setSkin({ theme: skin.theme === "dark" ? "light" : "dark" })} onError={setWindowError} />
    <div className="body"><div className="forest-backdrop" aria-hidden="true" />
      <Sidebar view={view} onNavigate={navigate} onAi={() => setAiOpen(true)} onAccount={() => setAccountOpen(true)} onHelp={() => navigate("more")} />
      <main ref={content} id="main-content" tabIndex={-1} className={`content ${view === "launch" ? "home-content" : "page-content"}`}>
        {view === "launch" && <Launch launcher={launcher} onAi={() => setAiOpen(true)} onDownload={() => navigate("download")} onVersions={() => navigate("versions")} onAccount={() => setAccountOpen(true)} />}
        {view === "versions" && <Versions launcher={launcher} onDownload={() => navigate("download")} onHome={() => navigate("launch")} />}
        <div hidden={view !== "download"}>{(downloadsVisited || view === "download") && <><nav className="resource-tabs" aria-label="下载资源类别">{RESOURCE_SECTIONS.map(item => <button key={item.id} aria-pressed={section === item.id} onClick={() => setSection(item.id)}>{item.label}</button>)}</nav><Download section={section} onSection={setSection} onInstalled={() => void launcher.refresh()} /></>}</div>
        <div hidden={view !== "mods"}>{(modsVisited || view === "mods") && <Resources kind="mod" onGoVanilla={() => { setSection("vanilla"); navigate("download"); }} />}</div>
        {view === "settings" && <Settings skin={skin} setSkin={setSkin} system={system} onSaved={() => void launcher.refresh()} />}
        {view === "more" && <More />}
      </main>
    </div>
    <footer className="statusbar"><span><i />{launcher.statusLabel}</span><span>FOREST TALE <em>/</em> ASTRAL <small>0.1.0</small></span></footer>
    <AiDrawer open={aiOpen} onClose={() => setAiOpen(false)} />
    <Modal open={accountOpen} title="帐号与档案" onClose={() => setAccountOpen(false)}><div className="account-detail"><span className="account-avatar"><Icon name="user" /></span><h3>{launcher.config?.username || "尚未连接帐号"}</h3><p>{launcher.config?.username ? "当前使用离线档案，未验证 Microsoft 身份。" : "Microsoft 登录尚未接入，当前没有已验证的登录身份。"}</p></div><p className="modal-note">离线用户名可以在桌面端的设置中修改。Microsoft 帐号认证尚未接入。</p><button className="btn btn-primary modal-full-button" onClick={() => { setAccountOpen(false); navigate("settings"); }}>前往设置 <Icon name="arrow" /></button></Modal>
    <Modal open={Boolean(windowError)} title="窗口操作未完成" onClose={() => setWindowError("")}><p className="error-message">{windowError}</p></Modal>
  </div>;
}

export default function App() { const runtime = useLocalLauncher(); return <AstralV2 runtime={runtime} />; }
