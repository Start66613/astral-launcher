import { useState } from "react";
import { Badge, Icon } from "./icons";
import { THEMES, launchState } from "./model";
import { usePreview } from "./state";
import { Modal } from "./App";
import { LiveLaunch } from "./LiveRuntime";
import {
  closeWindow,
  inTauri,
  minimizeWindow,
  toggleMaximize,
} from "../lib/bridge";

export function WindowBar() {
  const { prefs, setPrefs, runtime, notify } = usePreview();
  const windowAction = (action: () => Promise<void>) =>
    void action().catch((cause) => notify(String(cause)));
  return (
    <header className="window-bar" data-tauri-drag-region>
      <span className="window-brand" data-tauri-drag-region>
        <Icon name="star" />
        Astral Launcher
      </span>
      <div>
        <button
          className="icon-button"
          aria-label="切换深浅模式"
          onClick={() =>
            setPrefs((old) => ({
              ...old,
              mode: old.mode === "dark" ? "light" : "dark",
            }))
          }
        >
          <Icon name={prefs.mode === "dark" ? "shaderpack" : "moon"} />
        </button>
        <span className="chrome-line" />
        <button
          className="chrome-control"
          disabled={!runtime || !inTauri}
          onClick={() => windowAction(minimizeWindow)}
          aria-label="最小化窗口"
        >
          —
        </button>
        <button
          className="chrome-control"
          disabled={!runtime || !inTauri}
          onClick={() => windowAction(toggleMaximize)}
          aria-label="最大化窗口"
        >
          □
        </button>
        <button
          className="chrome-control"
          disabled={!runtime || !inTauri}
          onClick={() => windowAction(closeWindow)}
          aria-label="关闭窗口"
        >
          ×
        </button>
      </div>
    </header>
  );
}

/** The original B composition. Theme selection changes artwork and tokens only. */
export function HomeB() {
  const {
    versions,
    currentId,
    setCurrentId,
    navigate,
    scenario,
    openDiagnostics,
    openAccount,
    launchIndex,
    launchFailure,
    notify,
    runtime,
  } = usePreview();
  const [picker, setPicker] = useState(false);
  const current = versions.find((item) => item.id === currentId);
  const state = launchState(launchIndex, launchFailure);
  const otherVersions = versions
    .filter((item) => item.id !== currentId)
    .slice(0, 2);
  return (
    <section
      className="home-layout home-layout-b"
      data-page="home"
      data-layout="original-b"
    >
      <div className="b-kicker">
        <span className="kicker">
          <Icon name="moon" />
          YOUR NEXT ADVENTURE
        </span>
        <button className="quiet-button" onClick={() => navigate("versions")}>
          版本管理
          <Icon name="arrow" />
        </button>
      </div>
      <section className="b-hero">
        <div className="b-hero-copy">
          <span className="b-overline">慢一点，世界会等你。</span>
          <h1 tabIndex={-1}>
            此刻，
            <br />
            住进一片森林。
          </h1>
          <p>
            树影落在窗前，故事留在远方。
            <br />
            去一个，只属于你的地方。
          </p>
          <button
            className="b-hero-link"
            onClick={() => navigate("download/game")}
          >
            下载游戏，开始新旅程
            <Icon name="arrow" />
          </button>
        </div>
        <aside className="b-margin-note">
          <span>01 / THE QUIET WORLD</span>
          <i />
          <p>
            在喧嚣之外
            <br />
            给想象留一盏灯
          </p>
        </aside>
      </section>
      <div className="b-bottom">
        <section
          className={`launch-panel b-launch-dock ${launchFailure ? "has-error" : ""}`}
          aria-label="游戏启动区域"
        >
          <header className="b-dock-caption">
            <span className="kicker">YOUR NEXT ADVENTURE</span>
            <span>
              当前版本
              <button onClick={() => navigate("versions")}>
                管理
                <Icon name="arrow" />
              </button>
            </span>
          </header>
          <div className="b-dock-row">
            <div className="current-version">
              <Badge kind={current?.loader || "game"} large />
              <div>
                <h2>{current?.name || "还没有游戏版本"}</h2>
                <p>
                  {current
                    ? `${current.gameVersion} · ${current.loader === "game" ? "纯净原版" : `${current.loader} ${current.loaderVersion}`}`
                    : "下载 Minecraft，开始第一段旅程。"}
                </p>
              </div>
            </div>
            <button
              className="quiet-button b-switch"
              disabled={!versions.length}
              onClick={() => setPicker(true)}
            >
              切换
              <Icon name="chevron" />
            </button>
            <div className="launch-actions">
              <button className="account-control" onClick={openAccount}>
                <span className="avatar">
                  <Icon name="user" />
                </span>
                <span>
                  <strong>
                    {runtime
                      ? runtime.config?.username || "选择账号"
                      : scenario === "no-account"
                        ? "选择账号"
                        : "ForestWalker"}
                  </strong>
                  <small>
                    {(runtime && !runtime.config?.username) ||
                    (!runtime && scenario === "no-account")
                      ? "未登录"
                      : "离线档案"}
                  </small>
                </span>
                <Icon name="chevron" />
              </button>
              <button
                className="button launch-primary"
                disabled={
                  !current ||
                  (runtime
                    ? runtime.mode === "core"
                      ? !runtime.core.canLaunch
                      : runtime.mode !== "legacy" || runtime.legacy.launchBusy
                    : scenario === "running")
                }
                onClick={() => {
                  if (runtime) {
                    if (!runtime.config?.username) openAccount();
                    else if (runtime.mode === "core")
                      void runtime.core.launch();
                    else if (runtime.mode === "legacy")
                      void runtime.legacy.launch();
                  } else
                    scenario === "no-account"
                      ? openAccount()
                      : navigate("motion");
                }}
              >
                <Icon name="play" />
                <span>
                  {runtime
                    ? runtime.core.phase === "running"
                      ? "游戏进程运行中"
                      : runtime.launchBusy
                        ? "准备游戏中"
                        : "启动游戏"
                    : scenario === "running"
                      ? "游戏进程运行中"
                      : "启动游戏"}
                </span>
                <Icon name="arrow" />
              </button>
            </div>
          </div>
          {!runtime && launchIndex > 0 && (
            <div
              className={`inline-launch-status ${launchFailure ? "error-text" : ""}`}
              role="status"
            >
              <span className="status-dot" />
              {state.name}
              <span>{state.detail}</span>
              {launchFailure && (
                <>
                  <button
                    onClick={() => notify(`示例错误详情：${state.detail}`)}
                  >
                    错误详情
                  </button>
                  <button
                    onClick={() =>
                      notify(
                        "诊断前先预览并脱敏日志；此设计预览不读取或上传日志。",
                      )
                    }
                  >
                    诊断
                  </button>
                </>
              )}
            </div>
          )}
          {runtime && <LiveLaunch />}
        </section>
        <section className="recent-section b-worlds">
          <header className="section-heading">
            <div>
              <h2>换一个世界</h2>
              <span>最近使用</span>
            </div>
            <button
              className="text-button"
              onClick={() => navigate("versions")}
            >
              我的版本
              <Icon name="arrow" />
            </button>
          </header>
          <div className="b-world-grid">
            {otherVersions.map((version, index) => (
              <button
                key={version.id}
                className="recent-card b-world-card"
                onClick={() => setCurrentId(version.id)}
                aria-pressed={version.id === currentId}
              >
                <img
                  src={THEMES[index === 0 ? 0 : 2].image}
                  alt=""
                  decoding="async"
                  onError={(event) => {
                    event.currentTarget.hidden = true;
                  }}
                />
                <div>
                  <strong>{version.name}</strong>
                  <span>
                    {version.gameVersion} ·{" "}
                    {version.loader === "game" ? "原版" : version.loader}
                  </span>
                </div>
                <Icon name="arrow" />
              </button>
            ))}
            {!versions.length && (
              <button
                className="b-empty-world"
                onClick={() => navigate("download/game")}
              >
                <Badge kind="download" />
                <strong>下载第一个游戏版本</strong>
                <small>为新世界建立独立游戏目录</small>
              </button>
            )}
            <button className="b-ai-card" onClick={openDiagnostics}>
              <Icon name="star" />
              <strong>
                遇到问题时
                <br />
                我在这里。
              </strong>
              <span>
                AI 诊断
                <Icon name="arrow" />
              </span>
            </button>
          </div>
        </section>
      </div>
      {picker && (
        <Modal title="选择当前版本" close={() => setPicker(false)}>
          <div className="choice-list">
            {versions.map((version) => (
              <button
                key={version.id}
                aria-pressed={currentId === version.id}
                onClick={() => {
                  setCurrentId(version.id);
                  setPicker(false);
                }}
              >
                <Badge kind={version.loader} />
                <span>
                  <strong>{version.name}</strong>
                  <small>
                    {version.gameVersion} · {version.loader}{" "}
                    {version.loaderVersion}
                  </small>
                </span>
                {currentId === version.id && <Icon name="check" />}
              </button>
            ))}
          </div>
        </Modal>
      )}
    </section>
  );
}
