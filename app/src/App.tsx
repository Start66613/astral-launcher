import { useEffect, useState } from "react";
import TitleBar from "./components/TitleBar";
import Sidebar from "./components/Sidebar";
import AiDrawer from "./components/AiDrawer";
import Launch from "./pages/Launch";
import Download from "./pages/Download";
import Settings from "./pages/Settings";
import More from "./pages/More";
import { getSystemInfo } from "./lib/bridge";
import type { Skin, SystemInfo, Tab } from "./lib/types";

export type Section =
  | "vanilla"
  | "mod"
  | "modpack"
  | "datapack"
  | "resourcepack"
  | "shaderpack";

const DEFAULTS: Skin = {
  theme: "light",
  accent: "blue",
  radius: "default",
  motion: "on",
};

function loadSkin(): Skin {
  try {
    const raw = localStorage.getItem("astral.skin");
    const stored = raw ? { ...DEFAULTS, ...JSON.parse(raw) } : DEFAULTS;
    const q = new URLSearchParams(location.search);
    return {
      theme: (q.get("theme") as Skin["theme"]) ?? stored.theme,
      accent: (q.get("accent") as Skin["accent"]) ?? stored.accent,
      radius: (q.get("radius") as Skin["radius"]) ?? stored.radius,
      motion: (q.get("motion") as Skin["motion"]) ?? stored.motion,
    };
  } catch {
    return DEFAULTS;
  }
}

const TABS: Tab[] = ["launch", "download", "settings", "more"];

function initialTab(): Tab {
  const h = location.hash.replace("#", "") as Tab;
  return TABS.includes(h) ? h : "launch";
}

export default function App() {
  const [tab, setTabState] = useState<Tab>(initialTab);
  const [section, setSection] = useState<Section>(() => {
    const s = new URLSearchParams(location.search).get("section") as Section | null;
    return s ?? "vanilla";
  });
  const [skin, setSkinState] = useState<Skin>(loadSkin);
  const [aiOpen, setAiOpen] = useState(false);
  const [system, setSystem] = useState<SystemInfo | null>(null);

  function setTab(t: Tab) {
    setTabState(t);
    history.replaceState(null, "", "#" + t);
  }

  function setSkin(p: Partial<Skin>) {
    setSkinState((s) => {
      const next = { ...s, ...p };
      try {
        localStorage.setItem("astral.skin", JSON.stringify(next));
      } catch {
        /* 隐私模式忽略 */
      }
      return next;
    });
  }

  useEffect(() => {
    const el = document.documentElement;
    el.dataset.theme = skin.theme;
    el.dataset.accent = skin.accent;
    el.dataset.radius = skin.radius;
    el.dataset.motion = skin.motion;
  }, [skin]);

  useEffect(() => {
    void getSystemInfo().then(setSystem);
  }, []);

  return (
    <div className="app">
      <TitleBar tab={tab} onTab={setTab} onOpenAi={() => setAiOpen(true)} />
      <div className="body">
        <Sidebar tab={tab} section={section} onSection={setSection} />
        <main className="content">
          {tab === "launch" && (
            <Launch onOpenAi={() => setAiOpen(true)} system={system} />
          )}
          {tab === "download" && <Download section={section} onSection={setSection} />}
          {tab === "settings" && <Settings skin={skin} setSkin={setSkin} system={system} />}
          {tab === "more" && <More />}
        </main>
      </div>

      <AiDrawer open={aiOpen} onClose={() => setAiOpen(false)} />
    </div>
  );
}
