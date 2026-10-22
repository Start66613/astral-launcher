import { useEffect, useState } from "react";
import type { Tab } from "../lib/types";
import { ACCOUNT } from "../lib/data";
import { inTauri, launchVersion, onProgress, scanVersions } from "../lib/bridge";
import type { Section } from "../App";
import { Svg } from "./ui";

type Phase = "idle" | "running" | "done";

export default function Sidebar({
  tab,
  section,
  onSection,
}: {
  tab: Tab;
  section: Section;
  onSection: (s: Section) => void;
}) {
  return (
    <aside className="side">
      {tab === "launch" && <LaunchSide />}
      {tab === "download" && <DownloadSide section={section} onSection={onSection} />}
      {tab === "settings" && <SettingsSide />}
      {tab === "more" && <MoreSide />}
    </aside>
  );
}

function LaunchSide() {
  const [versions, setVersions] = useState<string[]>([]);
  const [current, setCurrent] = useState("");
  const [pickOpen, setPickOpen] = useState(false);
  const [phase, setPhase] = useState<Phase>("idle");
  const [pct, setPct] = useState(0);
  const [label, setLabel] = useState("");
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!inTauri) {
      setVersions(["1.21.1"]);
      setCurrent("1.21.1");
      return;
    }
    void scanVersions().then((list) => {
      setVersions(list);
      setCurrent((c) => c || list[list.length - 1] || "");
    });
  }, []);

  // 浏览器演示：假的进度
  useEffect(() => {
    if (phase !== "running" || inTauri) return;
    const id = window.setInterval(() => {
      setPct((p) => {
        const next = Math.min(100, p + 12);
        if (next >= 100) {
          setPhase("done");
          setLabel("已启动 · 用时 2.4 秒（演示）");
        }
        return next;
      });
    }, 130);
    return () => window.clearInterval(id);
  }, [phase]);

  useEffect(() => {
    if (phase !== "done") return;
    const id = window.setTimeout(() => {
      setPhase("idle");
      setPct(0);
      setLabel("");
    }, 2500);
    return () => window.clearTimeout(id);
  }, [phase]);

  async function start() {
    if (!current || phase === "running") return;
    setPhase("running");
    setPct(0);
    setErr(null);

    if (!inTauri) {
      setLabel("正在校验模组…（演示）");
      return;
    }

    setLabel("正在准备…");
    let un: (() => void) | undefined;
    un = await onProgress((v, p) => {
      if (v !== current) return;
      setLabel(p.label);
      setPct(p.total > 0 ? Math.round((p.done / p.total) * 100) : 0);
    });
    try {
      const pid = await launchVersion(current);
      setPhase("done");
      setPct(100);
      setLabel(`已启动 · PID ${pid}`);
    } catch (e) {
      setErr(String(e));
      setPhase("idle");
      setLabel("");
    } finally {
      un?.();
    }
  }

  const hint = err ?? label;

  return (
    <div className="sb">
      <div className="acct-top">
        <div className="seg">
          <button className="on">正版</button>
          <button>离线</button>
        </div>
      </div>

      <div className="acct">
        <div className="ava">{ACCOUNT.initial}</div>
        <div className="acct-name">{ACCOUNT.name}</div>
        <span className="chip acc">{ACCOUNT.type}</span>
        <a>切换账号</a>
      </div>

      <div className="side-bot">
        <div style={{ position: "relative" }}>
          <button className="ver-pick" onClick={() => setPickOpen((v) => !v)}>
            <div>
              <b>{current || "未安装任何版本"}</b>
              <span>
                {versions.length > 0
                  ? `本地 ${versions.length} 个版本`
                  : "去「下载」装一个"}
              </span>
            </div>
            <Svg>
              <path d="m6 9 6 6 6-6" />
            </Svg>
          </button>

          {pickOpen && (
            <div className="ver-menu">
              {versions.length === 0 && (
                <div className="ver-menu-empty">没有可用版本</div>
              )}
              {versions.map((v) => (
                <button
                  key={v}
                  className={"ver-menu-item" + (v === current ? " on" : "")}
                  onClick={() => {
                    setCurrent(v);
                    setPickOpen(false);
                  }}
                >
                  {v}
                </button>
              ))}
            </div>
          )}
        </div>

        <button
          className="btn btn-primary launch"
          disabled={phase === "running" || !current}
          onClick={() => void start()}
        >
          <svg viewBox="0 0 24 24">
            <path d="M8 5v14l11-7z" />
          </svg>
          {phase === "running" ? "正在启动…" : phase === "done" ? "已启动" : "启动游戏"}
        </button>

        {(phase === "running" || phase === "done") && (
          <>
            <div className="progress">
              <i style={{ width: `${pct}%` }} />
            </div>
            <div className="p-hint" style={err ? { color: "var(--red)" } : undefined}>
              {hint}
            </div>
          </>
        )}

        <div className="side-row">
          <button className="btn btn-ghost">版本选择</button>
          <button className="btn btn-ghost">版本设置</button>
        </div>
      </div>
    </div>
  );
}

function DownloadSide({
  section,
  onSection,
}: {
  section: Section;
  onSection: (s: Section) => void;
}) {
  const IDS: Section[] = [
    "vanilla",
    "mod",
    "modpack",
    "datapack",
    "resourcepack",
    "shaderpack",
  ];
  const items = [
    {
      label: "原版游戏",
      icon: (
        <>
          <path d="M12 2.6 3 7v10l9 4.4 9-4.4V7l-9-4.4Z" />
          <path d="M3 7l9 4.4L21 7M12 11.4v10" />
        </>
      ),
    },
    {
      label: "Mod",
      icon: (
        <>
          <path d="M4 6h16M4 12h16M4 18h10" />
        </>
      ),
    },
    { label: "整合包", icon: <path d="M12 3 3.5 7.5v9L12 21l8.5-4.5v-9L12 3Z" /> },
    {
      label: "数据包",
      icon: (
        <>
          <path d="M4 7h16v10H4z" />
          <path d="M4 12h16" />
        </>
      ),
    },
    {
      label: "资源包",
      icon: (
        <>
          <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
          <path d="m3.5 16 5-5 4 4 3-3 5 5" />
        </>
      ),
    },
    {
      label: "光影包",
      icon: (
        <>
          <circle cx="12" cy="12" r="4" />
          <path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9 7 7M17 17l2.1 2.1M19.1 4.9 17 7M7 17l-2.1 2.1" />
        </>
      ),
    },
  ];
  return (
    <div className="sb">
      {items.map((it, i) => (
        <div key={it.label} style={{ display: "contents" }}>
          {i === 1 && <div className="sb-label">社区资源</div>}
          <button
            className={"sb-btn" + (section === IDS[i] ? " on" : "")}
            onClick={() => onSection(IDS[i])}
          >
            <Svg>{it.icon}</Svg>
            {it.label}
          </button>
        </div>
      ))}
    </div>
  );
}

function SettingsSide() {
  const items = [
    { label: "启动", icon: <path d="M8 5v14l11-7z" /> },
    {
      label: "个性化",
      icon: (
        <>
          <circle cx="12" cy="9" r="3.4" />
          <path d="M5 20a7 7 0 0 1 14 0" />
        </>
      ),
    },
    {
      label: "其他",
      icon: (
        <>
          <circle cx="12" cy="12" r="3" />
          <path d="M12 3v2M12 19v2M3 12h2M19 12h2" />
        </>
      ),
    },
  ];
  return (
    <div className="sb">
      {items.map((it, i) => (
        <button className={"sb-btn" + (i === 0 ? " on" : "")} key={it.label}>
          <Svg>{it.icon}</Svg>
          {it.label}
        </button>
      ))}
    </div>
  );
}

function MoreSide() {
  const items = [
    {
      label: "帮助",
      icon: (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M9.6 9.4a2.5 2.5 0 1 1 3.3 2.4c-.6.3-.9.8-.9 1.4v.4" />
        </>
      ),
    },
    {
      label: "关于与鸣谢",
      icon: (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 8v8M8 12h8" />
        </>
      ),
    },
    {
      label: "反馈",
      icon: (
        <>
          <path d="M21 12a8 8 0 1 1-3.2-6.4" />
          <path d="M21 4v5h-5" />
        </>
      ),
    },
    {
      label: "新功能投票",
      icon: (
        <>
          <path d="M12 20V9" />
          <path d="m7 13 5-5 5 5" />
          <path d="M4 20h16" />
        </>
      ),
    },
  ];
  return (
    <div className="sb">
      {items.map((it, i) => (
        <button className={"sb-btn" + (i === 0 ? " on" : "")} key={it.label}>
          <Svg>{it.icon}</Svg>
          {it.label}
        </button>
      ))}
    </div>
  );
}
