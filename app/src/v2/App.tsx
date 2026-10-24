import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Badge, Icon } from "./icons";
import {
  LAUNCH_STEPS,
  RESEARCH,
  SAMPLE_VERSIONS,
  THEMES,
  launchState,
  type ThemeId,
} from "./model";
import { PreviewContext, usePreview, type Preferences } from "./state";
import { HomeB, WindowBar } from "./HomeB";
import type { LocalLauncher } from "../hooks/useLocalLauncher";
import {
  inTauri,
  patchConfig,
  minimizeWindow,
  parseApiError,
  getConfig,
  setConfig,
} from "../lib/bridge";
import {
  LiveLaunch,
  LiveSettings,
  LiveTasks,
  LiveVersions,
  LiveMore,
  LiveDiagnostics,
} from "./LiveRuntime";
import { LiveDownload, LiveGameInstaller, LiveResource } from "./LiveDownload";
import { DownloadPage, GameInstaller, ResourceInstaller } from "./Download";

function loadPreferences(storageKey: string): Preferences {
  const fallback: Preferences = {
    theme: "nightfall",
    mode: "dark",
    wallpaper: true,
    motion: true,
    intro: true,
    eco: false,
    randomOnStartup: true,
    launchEffect: "fireflies",
  };
  let stored: Partial<Preferences> = {};
  try {
    const raw = JSON.parse(localStorage.getItem(storageKey) || "{}");
    if (raw && typeof raw === "object") stored = raw;
  } catch {
    /* Random appearance still works without storage. */
  }
  const query = new URLSearchParams(location.search);
  const explicit = THEMES.find((item) => item.id === query.get("theme"));
  const previous = THEMES.find((item) => item.id === stored.theme);
  const randomOnStartup =
    typeof stored.randomOnStartup === "boolean" ? stored.randomOnStartup : true;
  const pool = THEMES.filter((item) => item.id !== previous?.id);
  const theme =
    explicit?.id ||
    (randomOnStartup
      ? pool[Math.floor(Math.random() * pool.length)].id
      : previous?.id || fallback.theme);
  const mode = query.get("mode") || stored.mode;
  return {
    ...fallback,
    theme,
    randomOnStartup,
    launchEffect: ["fireflies", "voxel", "light"].includes(
      stored.launchEffect || "",
    )
      ? stored.launchEffect!
      : "fireflies",
    mode: mode === "light" || mode === "dark" ? mode : fallback.mode,
    ...Object.fromEntries(
      (["wallpaper", "motion", "intro", "eco"] as const).map((key) => [
        key,
        typeof stored[key] === "boolean"
          ? stored[key]
          : fallback[key as keyof Preferences],
      ]),
    ),
  };
}

export function Modal({
  title,
  children,
  close,
  wide = false,
}: {
  title: string;
  children: ReactNode;
  close: () => void;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const element = ref.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className={`modal ${wide ? "wide" : ""}`}
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        close();
      }}
      onClick={(event) => {
        if (event.target !== event.currentTarget) return;
        const rect = event.currentTarget.getBoundingClientRect();
        if (
          event.clientX < rect.left ||
          event.clientX > rect.right ||
          event.clientY < rect.top ||
          event.clientY > rect.bottom
        )
          close();
      }}
    >
      <header>
        <div>
          <span className="kicker">ASTRAL</span>
          <h2 id={titleId}>{title}</h2>
        </div>
        <button className="icon-button" aria-label="关闭窗口" onClick={close}>
          <Icon name="close" />
        </button>
      </header>
      {children}
    </dialog>
  );
}

export function PageTitle({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children?: ReactNode;
}) {
  return (
    <header className="page-title">
      <div>
        <h1 tabIndex={-1}>{title}</h1>
        {subtitle && <p>{subtitle}</p>}
      </div>
      {children}
    </header>
  );
}
export function Empty({
  title,
  detail,
  action,
}: {
  title: string;
  detail: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      <Badge kind="game" large />
      <h2>{title}</h2>
      <p>{detail}</p>
      {action}
    </div>
  );
}
export function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: () => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className="toggle"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={onChange}
    >
      <i />
    </button>
  );
}
function Artwork({ hero = false }: { hero?: boolean }) {
  const { prefs } = usePreview();
  return (
    <div className={hero ? "hero-art" : "ambient-art"} aria-hidden="true">
      {THEMES.map((theme) => (
        <img
          key={theme.id}
          src={theme.image}
          alt=""
          decoding="async"
          className={theme.id === prefs.theme && prefs.wallpaper ? "on" : ""}
          onError={(event) => {
            event.currentTarget.hidden = true;
          }}
        />
      ))}
    </div>
  );
}

export default function App({ runtime }: { runtime?: LocalLauncher } = {}) {
  const storageKey = runtime
    ? "astral.v2.appearance"
    : "astral.v2.preview.preferences";
  const [prefs, setPrefs] = useState(() => loadPreferences(storageKey));
  const [route, setRoute] = useState(
    (
      {
        launch: "home",
        download: "download/game",
        mods: "download/mod",
      } as Record<string, string>
    )[location.hash.slice(1)] ||
      location.hash.slice(1) ||
      "home",
  );
  const [scenario, setScenario] = useState(
    new URLSearchParams(location.search).get("scenario") || "normal",
  );
  const [previewCurrentId, setPreviewCurrentId] = useState("preview_pine");
  const [accountOpen, setAccountOpen] = useState(false);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [tasksOpen, setTasksOpen] = useState(false);
  const [notice, setNotice] = useState("");
  const [launchIndex, setLaunchIndex] = useState(0);
  const [launchFailure, setLaunchFailure] = useState(false);
  const [systemReduce, setSystemReduce] = useState(
    matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  const main = useRef<HTMLElement>(null);
  const [requestedPage, argument] = route.split("/");
  const page = [
    "home",
    "versions",
    "download",
    "game",
    "resource",
    "settings",
    "more",
    ...(!runtime ? ["motion", "research"] : []),
  ].includes(requestedPage)
    ? requestedPage
    : "home";
  const versions = runtime
    ? runtime.mode === "core"
      ? [...runtime.core.instances]
          .sort((a, b) =>
            (b.lastPlayed || "").localeCompare(a.lastPlayed || ""),
          )
          .map((item) => ({
            id: item.id,
            name: item.name,
            gameVersion: item.gameVersion,
            loader: item.loader?.kind || "game",
            loaderVersion: item.loader?.version || "",
            shared: !item.managed,
            mods: item.modsCount,
            lastUsed: item.lastPlayed
              ? "上次运行 " + item.lastPlayed.slice(0, 10)
              : "尚未运行",
          }))
      : runtime.mode === "legacy"
        ? runtime.legacy.versions.map((id) => ({
            id,
            name: id,
            gameVersion: id,
            loader: "game",
            loaderVersion: "",
            shared: true,
            mods: 0,
            lastUsed: "共享游戏目录",
          }))
        : []
    : scenario === "empty"
      ? []
      : SAMPLE_VERSIONS;
  const currentId = runtime
    ? runtime.mode === "core"
      ? runtime.core.activeId || ""
      : runtime.legacy.current || ""
    : previewCurrentId;
  function setCurrentId(id: string) {
    if (!runtime) setPreviewCurrentId(id);
    else if (runtime.mode === "core")
      void runtime.core
        .select(id)
        .catch((cause) => setNotice(parseApiError(cause).message));
    else if (runtime.mode === "legacy") runtime.legacy.select(id);
  }
  function navigate(path: string) {
    if (path === route) return;
    history.pushState(null, "", `#${path}`);
    setRoute(path);
  }
  useEffect(() => {
    const handler = () =>
      setRoute(
        (
          {
            launch: "home",
            download: "download/game",
            mods: "download/mod",
          } as Record<string, string>
        )[location.hash.slice(1)] ||
          location.hash.slice(1) ||
          "home",
      );
    window.addEventListener("popstate", handler);
    window.addEventListener("hashchange", handler);
    return () => {
      window.removeEventListener("popstate", handler);
      window.removeEventListener("hashchange", handler);
    };
  }, []);
  useEffect(() => {
    main.current?.scrollTo(0, 0);
    main.current
      ?.querySelector<HTMLElement>("h1")
      ?.focus({ preventScroll: true });
  }, [route]);
  useEffect(() => {
    const media = matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => setSystemReduce(media.matches);
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.production = runtime && inTauri ? "true" : "false";
    root.dataset.theme = prefs.theme;
    root.dataset.mode = prefs.mode;
    root.dataset.motion = prefs.motion && !systemReduce ? "on" : "off";
    root.dataset.intro = prefs.intro ? "on" : "off";
    root.dataset.eco =
      prefs.eco || navigator.hardwareConcurrency <= 4 ? "on" : "off";
    try {
      localStorage.setItem(storageKey, JSON.stringify(prefs));
    } catch {
      /* Preferences remain usable for this session. */
    }
  }, [prefs, systemReduce]);
  useEffect(() => {
    const visibility = () => {
      document.documentElement.dataset.paused = document.hidden
        ? "true"
        : "false";
    };
    visibility();
    document.addEventListener("visibilitychange", visibility);
    return () => document.removeEventListener("visibilitychange", visibility);
  }, []);
  useEffect(() => {
    setLaunchIndex(
      scenario === "running" ? 6 : scenario === "launch-error" ? 4 : 0,
    );
    setLaunchFailure(scenario === "launch-error");
  }, [scenario]);
  const minimizedSessions = useRef(new Set<string>());
  useEffect(() => {
    const session = runtime?.core.session;
    if (
      runtime?.core.phase !== "running" ||
      !session ||
      minimizedSessions.current.has(session.sessionId)
    )
      return;
    minimizedSessions.current.add(session.sessionId);
    try {
      if (localStorage.getItem("astral.v2.afterLaunch") === "minimize")
        void minimizeWindow().catch((cause) => setNotice(String(cause)));
    } catch {}
  }, [runtime?.core.phase, runtime?.core.session?.sessionId]);
  const activeNav = ["download", "game", "resource"].includes(page)
    ? "download"
    : ["versions", "home"].includes(page)
      ? "home"
      : page;
  return (
    <PreviewContext.Provider
      value={{
        runtime,
        prefs,
        setPrefs,
        navigate,
        versions,
        currentId,
        setCurrentId,
        scenario,
        setScenario,
        openAccount: () => setAccountOpen(true),
        openTasks: () => setTasksOpen(true),
        openDiagnostics: () =>
          runtime
            ? setDiagnosticsOpen(true)
            : setNotice("设计预览不读取或上传日志。"),
        notify: setNotice,
        launchIndex,
        setLaunchIndex,
        launchFailure,
        setLaunchFailure,
      }}
    >
      {!runtime && (
        <div className="review-bar">
          <span className="review-label">
            <i />
            设计预览 <b>示例档案 · 不读写游戏文件</b>
          </span>
          <nav aria-label="设计评审">
            <button
              aria-pressed={!["motion", "research"].includes(page)}
              onClick={() => navigate("home")}
            >
              界面
            </button>
            <button
              aria-pressed={page === "motion"}
              onClick={() => navigate("motion")}
            >
              三种启动动效
            </button>
            <button
              aria-pressed={page === "research"}
              onClick={() => navigate("research")}
            >
              研究与差距
            </button>
          </nav>
          <label>
            场景
            <select
              aria-label="验收场景"
              value={scenario}
              onChange={(event) => setScenario(event.target.value)}
            >
              {[
                ["normal", "正常预览"],
                ["empty", "无版本"],
                ["no-account", "无账号"],
                ["offline", "无网络"],
                ["download-error", "下载失败"],
                ["launch-error", "启动失败"],
                ["running", "游戏运行中"],
                ["conflict", "兼容性冲突"],
              ].map(([id, name]) => (
                <option key={id} value={id}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        </div>
      )}
      {runtime && !inTauri && (
        <div className="browser-note">
          浏览器预览 · 未连接本地核心；不生成游戏版本、会话或任务。
        </div>
      )}
      <div
        className={
          "window-frame layout-b" + (page === "home" ? " is-home" : "")
        }
      >
        <WindowBar />
        <aside className="sidebar">
          <a
            className="brand"
            href="#home"
            onClick={(event) => {
              event.preventDefault();
              navigate("home");
            }}
          >
            <span className="brand-mark">
              <Icon name="star" />
            </span>
            <span>
              Astral<small>LAUNCHER</small>
            </span>
          </a>
          <nav aria-label="主导航">
            {[
              ["home", "play", "启动"],
              ["download", "download", "下载"],
              ["settings", "settings", "设置"],
              ["more", "more", "更多"],
            ].map(([path, icon, label]) => (
              <button
                key={path}
                className={activeNav === path ? "active" : ""}
                aria-current={activeNav === path ? "page" : undefined}
                title={label}
                onClick={() =>
                  navigate(path === "download" ? "download/game" : path)
                }
              >
                <Icon name={icon} />
                <span>{label}</span>
                <i />
              </button>
            ))}
          </nav>
          <div className="sidebar-bottom">
            <button
              onClick={() =>
                runtime
                  ? setDiagnosticsOpen(true)
                  : setNotice(
                      "AI 诊断：先预览与脱敏日志；设计预览不读取或上传日志。",
                    )
              }
              title="AI 诊断"
            >
              <Icon name="star" />
              <span>AI 诊断</span>
            </button>
            <button onClick={() => setTasksOpen(true)} title="下载与任务">
              <Icon name="download" />
              <span>任务</span>
            </button>
            <button
              className="sidebar-account"
              onClick={() => setAccountOpen(true)}
              title="切换账号"
              aria-label="切换账号"
            >
              <span className="avatar">
                <Icon name="user" />
              </span>
            </button>
          </div>
        </aside>
        <div className="workspace">
          <Artwork />
          <main ref={main} id="main" className={`main page-${page}`}>
            {runtime?.error && (
              <div className="banner error" role="alert">
                暂时无法连接游戏服务：{runtime.error}
                <button onClick={() => void runtime.refresh()}>重新连接</button>
              </div>
            )}
            {!runtime && scenario === "offline" && (
              <div className="banner warn">
                <Icon name="warning" />
                网络不可用，正在展示已保存的公开清单快照。安装需恢复连接。
              </div>
            )}
            {page === "home" && <HomeB />}
            {page === "versions" && (runtime ? <LiveVersions /> : <Versions />)}
            {page === "download" &&
              (runtime ? (
                <LiveDownload category={argument || "game"} />
              ) : (
                <DownloadPage category={argument || "game"} />
              ))}
            {page === "game" &&
              (runtime ? (
                <LiveGameInstaller
                  key={argument}
                  version={decodeURIComponent(argument || "1.21.1")}
                />
              ) : (
                <GameInstaller
                  version={decodeURIComponent(argument || "1.21.1")}
                />
              ))}
            {page === "resource" &&
              (runtime ? (
                <LiveResource
                  key={argument}
                  argument={argument || "mod:sodium"}
                />
              ) : (
                <ResourceInstaller slug={argument || "sodium"} />
              ))}
            {page === "settings" && <Settings />}
            {page === "motion" && !runtime && <MotionLab />}
            {page === "research" && !runtime && <Research />}
            {page === "more" && (runtime ? <LiveMore /> : <More />)}
          </main>
        </div>
      </div>
      {diagnosticsOpen && (
        <LiveDiagnostics close={() => setDiagnosticsOpen(false)} />
      )}
      {accountOpen && <Account close={() => setAccountOpen(false)} />}
      {tasksOpen &&
        (runtime ? (
          <LiveTasks close={() => setTasksOpen(false)} />
        ) : (
          <Tasks close={() => setTasksOpen(false)} />
        ))}
      {notice && (
        <Modal
          title={runtime ? "操作说明" : "预览说明"}
          close={() => setNotice("")}
        >
          <p className="body-copy">{notice}</p>
          <button className="button primary full" onClick={() => setNotice("")}>
            知道了
          </button>
        </Modal>
      )}
    </PreviewContext.Provider>
  );
}

function Account({ close }: { close: () => void }) {
  const { scenario, runtime } = usePreview();
  const [tab, setTab] = useState("offline");
  const [username, setUsername] = useState(runtime?.config?.username || "");
  const [saving, setSaving] = useState(false);
  const [profileError, setProfileError] = useState("");
  async function saveProfile() {
    if (!runtime) return;
    setProfileError("");
    if (!/^[A-Za-z0-9_]{1,16}$/.test(username)) {
      setProfileError("请输入 1–16 位字母、数字或下划线。");
      return;
    }
    setSaving(true);
    try {
      if (runtime.mode === "core") await patchConfig({ username });
      else if (runtime.mode === "legacy")
        await setConfig({ ...(await getConfig()), username });
      else throw Error("浏览器预览未连接本地核心");
      await runtime.refresh();
      close();
    } catch (cause) {
      setProfileError(parseApiError(cause).message);
    } finally {
      setSaving(false);
    }
  }
  return (
    <Modal title="切换账号" close={close}>
      <div className="segmented">
        {[
          ["offline", "离线档案"],
          ["microsoft", "Microsoft 正版"],
        ].map(([id, name]) => (
          <button key={id} aria-pressed={tab === id} onClick={() => setTab(id)}>
            {name}
          </button>
        ))}
      </div>
      {tab === "offline" ? (
        <div className="account-detail">
          <span className="avatar large">
            <Icon name="user" />
          </span>
          <h3>
            {runtime
              ? runtime.config?.username || "尚未选择离线档案"
              : scenario === "no-account"
                ? "尚未选择离线档案"
                : "ForestWalker"}
          </h3>
          <p>离线档案 · 未验证 Microsoft 身份</p>
          <label className="field">
            离线用户名
            <input
              value={runtime ? username : undefined}
              onChange={(event) => setUsername(event.target.value)}
              defaultValue={
                runtime
                  ? undefined
                  : scenario === "no-account"
                    ? ""
                    : "ForestWalker"
              }
              placeholder="输入你的名字"
              maxLength={16}
            />
          </label>
          {profileError && (
            <p className="error-text" role="alert">
              {profileError}
            </p>
          )}
          <button
            className="button primary full"
            disabled={!!runtime && (saving || !inTauri)}
            onClick={runtime ? () => void saveProfile() : close}
          >
            {runtime ? "保存离线档案" : "返回预览"}
          </button>
        </div>
      ) : (
        <div className="account-detail">
          <Badge kind="user" large />
          <h3>Microsoft 未登录</h3>
          <p>
            授权功能暂不可用。接入官方授权后，这里才能显示经过验证的正版账号。
          </p>
          <button className="button primary full" disabled>
            登录 Microsoft
          </button>
        </div>
      )}
    </Modal>
  );
}

function Settings() {
  const { prefs, setPrefs, openAccount, runtime } = usePreview();
  const patch = (key: keyof Preferences, value: unknown) => {
    if (key === "theme" || (key === "randomOnStartup" && value)) {
      const url = new URL(location.href);
      url.searchParams.delete("theme");
      history.replaceState(null, "", url);
    }
    setPrefs((old) => ({ ...old, [key]: value }));
  };
  return (
    <section className="content-page" data-page="settings">
      <PageTitle title="设置" subtitle="让 Astral 更合你的习惯。" />
      <div className="settings-layout">
        <nav aria-label="设置分类">
          <button
            onClick={() =>
              document
                .getElementById("appearance")
                ?.scrollIntoView({ behavior: "instant" })
            }
          >
            外观与动效
          </button>
          {[
            ["runtime-settings", "游戏与 Java"],
            ["network-settings", "下载与网络"],
            ["account-settings", "账号与高级"],
          ].map(([id, label]) => (
            <button
              key={id}
              onClick={() =>
                document
                  .getElementById(id)
                  ?.scrollIntoView({ behavior: "instant" })
              }
            >
              {label}
            </button>
          ))}
        </nav>
        <div className="settings-content">
          <section id="appearance">
            <h2>三片森林，同一个 Astral</h2>
            <p className="muted">只改变氛围，熟悉的操作始终在原位。</p>
            <div className="setting-row">
              <div>
                <strong>打开时随机切换森林</strong>
                <p>
                  三套背景和配色全部保留；每次打开随机选择，不连续重复。页面切换不变。关闭后固定当前主题，缩略图可手动预览。
                </p>
              </div>
              <Toggle
                checked={prefs.randomOnStartup}
                onChange={() =>
                  patch("randomOnStartup", !prefs.randomOnStartup)
                }
                label="打开时随机切换森林"
              />
            </div>
            <div className="theme-grid">
              {THEMES.map((theme) => (
                <button
                  key={theme.id}
                  className={`theme-card ${prefs.theme === theme.id ? "selected" : ""}`}
                  aria-pressed={prefs.theme === theme.id}
                  onClick={() => patch("theme", theme.id)}
                >
                  <img
                    src={theme.image}
                    alt={theme.name}
                    loading="lazy"
                    decoding="async"
                    onError={(event) => {
                      event.currentTarget.style.opacity = "0";
                    }}
                  />
                  <div>
                    <strong>{theme.name}</strong>
                    {prefs.theme === theme.id && <Icon name="check" />}
                    <small>{theme.subtitle}</small>
                  </div>
                </button>
              ))}
            </div>
            <div className="setting-row">
              <div>
                <strong>深浅模式</strong>
                <p>为每片森林选择适合当前光线的明暗。</p>
              </div>
              <div className="segmented">
                <button
                  aria-pressed={prefs.mode === "light"}
                  onClick={() => patch("mode", "light")}
                >
                  <Icon name="shaderpack" />
                  浅色
                </button>
                <button
                  aria-pressed={prefs.mode === "dark"}
                  onClick={() => patch("mode", "dark")}
                >
                  <Icon name="moon" />
                  深色
                </button>
              </div>
            </div>
            {[
              ["wallpaper", "森林壁纸", "关闭后保留主题配色，界面更纯粹。"],
              ["motion", "界面动画", "系统的减少动态效果偏好始终优先。"],
              [
                "intro",
                "打开时的轻柔入场",
                "约 500ms；不阻挡点击，不额外等待。",
              ],
              ["eco", "节能动效", "减少粒子与循环效果；适合低性能设备。"],
            ].map(([key, title, description]) => (
              <div className="setting-row" key={key}>
                <div>
                  <strong>{title}</strong>
                  <p>{description}</p>
                </div>
                <Toggle
                  checked={!!prefs[key as keyof Preferences]}
                  onChange={() =>
                    patch(
                      key as keyof Preferences,
                      !prefs[key as keyof Preferences],
                    )
                  }
                  label={title}
                />
              </div>
            ))}
          </section>
          {runtime ? (
            <LiveSettings />
          ) : (
            <>
              {" "}
              <section>
                <h2>游戏与 Java</h2>
                <div className="setting-row">
                  <div>
                    <strong>Java 运行环境</strong>
                    <p>自动准备所需 Java；手动选择待核心支持。</p>
                  </div>
                  <button className="button secondary" disabled>
                    自动管理
                  </button>
                </div>
                <div className="setting-row">
                  <div>
                    <strong>游戏内存与 JVM 参数</strong>
                    <p>按游戏版本分别配置，避免影响其他世界。</p>
                  </div>
                  <button
                    className="button secondary"
                    onClick={() => useNavigationHint()}
                  >
                    在版本管理中设置
                  </button>
                </div>
                <div className="setting-row">
                  <div>
                    <strong>启动游戏后</strong>
                    <p>关闭或最小化启动器需要配套进程与托盘策略。</p>
                  </div>
                  <select disabled aria-label="启动后行为">
                    <option>保持打开 · 待核心支持其他行为</option>
                  </select>
                </div>
              </section>
              <section>
                <h2>下载与网络</h2>
                <div className="setting-row">
                  <div>
                    <strong>下载源</strong>
                    <p>镜像只改变获取地址，仍需校验原始文件。</p>
                  </div>
                  <select aria-label="预览下载源" defaultValue="bmclapi">
                    <option value="bmclapi">BMCLAPI 镜像</option>
                    <option value="official">官方</option>
                  </select>
                </div>
                <div className="setting-row">
                  <div>
                    <strong>并发下载与任务恢复</strong>
                    <p>设置接口及持久化队列待核心补齐。</p>
                  </div>
                  <button className="button secondary" disabled>
                    暂不可配置
                  </button>
                </div>
              </section>
              <section>
                <h2>账号与高级</h2>
                <div className="setting-row">
                  <div>
                    <strong>账号管理</strong>
                    <p>离线档案与 Microsoft 授权状态。</p>
                  </div>
                  <button className="button secondary" onClick={openAccount}>
                    管理账号
                  </button>
                </div>
                <div className="setting-row">
                  <div>
                    <strong>日志与诊断</strong>
                    <p>分享前预览、脱敏并确认。</p>
                  </div>
                  <span className="tag">远程分析未接入</span>
                </div>
              </section>
            </>
          )}{" "}
        </div>
      </div>
    </section>
  );
  function useNavigationHint() {
    location.hash = "versions";
  }
}

function Versions() {
  const { versions, currentId, setCurrentId, navigate, notify } = usePreview();
  const [selected, setSelected] = useState(currentId);
  const [tab, setTab] = useState("overview");
  const [query, setQuery] = useState("");
  const current =
    versions.find((version) => version.id === selected) || versions[0];
  return (
    <section className="content-page" data-page="versions">
      <button className="breadcrumb" onClick={() => navigate("home")}>
        <Icon name="back" />
        启动 / 我的版本
      </button>
      <PageTitle
        title="版本管理"
        subtitle="每个世界的 Mod、存档与配置，放在一起管理。"
      >
        <button
          className="button primary"
          onClick={() => navigate("download/game")}
        >
          <Icon name="download" />
          下载游戏
        </button>
      </PageTitle>
      {!versions.length ? (
        <Empty
          title="还没有游戏版本"
          detail="一般玩家只需下载 Minecraft，Astral 会为它建立独立游戏配置。"
        />
      ) : (
        <div className="version-manager">
          <aside>
            <label className="search">
              <Icon name="search" />
              <input
                aria-label="搜索我的版本"
                placeholder="搜索我的版本"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
            {versions
              .filter((version) =>
                `${version.name} ${version.gameVersion}`.includes(query),
              )
              .map((version) => (
                <button
                  key={version.id}
                  className={current?.id === version.id ? "active" : ""}
                  onClick={() => setSelected(version.id)}
                >
                  <Badge kind={version.loader} />
                  <span>
                    <strong>{version.name}</strong>
                    <small>
                      {version.gameVersion} · {version.loader}
                    </small>
                  </span>
                </button>
              ))}
          </aside>
          <div className="version-detail">
            <header>
              <Badge kind={current.loader} large />
              <div>
                <h2>{current.name}</h2>
                <p>
                  {current.gameVersion} · {current.loader}{" "}
                  {current.loaderVersion}
                </p>
              </div>
              <button
                className="button primary"
                onClick={() => {
                  setCurrentId(current.id);
                  navigate("motion");
                }}
              >
                <Icon name="play" />
                启动
              </button>
            </header>
            <div className="tabs" role="tablist" aria-label="版本设置">
              {[
                ["overview", "总览"],
                ["components", "安装组件"],
                ["mods", "Mod"],
                ["saves", "存档与资源"],
                ["runtime", "运行设置"],
                ["logs", "日志"],
              ].map(([id, name]) => (
                <button
                  key={id}
                  role="tab"
                  aria-selected={tab === id}
                  onClick={() => setTab(id)}
                >
                  {name}
                </button>
              ))}
            </div>
            {current.shared && (
              <div className="banner warn">
                <Icon name="warning" />
                此版本使用共享游戏目录。为了保护其他版本，不提供改名、删除或目录内写入。
              </div>
            )}
            {tab === "overview" && (
              <>
                <div className="detail-grid">
                  <div>
                    <span>游戏版本</span>
                    <strong>{current.gameVersion}</strong>
                  </div>
                  <div>
                    <span>加载器</span>
                    <strong>
                      {current.loader === "game" ? "纯净原版" : current.loader}
                    </strong>
                  </div>
                  <div>
                    <span>Mod 数量</span>
                    <strong>
                      {current.mods} <small>个 · 示例</small>
                    </strong>
                  </div>
                  <div>
                    <span>文件位置</span>
                    <strong>
                      {current.shared ? "共享游戏目录" : "独立游戏目录"}
                    </strong>
                  </div>
                </div>
                <div className="banner">
                  <Icon name="folder" />
                  {current.shared
                    ? "此目录里的存档和配置会被其他共享目录版本使用。"
                    : "这个版本的 mods、config、saves 与其他独立版本分开保存。"}
                </div>
                <div className="action-grid">
                  <button
                    className="button secondary"
                    onClick={() =>
                      notify(
                        "预览不会打开真实目录。正式实现只使用 Rust 返回的当前版本路径。",
                      )
                    }
                  >
                    打开游戏目录
                  </button>
                  <button
                    className="button secondary"
                    disabled={current.shared}
                    onClick={() =>
                      notify(
                        "名称编辑会仅提交变化字段；此处为设计评审，不修改真实版本。",
                      )
                    }
                  >
                    编辑名称
                  </button>
                  <button
                    className="button secondary"
                    disabled
                    title="核心尚未提供复制命令"
                  >
                    复制配置 · 待支持
                  </button>
                  <button
                    className="button danger"
                    disabled={current.shared}
                    onClick={() =>
                      notify(
                        "删除前须显示版本名和目录，二次确认；默认移入应用回收站，运行中或任务中拒绝删除。此预览不删除文件。",
                      )
                    }
                  >
                    删除与数据保护
                  </button>
                </div>
              </>
            )}
            {tab === "components" && (
              <>
                <h3>游戏组件</h3>
                <div className="file-row">
                  <Badge kind={current.loader} />
                  <span>
                    <strong>
                      {current.loader === "game"
                        ? "Minecraft 原版"
                        : current.loader}
                    </strong>
                    <small>
                      {current.loaderVersion || current.gameVersion}
                    </small>
                  </span>
                  <button
                    className="button secondary"
                    disabled={current.shared}
                    onClick={() => navigate(`game/${current.gameVersion}`)}
                  >
                    安装组件
                  </button>
                </div>
              </>
            )}
            {tab === "mods" && (
              <>
                <header className="section-heading">
                  <h3>当前版本的 Mod</h3>
                  <button
                    className="text-button"
                    onClick={() => navigate("download/mod")}
                  >
                    获取 Mod
                    <Icon name="arrow" />
                  </button>
                </header>
                {current.shared ? (
                  <p className="muted">共享目录版本不提供 Mod 写入操作。</p>
                ) : (
                  <>
                    <div className="file-row">
                      <Badge kind="mod" />
                      <span>
                        <strong>Sodium</strong>
                        <small>示例条目 · 具体文件版本以扫描结果为准</small>
                      </span>
                      <span className="tag">启用</span>
                    </div>
                    <div className="file-row">
                      <Badge kind="api" />
                      <span>
                        <strong>Fabric API</strong>
                        <small>示例条目 · 不代表真实已安装</small>
                      </span>
                      <span className="tag">启用</span>
                    </div>
                  </>
                )}
              </>
            )}
            {tab === "saves" && (
              <div className="action-grid">
                {["存档", "资源包", "光影包", "截图"].map((name) => (
                  <button
                    key={name}
                    className="resource-folder"
                    onClick={() =>
                      notify(
                        `${name}管理会使用当前版本的 Rust 返回目录；预览不读取真实文件。`,
                      )
                    }
                  >
                    <Icon name="folder" />
                    <strong>{name}</strong>
                    <Icon name="arrow" />
                  </button>
                ))}
              </div>
            )}
            {tab === "runtime" && (
              <>
                <h3>只为这个版本设置</h3>
                <label className="field">
                  Java
                  <input readOnly value="自动管理（手动 Java 路径暂未生效）" />
                </label>
                <label className="field">
                  游戏内存
                  <select defaultValue="auto">
                    <option value="auto">自动分配</option>
                    <option>4096 MB · 示例</option>
                    <option>6144 MB · 示例</option>
                  </select>
                </label>
                <label className="field">
                  JVM 参数
                  <textarea placeholder="留空使用默认参数；示例编辑不写入游戏配置" />
                </label>
                <button
                  className="button secondary"
                  onClick={() =>
                    notify(
                      "这里仅预览编辑表单；正式阶段使用 update_instance，只提交变化字段。",
                    )
                  }
                >
                  预览保存说明
                </button>
              </>
            )}
            {tab === "logs" && (
              <Empty
                title="日志由你选择分享"
                detail="读取日志、脱敏预览和导出后续接入；预览不会读取你的游戏日志。"
              />
            )}
          </div>
        </div>
      )}
    </section>
  );
}

export function Effect({
  variant,
  active,
  phase,
}: {
  variant: string;
  active: boolean;
  phase: string;
}) {
  return (
    <div
      className={`launch-effect effect-${variant} ${active ? "active" : ""}`}
      data-phase={phase}
      aria-hidden="true"
    >
      <div className="effect-horizon" />
      <div className="effect-emblem">
        <Icon name={variant === "voxel" ? "game" : "star"} />
      </div>
      {Array.from(
        { length: variant === "fireflies" ? 8 : variant === "voxel" ? 6 : 2 },
        (_, index) => (
          <i key={index} style={{ "--i": index } as React.CSSProperties} />
        ),
      )}
    </div>
  );
}
function MotionLab() {
  const {
    launchIndex,
    setLaunchIndex,
    launchFailure,
    setLaunchFailure,
    prefs,
    setPrefs,
    notify,
  } = usePreview();
  const state = launchState(launchIndex, launchFailure);
  const [choice, setChoice] = useState("");
  const [copied, setCopied] = useState(false);
  return (
    <section className="content-page motion-page" data-page="motion">
      <PageTitle
        title="一次出发，三种心情。"
        subtitle="三个原型共用同一组 Task / GameSession 契约格式事件；手动推进，不按时间伪造成功。"
      >
        <button
          className="button secondary"
          onClick={() => setPrefs((old) => ({ ...old, motion: !old.motion }))}
        >
          {prefs.motion ? "关闭动画" : "开启动画"}
        </button>
      </PageTitle>
      <div className="motion-controls">
        <label>
          当前事件
          <select
            aria-label="启动事件"
            value={launchFailure ? "error" : launchIndex}
            onChange={(event) => {
              setLaunchFailure(event.target.value === "error");
              if (event.target.value !== "error")
                setLaunchIndex(Number(event.target.value));
            }}
          >
            {LAUNCH_STEPS.map((step, index) => (
              <option key={step.id} value={index}>
                {index}. {step.name}
              </option>
            ))}
            <option value="error">启动失败</option>
          </select>
        </label>
        <button
          className="button primary"
          disabled={launchIndex >= 7 && !launchFailure}
          onClick={() => {
            setLaunchFailure(false);
            setLaunchIndex(Math.min(launchIndex + 1, 7));
          }}
        >
          下一事件
          <Icon name="arrow" />
        </button>
        <button
          className="button secondary"
          onClick={() => {
            setLaunchIndex(4);
            setLaunchFailure(true);
          }}
        >
          模拟启动失败
        </button>
        <button
          className="icon-button"
          aria-label="重置启动演示"
          onClick={() => {
            setLaunchIndex(0);
            setLaunchFailure(false);
          }}
        >
          <Icon name="refresh" />
        </button>
      </div>
      <div className="motion-grid">
        {[
          ["fireflies", "01", "森林萤火", "少量萤火，轻轻托住一次出发。"],
          ["voxel", "02", "像素方块组合", "方块聚合，呼应熟悉的游戏语言。"],
          ["light", "03", "极简光影过渡", "只保留光线与呼吸，最轻也最安静。"],
        ].map(([id, number, title, description]) => (
          <article
            key={id}
            className={`motion-card ${choice === id ? "chosen" : ""}`}
            data-variant={id}
            data-state={state.id}
          >
            <header>
              <span className="kicker">DIRECTION {number}</span>
              <h2>{title}</h2>
              <p>{description}</p>
            </header>
            <Effect variant={id} active={state.active} phase={state.id} />
            <div
              className={`mock-preparation ${state.id === "idle" ? "is-idle" : ""} ${launchFailure ? "failed" : ""}`}
            >
              <div>
                <Badge kind={launchFailure ? "warning" : "fabric"} />
                <span>
                  <strong>{state.name}</strong>
                  <small>林间小径 · 1.21.1</small>
                </span>
              </div>
              <p>{state.detail}</p>
              {state.task && !launchFailure && state.active && (
                <progress
                  aria-label={`${title}当前阶段进度`}
                  max={state.task.total > 0 ? state.task.total : undefined}
                  value={state.task.total > 0 ? state.task.done : undefined}
                />
              )}
              {launchFailure && (
                <div className="failure-actions">
                  <button
                    onClick={async () => {
                      try {
                        await navigator.clipboard.writeText(state.detail);
                        setCopied(true);
                      } catch {
                        notify(state.detail);
                      }
                    }}
                  >
                    {copied ? "示例错误已复制" : "复制错误"}
                  </button>
                  <button
                    onClick={() =>
                      notify(
                        "诊断前先预览与脱敏日志。设计原型不上传或分析日志。",
                      )
                    }
                  >
                    诊断入口
                  </button>
                </div>
              )}
              {state.id === "idle" && (
                <button
                  className="button primary proto-launch"
                  onClick={() => {
                    setLaunchIndex(1);
                    setLaunchFailure(false);
                  }}
                >
                  <Icon name="play" />
                  预览启动
                  <Icon name="arrow" />
                </button>
              )}
            </div>
            <button
              className={`button ${choice === id ? "primary" : "secondary"} full`}
              onClick={() => setChoice(id)}
            >
              {choice === id ? "当前偏好 · 尚未正式接入" : "标记为我的偏好"}
            </button>
          </article>
        ))}
      </div>
      <div className="motion-footnotes">
        <div>
          <Icon name="check" />
          <span>
            <strong>所有状态可静态辨识</strong>
            <small>
              关闭动画、系统减少动态效果和节能模式都有完整文字与图标。
            </small>
          </span>
        </div>
        <div>
          <Icon name="warning" />
          <span>
            <strong>进程运行 ≠ 游戏窗口就绪</strong>
            <small>
              当前核心只能确认会话进程；窗口或主菜单就绪需要额外信号。
            </small>
          </span>
        </div>
      </div>
      <details className="trace">
        <summary>查看当前示例事件（仅评审）</summary>
        <pre>
          {JSON.stringify(
            { task: state.task, session: state.session },
            null,
            2,
          )}
        </pre>
      </details>
    </section>
  );
}

function Tasks({ close }: { close: () => void }) {
  const { scenario } = usePreview();
  const [stage, setStage] = useState(
    scenario === "download-error" ? "error" : "download",
  );
  const stages = [
    ["queued", "排队中"],
    ["download", "下载中"],
    ["assets", "校验中"],
    ["extract", "安装中"],
    ["done", "完成"],
    ["error", "失败"],
  ];
  return (
    <Modal title="下载与任务 · 状态演示" close={close} wide>
      <p className="muted">手动切换示例事件。此处没有真实下载任务。</p>
      <div className="segmented wrap">
        {stages.map(([id, title]) => (
          <button
            key={id}
            aria-pressed={stage === id}
            onClick={() => setStage(id)}
          >
            {title}
          </button>
        ))}
      </div>
      <article className="task-item">
        <Badge kind="game" />
        <div>
          <h3>Minecraft 1.21.1</h3>
          <p>
            {stages.find((item) => item[0] === stage)?.[1]} ·{" "}
            {stage === "download"
              ? "依赖文件 21/61"
              : stage === "error"
                ? "NETWORK · 示例网络中断"
                : "当前任务阶段"}
          </p>
          {!["queued", "done", "error"].includes(stage) && (
            <progress
              max={stage === "download" ? 61 : undefined}
              value={stage === "download" ? 21 : undefined}
              aria-label="当前下载阶段进度"
            />
          )}
          {stage === "done" && (
            <span className="tag">示例完成事件 · 非真实安装</span>
          )}
        </div>
      </article>
      {stage === "error" && (
        <div className="banner error">
          <Icon name="warning" />
          失败详情保留。真正重试必须由核心返回新任务 ID，不能只清除这条错误。
        </div>
      )}
      <p className="muted">
        取消暂不可用：后台任务尚不保证停止文件写入。核实后才开放取消。
      </p>
    </Modal>
  );
}
function Research() {
  return (
    <section className="content-page research-page" data-page="research">
      <PageTitle
        title="学会组织，不借用外观。"
        subtitle="2026-10-24 · 六个项目的 README、许可证与相关界面源码选读；未移植代码、图标或品牌素材。"
      />
      <div className="research-intro">
        <strong>采用一套结构，保留三片森林。</strong>
        <p>
          四项主导航、启动区邻接账号、两条不同的安装路径、可解释的兼容性。Ast​ral
          仍使用自己的 React / Tauri / Rust 架构。
        </p>
      </div>
      <div className="research-list">
        {RESEARCH.map((project, index) => (
          <article key={project.name}>
            <span className="research-number">0{index + 1}</span>
            <div>
              <header>
                <h2>{project.name}</h2>
                <span className="tag">{project.license}</span>
              </header>
              <p>
                <b>观察</b>
                {project.learn}
              </p>
              <p>
                <b>Astral</b>
                {project.adopt}
              </p>
              <a href={project.file} target="_blank" rel="noreferrer">
                查看选读的原始界面文件
                <Icon name="arrow" />
              </a>
            </div>
          </article>
        ))}
      </div>
      <section className="gap-summary">
        <h2>先补真实能力，再点亮按钮。</h2>
        <div>
          <p>
            <strong>现有可复用</strong>实例查询与选择、Task
            revision、会话与退出、模组 CRUD、Fabric 构建目录。
          </p>
          <p>
            <strong>必须补齐</strong>
            安装计划与自动创建版本、实例定向资源安装、依赖与冲突校验、存档级数据包安装、账号与窗口就绪信号。
          </p>
          <p>
            <strong>下一步</strong>确认布局、主题和一种启动动效后，再进入正式
            React 组件与接口接入。
          </p>
        </div>
      </section>
    </section>
  );
}
function More() {
  const { navigate, notify } = usePreview();
  return (
    <section className="content-page">
      <PageTitle title="更多" subtitle="工具、帮助和关于 Astral。" />
      <div className="more-grid">
        {[
          ["log", "日志与诊断", "先看清问题，再决定分享什么。"],
          ["folder", "导入已有版本", "保留原目录，先预览迁移计划。"],
          ["settings", "高级工具", "手动配置版本、目录和运行参数。"],
          ["star", "关于 Astral", "原创森林主题，清晰而可靠。"],
        ].map(([icon, title, text]) => (
          <button
            key={title}
            onClick={() =>
              notify(`${title}：这是设计入口，未执行文件或核心操作。`)
            }
          >
            <Badge kind={icon} />
            <h2>{title}</h2>
            <p>{text}</p>
            <Icon name="arrow" />
          </button>
        ))}
      </div>
      <button className="text-button" onClick={() => navigate("research")}>
        阅读本轮设计研究
        <Icon name="arrow" />
      </button>
    </section>
  );
}
