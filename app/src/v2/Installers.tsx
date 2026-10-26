import { useEffect, useRef, useState, type ReactNode } from "react";
import { parseApiError } from "../lib/bridge";
import type { Task } from "../lib/types";
import { Badge, Icon } from "./icons";
import { Empty } from "./App";
import { TaskView } from "./LiveRuntime";
import { MotionDetails, MotionRegion } from "./motion";
import { usePreview } from "./state";
import { date, size, CATEGORIES } from "./model";
import { MODRINTH, publicJson, waitForTask } from "./live-api";
import * as api from "./core-api";

function useInstallPlan<P extends api.Plan>(
  signature: string,
  prepare: () => Promise<P>,
  start: (plan: P) => Promise<Task>,
) {
  const { runtime } = usePreview();
  const [plan, setPlan] = useState<P | null>(null),
    [task, setTask] = useState<Task | null>(null);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [complete, setComplete] = useState("");
  const lock = useRef(false),
    key = useRef(signature);
  key.current = signature;
  useEffect(() => {
    setPlan(null);
    setError("");
    setComplete("");
  }, [signature]);
  async function run(confirm: boolean) {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError("");
    setComplete("");
    const ticket = signature;
    try {
      if (!confirm) {
        const value = await prepare();
        if (ticket === key.current) setPlan(value);
      } else {
        if (!plan || ticket !== key.current) return;
        if (plan.issues.some((i) => i.blocking))
          throw Error("请先解决计划中的阻塞问题");
        if (Date.parse(plan.expiresAt) <= Date.now()) {
          setPlan(null);
          throw Error("计划已过期，请重新检查安装计划。");
        }
        const submitted = await start(plan);
        setTask(submitted);
        setPlan(null);
        const done = await waitForTask(submitted, setTask);
        await runtime?.core.refresh();
        const result = done.result as unknown as {
          instance?: { id: string; name: string };
        } | null;
        if (result?.instance) await runtime?.core.select(result.instance.id);
        setComplete(
          api.cancelledLate(done)
            ? "取消请求到达前已完成；请检查安装结果。"
            : "核心已确认安装完成。可在我的版本查看结果。",
        );
      }
    } catch (cause) {
      const e = parseApiError(cause);
      if (String(e.code) === "PLAN_EXPIRED") setPlan(null);
      setError(`${e.code} · ${e.message}${e.detail ? `\n${e.detail}` : ""}`);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  return { plan, task, busy, error, complete, run };
}
type Operation<P extends api.Plan> = ReturnType<typeof useInstallPlan<P>>;
function PlanReview({
  plan,
  children,
}: {
  plan: api.Plan;
  children?: ReactNode;
}) {
  return (
    <section
      className="surface component-section plan-review"
      aria-label="真实安装计划"
    >
      <header>
        <span className="step-number">✓</span>
        <div>
          <h2>安装前检查</h2>
          <p>
            本地核心返回的计划 ·{" "}
            {plan.downloadBytes === null
              ? "下载总量暂不可计算"
              : size(plan.downloadBytes)}
          </p>
        </div>
      </header>
      {children}
      {plan.issues.length ? (
        plan.issues.map((issue, i) => (
          <div
            className={`banner ${issue.blocking ? "error" : "warn"}`}
            key={i}
          >
            <strong>
              {issue.blocking ? "阻止安装" : "提示"} · {issue.code}
            </strong>
            <span>{issue.message}</span>
          </div>
        ))
      ) : (
        <p>核心预检未报告阻塞问题；执行时仍会校验文件与目标目录。</p>
      )}
      <small className="muted">
        有效至 {new Date(plan.expiresAt).toLocaleTimeString()} ·
        修改选择后需要重新规划。
      </small>
    </section>
  );
}
function Result<P extends api.Plan>({ op }: { op: Operation<P> }) {
  const { navigate } = usePreview();
  return (
    <>
      {op.task && <TaskView task={op.task} />}
      {op.error && (
        <pre className="banner error" role="alert">
          {op.error}
        </pre>
      )}
      {op.complete && (
        <div className="banner" role="status">
          {op.complete}
          <button onClick={() => navigate("versions")}>查看我的版本</button>
        </div>
      )}
    </>
  );
}
function Footer<P extends api.Plan>({
  op,
  ready,
  title,
}: {
  op: Operation<P>;
  ready: boolean;
  title: string;
}) {
  const { runtime } = usePreview();
  const connected = runtime?.mode === "core";
  return (
    <footer className="install-footer">
      <div>
        <strong>{title}</strong>
        <small>
          {!connected
            ? "浏览器预览 · 未连接"
            : op.busy
              ? "正在处理，可切换页面查看任务"
              : op.plan
                ? "请核对目录、文件与兼容性后确认"
                : "先生成真实安装计划"}
        </small>
      </div>
      <div className="action-grid">
        {op.plan && (
          <button
            className="button secondary"
            disabled={op.busy}
            onClick={() => void op.run(false)}
          >
            重新规划
          </button>
        )}
        <button
          className="button primary"
          disabled={
            !connected ||
            !ready ||
            op.busy ||
            Boolean(op.plan?.issues.some((i) => i.blocking))
          }
          onClick={() => void op.run(Boolean(op.plan))}
        >
          <Icon name="download" />
          {op.busy ? "正在处理…" : op.plan ? "确认并开始安装" : "检查安装计划"}
        </button>
      </div>
    </footer>
  );
}
export function IntegratedGameInstaller({ version }: { version: string }) {
  const { runtime, goBack } = usePreview();
  const [name, setName] = useState(`Minecraft ${version}`),
    [loader, setLoader] = useState<api.LoaderKind | "vanilla">("vanilla"),
    [build, setBuild] = useState("");
  const [builds, setBuilds] = useState<api.LoaderBuild[]>([]),
    [catalog, setCatalog] = useState<api.ComponentBuild[]>([]),
    [shader, setShader] = useState("none"),
    [componentVersion, setComponentVersion] = useState("");
  const [versions, setVersions] = useState<api.ComponentVersion[]>([]),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(false),
    [retry, setRetry] = useState(0);
  const connected =
    runtime?.mode === "core" && runtime.api?.features.includes("game-install");
  useEffect(() => {
    let alive = true;
    setBuild("");
    setBuilds([]);
    setShader("none");
    setComponentVersion("");
    setVersions([]);
    setError("");
    if (!connected) return;
    setLoading(true);
    Promise.all([
      loader === "vanilla"
        ? Promise.resolve([])
        : api.listLoaderBuilds(version, loader),
      api.getComponentCatalog(
        version,
        loader === "vanilla" ? undefined : loader,
      ),
    ])
      .then(([rows, components]) => {
        if (!alive) return;
        setBuilds(rows);
        setBuild(
          (
            rows.find((v) => v.available && v.recommended) ||
            rows.find((v) => v.available)
          )?.version || "",
        );
        setCatalog(components.shaderSupport);
      })
      .catch((c) => {
        if (alive) setError(parseApiError(c).message);
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [version, loader, connected, retry]);
  useEffect(() => {
    let alive = true;
    setComponentVersion("");
    setVersions([]);
    if (shader === "none" || !connected) return;
    api
      .listComponentVersions(
        shader,
        version,
        loader === "vanilla" ? undefined : loader,
      )
      .then((rows) => {
        if (alive) {
          setVersions(rows);
          setComponentVersion(
            (rows.find((v) => v.recommended) || rows[0])?.versionId || "",
          );
        }
      })
      .catch((c) => {
        if (alive) setError(parseApiError(c).message);
      });
    return () => {
      alive = false;
    };
  }, [shader, version, loader, connected, retry]);
  const request: api.GameRequest = {
    name: name.trim(),
    gameVersion: version,
    loader: loader === "vanilla" ? null : { kind: loader, version: build },
    shaderSupport: shader,
    ...(shader !== "none" ? { shaderSupportVersion: componentVersion } : {}),
  };
  const op = useInstallPlan(
    JSON.stringify(request),
    () => api.prepareGameInstall(request),
    (p) => api.startGameInstall(p.planId, p.revision),
  );
  return (
    <section className="content-page installer-page">
      <button className="breadcrumb" onClick={() => goBack("download/game")}>
        <Icon name="back" />
        下载 / 原版游戏
      </button>
      <header className="installer-heading">
        <Badge kind="game" large />
        <div>
          <span className="kicker">MINECRAFT</span>
          <h1 tabIndex={-1}>{version}</h1>
          <p>先选择组件，再检查完整安装计划。</p>
        </div>
      </header>
      {error && (
        <div className="banner error" role="alert">
          {error}
          <button onClick={() => setRetry((v) => v + 1)}>重新读取</button>
        </div>
      )}
      <div className="install-layout">
        <div className="install-main">
          <fieldset disabled={op.busy} className="install-fields">
            <section className="surface component-section">
              <label className="field">
                安装名称
                <input
                  aria-label="安装名称"
                  value={name}
                  maxLength={80}
                  onChange={(e) => setName(e.target.value)}
                />
              </label>
              <p className="muted">
                目标目录由核心生成；独立保存 Mod、配置与存档。
              </p>
            </section>
            <section className="surface component-section">
              <header>
                <span className="step-number">01</span>
                <div>
                  <h2>Mod 加载器</h2>
                  <p>主加载器互斥，所有构建均来自真实兼容列表。</p>
                </div>
              </header>
              <div className="loader-grid">
                {(
                  ["vanilla", "fabric", "forge", "neoforge", "quilt"] as const
                ).map((kind) => (
                  <button
                    key={kind}
                    aria-pressed={loader === kind}
                    className={loader === kind ? "selected" : ""}
                    onClick={() => {
                      if (kind === loader) return;
                      setBuild("");
                      setBuilds([]);
                      setCatalog([]);
                      setShader("none");
                      setVersions([]);
                      setComponentVersion("");
                      setLoading(true);
                      setLoader(kind);
                    }}
                  >
                    <Badge kind={kind} />
                    <span>
                      <strong>{kind === "vanilla" ? "纯净原版" : kind}</strong>
                      <small>
                        {loader === kind ? "当前选择" : "选择加载器"}
                      </small>
                    </span>
                  </button>
                ))}
              </div>
              {loader !== "vanilla" && (
                <MotionDetails open>
                  <summary>选择具体构建 · {builds.length}</summary>
                  <label className="field">
                    加载器构建
                    <select
                      aria-label="加载器构建"
                      value={build}
                      disabled={loading || op.busy}
                      onChange={(e) => setBuild(e.target.value)}
                    >
                      <option value="">
                        {loading ? "正在读取…" : "请选择兼容构建"}
                      </option>
                      {builds.map((v) => (
                        <option
                          key={v.id}
                          value={v.version}
                          disabled={!v.available}
                        >
                          {v.version} · {v.recommended ? "推荐 · " : ""}
                          {v.stability}
                          {!v.available ? ` · ${v.unavailableReason}` : ""}
                        </option>
                      ))}
                    </select>
                  </label>
                  {!loading && !builds.length && (
                    <p className="error-text">
                      此版本没有可安装构建，或读取失败；请重试或选择其他加载器。
                    </p>
                  )}
                </MotionDetails>
              )}
            </section>
            <section className="surface component-section">
              <header>
                <span className="step-number">02</span>
                <div>
                  <h2>光影支持组件</h2>
                  <p>支持组件不是光影包；依赖由核心安装计划检查。</p>
                </div>
              </header>
              <div className="loader-grid">
                <button
                  className={shader === "none" ? "selected" : ""}
                  aria-pressed={shader === "none"}
                  onClick={() => setShader("none")}
                >
                  <Badge kind="close" />
                  <strong>不安装</strong>
                </button>
                {catalog.map((c) => (
                  <button
                    key={c.id}
                    disabled={!c.available || loading}
                    aria-pressed={shader === c.id}
                    className={shader === c.id ? "selected" : ""}
                    onClick={() => setShader(c.id)}
                  >
                    <Badge kind={c.id} />
                    <span>
                      <strong>{c.label}</strong>
                      <small>
                        {c.available
                          ? c.version || "选择发行文件"
                          : c.unavailableReason || "当前组合不可用"}
                      </small>
                    </span>
                  </button>
                ))}
              </div>
              {shader !== "none" && (
                <label className="field">
                  组件发行文件
                  <select
                    aria-label="光影组件版本"
                    value={componentVersion}
                    onChange={(e) => setComponentVersion(e.target.value)}
                  >
                    <option value="">请选择实际发行文件</option>
                    {versions.map((v) => (
                      <option key={v.versionId} value={v.versionId}>
                        {v.versionNumber} · {v.channel}
                        {v.recommended ? " · 推荐" : ""} · {size(v.size)}
                      </option>
                    ))}
                  </select>
                </label>
              )}
            </section>
          </fieldset>
          {op.plan && (
            <PlanReview plan={op.plan}>
              <p className="break">目标：{op.plan.target.displayPath}</p>
              <p>
                {op.plan.request.loader
                  ? `${op.plan.request.loader.kind} ${op.plan.request.loader.version}`
                  : "纯净原版"}
              </p>
              <MotionDetails open>
                <summary>安装文件与组件</summary>
                {op.plan.files.map((f) => (
                  <div className="file-row" key={f.id}>
                    <span>
                      <strong>{f.displayName}</strong>
                      <small>
                        {f.action} · {f.role} · {f.relativePath}
                      </small>
                    </span>
                    <small>
                      {f.sizeBytes === null ? "大小待计算" : size(f.sizeBytes)}
                    </small>
                  </div>
                ))}
                {op.plan.components.map((c) => (
                  <p key={c.id}>
                    {c.label} · {c.fileName} · {size(c.sizeBytes)}
                  </p>
                ))}
              </MotionDetails>
            </PlanReview>
          )}
          <Result op={op} />
        </div>
        <aside className="surface install-side">
          <span className="kicker">A NEW CHAPTER</span>
          <h2>
            新的旅程，
            <br />
            由你选择。
          </h2>
          <div className="summary-scene" />
          <p>
            {version}
            <br />
            {loader} {build}
            <br />
            {shader !== "none" ? shader : "不安装光影支持组件"}
          </p>
          <p>
            必需依赖、文件复用与兼容性问题以核心预检结果为准；不会静默安装推荐
            Mod。
          </p>
        </aside>
      </div>
      <Footer
        op={op}
        ready={Boolean(
          connected &&
            name.trim() &&
            !loading &&
            (loader === "vanilla" || build) &&
            (shader === "none" || componentVersion),
        )}
        title={name}
      />
    </section>
  );
}

export function IntegratedResource({ argument }: { argument: string }) {
  const { runtime, goBack } = usePreview();
  const colon = argument.indexOf(":"),
    kind = colon < 0 ? "mod" : argument.slice(0, colon),
    id = colon < 0 ? argument : argument.slice(colon + 1),
    pack = kind === "modpack";
  const [project, setProject] = useState<api.ResourceProject | null>(null),
    [authors, setAuthors] = useState<string[]>([]),
    [files, setFiles] = useState<api.ResourceFile[]>([]),
    [fileId, setFileId] = useState(""),
    [targetId, setTargetId] = useState(""),
    [worlds, setWorlds] = useState<api.WorldTarget[]>([]),
    [world, setWorld] = useState("");
  const [channel, setChannel] = useState("all"),
    [name, setName] = useState(""),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(false),
    [retry, setRetry] = useState(0),
    [overwrite, setOverwrite] = useState(false);
  const connected = runtime?.mode === "core",
    target = runtime?.core.instances.find((v) => v.id === targetId),
    selected = files.find((f) => f.versionId === fileId);
  useEffect(() => {
    let alive = true;
    setError("");
    setProject(null);
    setAuthors([]);
    if (!connected) return;
    api
      .getResourceProject(id)
      .then((p) => {
        if (alive) {
          setProject(p);
          setName(p.title);
        }
      })
      .catch((c) => {
        if (alive) setError(parseApiError(c).message);
      });
    const abort = new AbortController();
    publicJson<{ role: string; user: { username: string } }[]>(
      `${MODRINTH}/project/${encodeURIComponent(id)}/members`,
      abort.signal,
    )
      .then((rows) => {
        if (alive)
          setAuthors(
            rows.filter((r) => r.role === "Owner").map((r) => r.user.username),
          );
      })
      .catch(() => {});
    return () => {
      alive = false;
      abort.abort();
    };
  }, [id, connected, retry]);
  useEffect(() => {
    let alive = true;
    setFileId("");
    setWorld("");
    setWorlds([]);
    setOverwrite(false);
    setError("");
    if (!connected || (!pack && !target)) {
      setFiles([]);
      return;
    }
    setLoading(true);
    const filter = target && (kind === "mod" || kind === "datapack");
    api
      .listResourceFiles(
        id,
        filter ? target.gameVersion : undefined,
        kind === "mod" ? target?.loader?.kind || "minecraft" : undefined,
      )
      .then((rows) => {
        if (alive) setFiles(rows);
      })
      .catch((c) => {
        if (alive) {
          setError(parseApiError(c).message);
          setFiles([]);
        }
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    if (kind === "datapack" && target)
      api
        .listTargetWorlds(target.id)
        .then((rows) => {
          if (alive) setWorlds(rows);
        })
        .catch((c) => {
          if (alive) setError(parseApiError(c).message);
        });
    return () => {
      alive = false;
    };
  }, [
    id,
    connected,
    target?.id,
    target?.gameVersion,
    target?.loader?.kind,
    kind,
    pack,
    retry,
  ]);
  const request = {
    instanceId: targetId,
    kind: kind as api.ResourceKind,
    projectId: id,
    versionId: fileId,
    ...(kind === "datapack" ? { worldId: world } : {}),
    overwrite,
  };
  const packRequest = { projectId: id, versionId: fileId, name: name.trim() };
  const op = useInstallPlan<api.ResourcePlan | api.ModpackPlan>(
    JSON.stringify(pack ? packRequest : request),
    () =>
      pack
        ? api.prepareModpackInstall(packRequest)
        : api.prepareResourceInstall(request),
    (p) =>
      pack
        ? api.startModpackInstall(p.planId, p.revision)
        : api.startResourceInstall(p.planId, p.revision),
  );
  return (
    <section className="content-page installer-page">
      <button className="breadcrumb" onClick={() => goBack(`download/${kind}`)}>
        <Icon name="back" />
        下载 / {CATEGORIES.find((c) => c.id === kind)?.name || "资源"}
      </button>
      <header className="project-heading">
        <span className="project-icon-wrap">
          <Badge kind={kind} large />
          {project?.iconUrl && (
            <img
              className="project-icon"
              src={project.iconUrl}
              alt=""
              onError={(e) => {
                e.currentTarget.hidden = true;
              }}
            />
          )}
        </span>
        <div>
          <span className="kicker">MODRINTH</span>
          <h1 tabIndex={-1}>{project?.title || "资源详情"}</h1>
          <p>
            {project?.description ||
              (!connected ? "浏览器预览 · 未连接" : "正在读取项目资料…")}
          </p>
          {project && (
            <div className="project-meta">
              <span>作者：{authors.join(" / ") || "见项目主页"}</span>
              <span>{project.downloads.toLocaleString()} 次下载</span>
              <span>
                {project.updatedAt ? date(project.updatedAt) : "更新时间未提供"}
              </span>
              <a
                href={`https://modrinth.com/project/${encodeURIComponent(project.id)}`}
                target="_blank"
                rel="noreferrer"
              >
                项目主页 · {project.licenseId || "查看许可"}
              </a>
            </div>
          )}
        </div>
      </header>
      {error && (
        <div className="banner error" role="alert">
          {error}
          <button onClick={() => setRetry((v) => v + 1)}>重新读取</button>
        </div>
      )}
      <div className="install-layout">
        <div className="install-main">
          <fieldset className="install-fields" disabled={op.busy}>
            <section className="surface component-section">
              <header>
                <span className="step-number">01</span>
                <div>
                  <h2>{pack ? "创建独立版本" : "选择目标游戏版本"}</h2>
                  <p>
                    {pack
                      ? "整合包创建独立目录，不覆盖已有世界。"
                      : "只向所选版本写入，核心重新验证兼容性与目录。"}
                  </p>
                </div>
              </header>
              {pack ? (
                <label className="field">
                  安装名称
                  <input
                    aria-label="整合包安装名称"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    maxLength={80}
                  />
                </label>
              ) : (
                <div className="target-grid">
                  {runtime?.core.instances.map((i) => (
                    <button
                      key={i.id}
                      disabled={!i.managed}
                      className={targetId === i.id ? "selected" : ""}
                      aria-pressed={targetId === i.id}
                      onClick={() => setTargetId(i.id)}
                    >
                      <Badge kind={i.loader?.kind || "game"} />
                      <span>
                        <strong>{i.name}</strong>
                        <small>
                          {i.gameVersion} ·{" "}
                          {i.loader
                            ? `${i.loader.kind} ${i.loader.version}`
                            : "纯净原版"}
                        </small>
                        <small>
                          {!i.managed
                            ? "共享目录，不开放写入"
                            : !i.health.ok
                              ? i.health.reasons[0]
                              : "选择后查询发行文件"}
                        </small>
                      </span>
                      <span className="radio-dot" />
                    </button>
                  ))}
                  {!runtime?.core.instances.length && (
                    <Empty
                      title="没有可选游戏版本"
                      detail="请先下载游戏；不会回退到全局目录。"
                    />
                  )}
                </div>
              )}
              {kind === "datapack" && target && (
                <label className="field">
                  目标存档
                  <select
                    aria-label="目标存档"
                    value={world}
                    onChange={(e) => setWorld(e.target.value)}
                  >
                    <option value="">请选择具体世界</option>
                    {worlds.map((w) => (
                      <option key={w.worldId} value={w.worldId}>
                        {w.displayName} · {w.gameVersion || "版本未知"}
                      </option>
                    ))}
                  </select>
                  {!worlds.length && (
                    <small>没有可用存档，请先在游戏内创建世界。</small>
                  )}
                </label>
              )}
            </section>
            <section className="surface component-section">
              <header>
                <span className="step-number">02</span>
                <div>
                  <h2>选择发行文件</h2>
                  <p>可自由选择实际兼容版本；资源包与光影包最终由核心校验。</p>
                </div>
              </header>
              <div className="segmented file-channels">
                {[
                  ["all", "全部"],
                  ["release", "正式版"],
                  ["beta", "Beta"],
                  ["alpha", "Alpha"],
                ].map(([v, label]) => (
                  <button
                    key={v}
                    aria-pressed={channel === v}
                    onClick={() => setChannel(v)}
                  >
                    {label}
                  </button>
                ))}
              </div>
              <p className="loading-feedback" role="status">
                {loading ? "正在查询…上次结果暂不可选择" : ""}
              </p>
              <MotionDetails open>
                <summary>发行文件 · {files.length}</summary>
                <MotionRegion changeKey={channel + targetId}>
                  <div className="file-list" aria-busy={loading}>
                    {files
                      .filter((f) => channel === "all" || f.channel === channel)
                      .map((f) => (
                        <button
                          key={f.versionId}
                          className={`file-option ${fileId === f.versionId ? "selected" : ""}`}
                          disabled={loading}
                          aria-pressed={fileId === f.versionId}
                          onClick={() => setFileId(f.versionId)}
                        >
                          <span className="radio-dot" />
                          <span>
                            <strong>{f.versionNumber}</strong>
                            <p>
                              {date(f.publishedAt)} · {size(f.size)} ·{" "}
                              {f.channel}
                            </p>
                            <small>
                              {f.gameVersions.join(" / ")} ·{" "}
                              {f.loaders.join(" / ")}
                            </small>
                          </span>
                        </button>
                      ))}
                    {!loading && !files.length && (target || pack) && (
                      <p className="banner warn">
                        没有匹配文件。请更换游戏版本或加载器；不会安装到错误目录。
                      </p>
                    )}
                  </div>
                </MotionRegion>
              </MotionDetails>
            </section>
            <section className="surface component-section">
              <header>
                <span className="step-number">03</span>
                <div>
                  <h2>依赖与冲突检查</h2>
                  <p>选择完成后生成计划，安装前必须确认。</p>
                </div>
              </header>
              {selected?.dependencies.map((d, i) => (
                <p key={i}>
                  {d.kind} · {d.projectId || d.versionId || "项目未指定"}
                </p>
              ))}
              {!pack && (
                <label className="dependency-row">
                  <input
                    type="checkbox"
                    checked={overwrite}
                    onChange={(e) => setOverwrite(e.target.checked)}
                  />
                  允许替换计划列明的同名文件（默认不覆盖）
                </label>
              )}
            </section>
          </fieldset>
          {op.plan && (
            <PlanReview plan={op.plan}>
              {"targetDir" in op.plan ? (
                <>
                  <p className="break">目标：{op.plan.targetDir}</p>
                  <p>{op.plan.fileName}</p>
                  {op.plan.replaces && (
                    <p className="banner warn">将替换：{op.plan.replaces}</p>
                  )}
                  {op.plan.dependencies.map((d) => (
                    <div className="file-row" key={d.projectId + d.versionId}>
                      <Badge kind="mod" />
                      <span>
                        <strong>必需依赖 · {d.fileName}</strong>
                        <small>
                          {d.projectId} · {size(d.size)}
                        </small>
                      </span>
                    </div>
                  ))}
                </>
              ) : (
                <>
                  <p>
                    Minecraft {op.plan.info.mcVersion} ·{" "}
                    {op.plan.info.loaderKind || "原版"}{" "}
                    {op.plan.info.loaderVersion}
                  </p>
                  <p>
                    {op.plan.info.fileCount} 个文件 · {op.plan.info.summary}
                  </p>
                </>
              )}
            </PlanReview>
          )}
          <Result op={op} />
        </div>
        <aside className="surface install-side">
          <Badge kind={kind} large />
          <h2>
            每份内容，
            <br />
            都有正确的位置。
          </h2>
          <p>目标：{pack ? name : target?.name || "尚未选择"}</p>
          <p>发行文件：{selected?.versionNumber || "尚未选择"}</p>
          <p className="break">
            {pack
              ? "创建新的独立游戏配置"
              : kind === "datapack"
                ? worlds.find((w) => w.worldId === world)?.dir
                : target?.dirs[
                    kind === "mod"
                      ? "mods"
                      : kind === "resourcepack"
                        ? "resourcepacks"
                        : "shaderpacks"
                  ]}
          </p>
        </aside>
      </div>
      <Footer
        op={op}
        title={project?.title || "资源安装"}
        ready={Boolean(
          project &&
            selected &&
            !loading &&
            (pack
              ? name.trim()
              : target?.managed && (kind !== "datapack" || world)),
        )}
      />
    </section>
  );
}
