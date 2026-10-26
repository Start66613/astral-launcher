import { MotionRegion, MotionDetails } from "./motion";
import { useEffect, useRef, useState } from "react";
import { Badge, Icon } from "./icons";
import { CATEGORIES, catalog, date, size } from "./model";
import { usePreview } from "./state";
import { Empty, PageTitle } from "./App";
import { GameCatalog } from "./Download";
import { Health, TaskView } from "./LiveRuntime";
import {
  installLoader,
  listLoaders,
  MODRINTH,
  publicJson,
  waitForTask,
  type LoaderCatalog,
  type Project,
  type ProjectVersion,
} from "./live-api";
import {
  inTauri,
  listInstances,
  listSaves,
  listVersions,
  parseApiError,
  startInstall,
} from "../lib/bridge";
import type { CoreSaveEntry, Task, VersionSummary } from "../lib/types";

export function LiveDownload({ category }: { category: string }) {
  const { navigate } = usePreview();
  const selected =
    CATEGORIES.find((item) => item.id === category) || CATEGORIES[0];
  return (
    <section className="content-page download-page">
      <PageTitle
        title="为下一次冒险做准备。"
        subtitle="选择游戏，或为已有世界寻找新的内容。"
      />
      <nav className="download-categories" aria-label="下载分类">
        {CATEGORIES.map((item) => (
          <button
            key={item.id}
            aria-current={selected.id === item.id ? "page" : undefined}
            className={selected.id === item.id ? "active" : ""}
            onClick={() => navigate(`download/${item.id}`)}
          >
            <Icon name={item.id} />
            <span>{item.name}</span>
          </button>
        ))}
      </nav>
      {selected.id === "game" ? (
        <LiveCatalog />
      ) : (
        <LiveSearch key={selected.id} kind={selected.id} />
      )}
    </section>
  );
}
function LiveCatalog() {
  const [data, setData] = useState<typeof catalog | null>(null);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let alive = true;
    const controller = new AbortController();
    setError("");
    const request = inTauri
      ? listVersions().then((versions) => ({
          versions,
          latest: {
            release:
              versions
                .filter((v) => v.kind === "release")
                .sort((a, b) => b.released.localeCompare(a.released))[0]?.id ||
              "",
            snapshot:
              versions
                .filter((v) => v.kind === "snapshot")
                .sort((a, b) => b.released.localeCompare(a.released))[0]?.id ||
              "",
          },
        }))
      : publicJson<{
          latest: { release: string; snapshot: string };
          versions: { id: string; type: string; releaseTime: string }[];
        }>(
          "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
          controller.signal,
        ).then((raw) => ({
          latest: raw.latest,
          versions: raw.versions.map((v) => ({
            id: v.id,
            kind: v.type,
            released: v.releaseTime,
          })),
        }));
    request
      .then((value) => {
        if (alive)
          setData({
            ...value,
            source:
              "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            capturedAt: new Date().toISOString(),
          });
      })
      .catch((cause) => {
        if (alive) setError(String(cause));
      });
    return () => {
      alive = false;
      controller.abort();
    };
  }, [retry]);
  return (
    <>
      {error && (
        <div className="banner error" role="alert">
          {error}
          <button onClick={() => setRetry((value) => value + 1)}>
            重新读取
          </button>
        </div>
      )}
      {data ? (
        <GameCatalog source={data} />
      ) : (
        !error && (
          <Empty
            title="正在读取官方版本清单"
            detail="联网读取真实版本；不会使用虚构的最新版本号。"
          />
        )
      )}
    </>
  );
}
function LiveSearch({ kind }: { kind: string }) {
  const { navigate } = usePreview();
  const [q, setQ] = useState("");
  const [game, setGame] = useState("");
  const [loader, setLoader] = useState("");
  const [offset, setOffset] = useState(0);
  const [refreshing, setRefreshing] = useState(true);
  const [result, setResult] = useState<{
    hits: (Project & { project_id: string; icon_url?: string })[];
    total_hits: number;
  } | null>(null);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let alive = true;
    const controller = new AbortController();
    setError("");
    setRefreshing(true);
    const timer = setTimeout(() => {
      const facets = [
        [`project_type:${kind === "shaderpack" ? "shader" : kind}`],
      ];
      if (game.trim()) facets.push([`versions:${game.trim()}`]);
      if (loader) facets.push([`categories:${loader}`]);
      const params = new URLSearchParams({
        query: q,
        facets: JSON.stringify(facets),
        limit: "20",
        offset: String(offset),
        index: "downloads",
      });
      publicJson<NonNullable<typeof result>>(
        `${MODRINTH}/search?${params}`,
        controller.signal,
      )
        .then((value) => {
          if (alive) setResult(value);
        })
        .catch((cause) => {
          if (alive) setError(String(cause));
        })
        .finally(() => {
          if (alive) setRefreshing(false);
        });
    }, 280);
    return () => {
      alive = false;
      controller.abort();
      clearTimeout(timer);
    };
  }, [kind, q, game, loader, offset, retry]);
  return (
    <>
      <div className="catalog-toolbar">
        <label className="search">
          <Icon name="search" />
          <input
            aria-label="搜索资源"
            value={q}
            placeholder="搜索名称或关键词"
            onChange={(event) => {
              setQ(event.target.value);
              setOffset(0);
            }}
          />
        </label>
        <input
          className="version-filter"
          aria-label="筛选 Minecraft 版本"
          placeholder="Minecraft 版本"
          value={game}
          onChange={(event) => {
            setGame(event.target.value);
            setOffset(0);
          }}
        />
        {["mod", "modpack"].includes(kind) && (
          <select
            aria-label="筛选加载器"
            value={loader}
            onChange={(event) => {
              setLoader(event.target.value);
              setOffset(0);
            }}
          >
            <option value="">全部加载器</option>
            {["fabric", "forge", "neoforge", "quilt"].map((id) => (
              <option key={id}>{id}</option>
            ))}
          </select>
        )}
      </div>
      {error && (
        <div className="banner error" role="alert">
          网络读取未完成：{error}
          <button onClick={() => setRetry((value) => value + 1)}>重试</button>
        </div>
      )}
      {!result && !error && (
        <Empty
          title="正在寻找内容"
          detail="查询 Modrinth 公开资源，不自动安装。"
        />
      )}
      <p className="loading-feedback" role="status">
        {refreshing
          ? result
            ? "正在刷新，上次结果暂不可选择…"
            : "正在读取搜索结果…"
          : error
            ? "网络未完成，保留上次结果"
            : ""}
      </p>
      <MotionRegion
        changeKey={
          result?.hits.map((project) => project.project_id).join(",") || ""
        }
      >
        <div className="resource-grid" aria-busy={refreshing}>
          {result?.hits.map((project) => (
            <button
              className="project-card"
              disabled={refreshing || !!error}
              key={project.project_id}
              onClick={() => navigate(`resource/${kind}:${project.project_id}`)}
            >
              <div className="project-cover">
                <Badge kind={kind} large />
                {project.icon_url && (
                  <img
                    className="project-icon resource-card-icon"
                    src={project.icon_url}
                    alt=""
                    loading="lazy"
                    decoding="async"
                    onError={(event) => {
                      event.currentTarget.hidden = true;
                    }}
                  />
                )}
                <span className="tag">Modrinth</span>
              </div>
              <div>
                <h3>{project.title}</h3>
                <p>{project.description}</p>
                <footer>
                  <span>
                    {new Intl.NumberFormat("zh", {
                      notation: "compact",
                    }).format(project.downloads)}{" "}
                    次下载
                  </span>
                  <Icon name="arrow" />
                </footer>
              </div>
            </button>
          ))}
        </div>
      </MotionRegion>
      {result && !result.hits.length && (
        <Empty
          title="没有找到匹配资源"
          detail="调整关键词、游戏版本或加载器试试。"
        />
      )}
      {result && (
        <div className="pagination">
          <span>{result.total_hits} 个结果</span>
          <button
            className="button secondary"
            disabled={!offset}
            onClick={() => setOffset((value) => Math.max(0, value - 20))}
          >
            上一页
          </button>
          <button
            className="button secondary"
            disabled={offset + 20 >= result.total_hits}
            onClick={() => setOffset((value) => value + 20)}
          >
            下一页
          </button>
        </div>
      )}
    </>
  );
}
export function LiveGameInstaller({ version }: { version: string }) {
  const { runtime, navigate, goBack, notify } = usePreview();
  const [loader, setLoader] = useState("vanilla");
  const [build, setBuild] = useState("");
  const [catalogs, setCatalogs] = useState<LoaderCatalog[]>([]);
  const [name, setName] = useState(`Minecraft ${version}`);
  const [error, setError] = useState("");
  const [task, setTask] = useState<Task | null>(null);
  const [busy, setBusy] = useState(false);
  const [complete, setComplete] = useState<string | null>(null);
  const lock = useRef(false);
  useEffect(() => {
    let alive = true;
    setCatalogs([]);
    setBuild("");
    if (runtime?.mode === "core" && runtime.api?.features.includes("loaders"))
      listLoaders(version)
        .then((value) => {
          if (alive) setCatalogs(value);
        })
        .catch((cause) => {
          if (alive) setError(parseApiError(cause).message);
        });
    return () => {
      alive = false;
    };
  }, [version, runtime?.mode, runtime?.api?.features.includes("loaders")]);
  async function install() {
    if (lock.current || !runtime || runtime.mode !== "core") return;
    lock.current = true;
    setBusy(true);
    setError("");
    setComplete(null);
    try {
      if (
        [...runtime.core.tasks.values()].some(
          (t) =>
            t.gameVersion === version &&
            (t.state === "running" || t.state === "queued"),
        )
      )
        throw Error("此游戏版本已有任务，请先在下载与任务中查看。");
      await waitForTask(await startInstall(version), setTask);
      const created = await waitForTask(
        await runtime.core.create({ name: name.trim(), gameVersion: version }),
        setTask,
      );
      if (created.result?.kind !== "create")
        throw Error("核心未返回新版本配置，不能确认整体安装完成");
      const instance = created.result.instance;
      if (loader !== "vanilla")
        await waitForTask(
          await installLoader(instance.id, loader, build),
          setTask,
        );
      await runtime.core.refresh();
      const confirmed = (await listInstances()).find(
        (item) => item.id === instance.id,
      );
      if (!confirmed?.health.ok)
        throw Error(
          confirmed?.health.reasons.join("；") ||
            "无法确认安装后的游戏文件，请在版本管理中检查",
        );
      setComplete(confirmed.name);
    } catch (cause) {
      setError(parseApiError(cause).message);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  const canInstall =
    runtime?.mode === "core" &&
    runtime.core.hasTasks &&
    runtime.core.status === "ready" &&
    !busy &&
    name.trim() &&
    (loader === "vanilla" || !!build);
  return (
    <section className="content-page installer-page">
      <button className="breadcrumb" onClick={() => goBack("download/game")}>
        <Icon name="back" />
        下载 / 原版游戏
      </button>
      <PageTitle
        title={`Minecraft ${version}`}
        subtitle="安装组件与光影支持分别选择，不覆盖已有世界。"
      />
      <div className="install-layout">
        <div className="install-main">
          <section className="surface install-basics">
            <label className="field">
              安装名称
              <input
                aria-label="安装名称"
                value={name}
                disabled={busy}
                maxLength={80}
                onChange={(event) => setName(event.target.value)}
              />
            </label>
            <div className="field">
              <span>目标目录</span>
              <div className="path">
                <Icon name="folder" />
                <code>
                  {runtime?.config?.gameDir || "连接后读取游戏目录"} / instances
                  / 自动分配安全 ID
                </code>
              </div>
              <small>创建新的独立游戏配置，保留现有存档。</small>
            </div>
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">01</span>
              <div>
                <h2>Mod 加载器</h2>
                <p>选择一种；具体构建由当前游戏版本的目录提供。</p>
              </div>
            </header>
            <div className="loader-options">
              {[
                ["vanilla", "纯净原版"],
                ["fabric", "Fabric"],
                ["forge", "Forge"],
                ["neoforge", "NeoForge"],
                ["quilt", "Quilt"],
              ].map(([id, title]) => {
                const entry = catalogs.find((c) => c.kind === id);
                return (
                  <button
                    key={id}
                    disabled={busy || (id !== "vanilla" && !entry)}
                    aria-pressed={loader === id}
                    className={loader === id ? "selected" : ""}
                    onClick={() => {
                      setLoader(id);
                      setBuild(entry?.recommended || "");
                    }}
                  >
                    <Badge kind={id === "vanilla" ? "game" : id} />
                    <strong>{title}</strong>
                    <small>
                      {id === "vanilla"
                        ? "无需加载器"
                        : entry
                          ? "可选择构建"
                          : "暂不可安装"}
                    </small>
                    <span className="radio-dot" />
                  </button>
                );
              })}
            </div>
            {loader !== "vanilla" && (
              <MotionDetails open className="build-picker">
                <summary>具体构建与其他兼容版本</summary>
                <p className="muted">
                  推荐：
                  {catalogs.find((c) => c.kind === loader)?.recommended ||
                    "目录未标记"}
                  ；没有稳定性字段时不猜测稳定／测试。
                </p>
                <label className="field">
                  具体构建
                  <select
                    aria-label="Loader 具体版本"
                    value={build}
                    disabled={busy}
                    onChange={(event) => setBuild(event.target.value)}
                  >
                    <option value="">请选择构建</option>
                    {catalogs
                      .find((c) => c.kind === loader)
                      ?.versions.map((id) => (
                        <option key={id}>{id}</option>
                      ))}
                  </select>
                </label>
              </MotionDetails>
            )}
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">02</span>
              <div>
                <h2>光影支持组件</h2>
                <p>组件让游戏能加载光影；光影包在下载分类中另选。</p>
              </div>
            </header>
            <div className="support-options">
              {[
                ["close", "不安装"],
                ["iris", "Iris"],
                ["oculus", "Oculus"],
                ["optifine", "OptiFine"],
              ].map(([id, title]) => (
                <button
                  key={id}
                  disabled={id !== "close"}
                  className={id === "close" ? "selected" : ""}
                >
                  <Badge kind={id} />
                  <span>
                    <strong>{title}</strong>
                    <small>
                      {id === "close" ? "当前选择" : "安全依赖安装暂不可用"}
                    </small>
                  </span>
                </button>
              ))}
            </div>
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">03</span>
              <div>
                <h2>依赖与推荐组件</h2>
                <p>
                  Fabric API 按所用 Mod 的依赖决定，不对所有 Fabric
                  游戏强制安装。
                </p>
              </div>
            </header>
            <button
              className="button secondary"
              onClick={() => navigate("resource/mod:P7dR8mSH")}
            >
              查看 Fabric API 发行文件
            </button>
            <p className="muted">
              依赖安装计划未提供，因此不会静默下载任何推荐组件。
            </p>
          </section>
          {task && <TaskView key={task.taskId} task={task} />}
          {error && (
            <div className="banner error" role="alert">
              {error}
              <button onClick={() => notify(error)}>保留错误详情</button>
            </div>
          )}
          {complete && (
            <div className="banner">
              <Icon name="check" />
              {complete} 已安装，且游戏文件基础检查通过。
              <button onClick={() => navigate("versions")}>查看我的版本</button>
            </div>
          )}
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
            {loader === "vanilla" ? "纯净原版" : `${loader} ${build}`}
            <br />
            独立保存 Mod、配置与存档
          </p>
          <p>
            安装、校验和目录创建均由本地核心执行。完成后的状态以重新读取结果为准。
          </p>
        </aside>
      </div>
      <footer className="install-footer">
        <div>
          <strong>{name}</strong>
          <small>
            {runtime?.mode === "browser"
              ? "浏览器预览 · 未连接"
              : busy
                ? "正在执行真实任务，可在任务列表查看"
                : "游戏版本 → 新的独立配置"}
          </small>
        </div>
        <button
          className="button primary"
          disabled={!canInstall}
          onClick={() => void install()}
        >
          <Icon name="download" />
          {busy ? "安装中…" : "开始安装"}
        </button>
      </footer>
    </section>
  );
}
export function LiveResource({ argument }: { argument: string }) {
  const { runtime, navigate, goBack } = usePreview();
  const [kind, id] = argument.includes(":")
    ? argument.split(":")
    : ["mod", argument];
  const [project, setProject] = useState<Project | null>(null);
  const [authors, setAuthors] = useState<string[]>([]);
  const [targetId, setTargetId] = useState("");
  const [files, setFiles] = useState<ProjectVersion[]>([]);
  const [fileId, setFileId] = useState("");
  const [channel, setChannel] = useState("all");
  const [saves, setSaves] = useState<CoreSaveEntry[]>([]);
  const [world, setWorld] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [retry, setRetry] = useState(0);
  const [packName, setPackName] = useState("");
  const modpack = kind === "modpack";
  const target = runtime?.core.instances.find((i) => i.id === targetId);
  const selected = files.find((f) => f.id === fileId);
  useEffect(() => {
    let alive = true;
    const abort = new AbortController();
    setError("");
    setProject(null);
    publicJson<Project>(
      `${MODRINTH}/project/${encodeURIComponent(id)}`,
      abort.signal,
    )
      .then((value) => {
        if (alive) {
          setProject(value);
          setPackName(value.title);
        }
      })
      .catch((cause) => {
        if (alive) setError(String(cause));
      });
    publicJson<{ role: string; user: { username: string } }[]>(
      `${MODRINTH}/project/${encodeURIComponent(id)}/members`,
      abort.signal,
    )
      .then((rows) => {
        if (alive)
          setAuthors(
            rows
              .filter((row) => row.role === "Owner")
              .map((row) => row.user.username),
          );
      })
      .catch(() => {});
    return () => {
      alive = false;
      abort.abort();
    };
  }, [id, retry]);
  useEffect(() => {
    let alive = true;
    const abort = new AbortController();
    setFileId("");
    setSaves([]);
    setWorld("");
    if (!modpack && !target) {
      setFiles([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError("");
    const params = new URLSearchParams();
    if (target) {
      params.set("game_versions", JSON.stringify([target.gameVersion]));
      if (kind === "mod")
        params.set(
          "loaders",
          JSON.stringify([target.loader?.kind || "minecraft"]),
        );
    }
    publicJson<ProjectVersion[]>(
      `${MODRINTH}/project/${encodeURIComponent(id)}/version?${params}`,
      abort.signal,
    )
      .then((rows) => {
        if (alive) setFiles(rows);
      })
      .catch((cause) => {
        if (alive) {
          setError(String(cause));
          setFiles([]);
        }
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    if (
      kind === "datapack" &&
      target &&
      runtime?.api?.features.includes("saves")
    )
      listSaves(target.id)
        .then((rows) => {
          if (alive) setSaves(rows);
        })
        .catch((cause) => {
          if (alive) setError(parseApiError(cause).message);
        });
    return () => {
      alive = false;
      abort.abort();
    };
  }, [id, target?.id, kind, modpack, retry]);
  const directory = modpack
    ? "新建独立版本目录"
    : !target
      ? "尚未选择"
      : kind === "datapack"
        ? world
          ? `${world}/datapacks`
          : "请先选择存档"
        : kind === "mod"
          ? target.dirs.mods
          : kind === "resourcepack"
            ? target.dirs.resourcepacks
            : target.dirs.shaderpacks;
  return (
    <section className="content-page installer-page">
      <button className="breadcrumb" onClick={() => goBack(`download/${kind}`)}>
        <Icon name="back" />
        下载 / {CATEGORIES.find((c) => c.id === kind)?.name || "Mod"}
      </button>
      <header className="project-heading">
        {project?.icon_url ? (
          <span className="project-icon-wrap">
            <Badge kind={kind} large />
            <img
              className="project-icon"
              src={project.icon_url}
              alt=""
              decoding="async"
              onError={(event) => {
                event.currentTarget.hidden = true;
              }}
            />
          </span>
        ) : (
          <Badge kind={kind} large />
        )}
        <div>
          <span className="kicker">MODRINTH</span>
          <h1 tabIndex={-1}>{project?.title || "资源详情"}</h1>
          <p>{project?.description || "正在读取公开项目信息…"}</p>
          {project && (
            <div className="project-meta">
              <span>作者：{authors.join(" / ") || "见项目主页"}</span>
              <span>{project.downloads.toLocaleString()} 次下载</span>
              <span>更新于 {date(project.updated)}</span>
              <a
                href={`https://modrinth.com/project/${project.id}`}
                target="_blank"
                rel="noreferrer"
              >
                项目主页与许可
                <Icon name="arrow" />
              </a>
            </div>
          )}
        </div>
      </header>
      {error && (
        <div className="banner error" role="alert">
          {error}
          <button onClick={() => setRetry((value) => value + 1)}>
            重试读取
          </button>
        </div>
      )}
      <div className="install-layout">
        <div className="install-main">
          <section className="surface component-section">
            <header>
              <span className="step-number">01</span>
              <div>
                <h2>{modpack ? "创建独立版本" : "选择目标游戏版本"}</h2>
                <p>
                  {modpack
                    ? "整合包不直接覆盖已有世界。"
                    : "只向所选游戏目录安装；不会自动写入共享目录。"}
                </p>
              </div>
            </header>
            {modpack ? (
              <>
                <label className="field">
                  新版本名称
                  <input
                    value={packName}
                    onChange={(event) => setPackName(event.target.value)}
                  />
                </label>
                <div className="banner">
                  整合包需检查覆盖文件与压缩内容，安装能力尚未提供。
                </div>
              </>
            ) : (
              <div className="target-grid">
                {runtime?.core.instances.map((instance) => (
                  <button
                    key={instance.id}
                    disabled={!instance.managed}
                    className={targetId === instance.id ? "selected" : ""}
                    aria-pressed={targetId === instance.id}
                    onClick={() => setTargetId(instance.id)}
                  >
                    <Badge kind={instance.loader?.kind || "game"} />
                    <div>
                      <strong>{instance.name}</strong>
                      <span>
                        {instance.gameVersion} ·{" "}
                        {instance.loader
                          ? `${instance.loader.kind} ${instance.loader.version}`
                          : "纯净原版"}
                      </span>
                      <small>
                        {!instance.managed
                          ? "共享游戏目录，不提供写入"
                          : instance.health.ok
                            ? "选择后查询匹配文件"
                            : "游戏文件需要补全；仍可查看兼容文件"}
                      </small>
                    </div>
                    <span className="radio-dot" />
                  </button>
                ))}
                {!runtime?.core.instances.length && (
                  <Empty
                    title="没有可选游戏版本"
                    detail="请连接本地核心并下载游戏，资源需要明确的安装目标。"
                  />
                )}
              </div>
            )}
            {kind === "datapack" && target && (
              <label className="field nested">
                选择目标存档
                <select
                  aria-label="目标存档"
                  value={world}
                  onChange={(event) => setWorld(event.target.value)}
                >
                  <option value="">请选择存档</option>
                  {saves.map((save) => (
                    <option key={save.dir} value={save.dir}>
                      {save.name} · {save.gameVersion || "版本未知"}
                    </option>
                  ))}
                </select>
              </label>
            )}
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">02</span>
              <div>
                <h2>选择资源文件版本</h2>
                <p>
                  版本号、渠道与大小来自项目发行元数据，可选其他实际匹配版本。
                </p>
              </div>
            </header>
            <div className="segmented file-channels">
              {[
                ["all", "全部"],
                ["release", "正式版"],
                ["beta", "Beta"],
                ["alpha", "Alpha"],
              ].map(([value, label]) => (
                <button
                  key={value}
                  aria-pressed={channel === value}
                  onClick={() => setChannel(value)}
                >
                  {label}
                </button>
              ))}
            </div>
            <p className="loading-feedback" role="status">
              {loading
                ? files.length
                  ? `正在为 ${target?.name || "所选版本"} 查询文件，下方为上次结果，请等待更新…`
                  : "正在查询匹配文件…"
                : ""}
            </p>
            <MotionDetails open className="resource-file-disclosure">
              <summary>兼容的发行文件 · {files.length}</summary>{" "}
              <MotionRegion
                changeKey={`${channel}:${files.map((file) => file.id).join(",")}`}
              >
                <div className="file-list" aria-busy={loading}>
                  {files
                    .filter(
                      (file) =>
                        channel === "all" || file.version_type === channel,
                    )
                    .map((file) => (
                      <button
                        key={file.id}
                        className={`file-option ${fileId === file.id ? "selected" : ""}`}
                        disabled={loading}
                        onClick={() => setFileId(file.id)}
                      >
                        <span className="radio-dot" />
                        <div>
                          <strong>{file.version_number}</strong>
                          <p>
                            {date(file.date_published)} ·{" "}
                            {size(
                              file.files.find((f) => f.primary)?.size ||
                                file.files[0]?.size ||
                                0,
                            )}{" "}
                            · {file.loaders.join(" / ")}
                          </p>
                          <small>
                            Minecraft {file.game_versions.join(" / ")}
                          </small>
                        </div>
                        <div>
                          <span className="tag">{file.version_type}</span>
                          {file.featured && (
                            <span className="tag">项目方精选</span>
                          )}
                        </div>
                      </button>
                    ))}
                  {(target || modpack) &&
                    !files.length &&
                    !error &&
                    !loading && (
                      <div className="banner warn">
                        没有匹配的发行文件，请更换目标游戏或加载器；不会自动选择错误目录。
                      </div>
                    )}
                </div>
              </MotionRegion>
            </MotionDetails>
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">03</span>
              <div>
                <h2>依赖与冲突</h2>
                <p>元数据匹配不等于安全安装校验通过。</p>
              </div>
            </header>
            {selected?.dependencies.map((dependency, index) => (
              <div className="dependency-row" key={index}>
                <Badge kind="api" />
                <span>
                  <strong>
                    {dependency.dependency_type === "required"
                      ? "必需依赖"
                      : dependency.dependency_type === "incompatible"
                        ? "不兼容项目"
                        : "可选依赖"}
                  </strong>
                  <small>
                    {dependency.project_id || "具体项目需进一步解析"} ·
                    已安装状态待检查
                  </small>
                </span>
              </div>
            ))}
            <div className="banner warn">
              定向安装、依赖与冲突检查尚未提供，因此当前不会下载资源、写入游戏目录或显示安装成功。
              {kind === "datapack"
                ? "数据包还需要验证目标世界与数据包格式。"
                : ""}
            </div>
          </section>
        </div>
        <aside className="surface install-side">
          <Badge kind={kind} large />
          <h2>
            每个资源，
            <br />
            都有正确的位置。
          </h2>
          <p>目标：{modpack ? packName : target?.name || "尚未选择"}</p>
          <p>文件：{selected?.version_number || "尚未选择"}</p>
          <p className="break">目录：{directory}</p>
        </aside>
      </div>
      <footer className="install-footer">
        <div>
          <strong>{project?.title || "资源详情"}</strong>
          <small>
            {selected
              ? "文件已选择，完整安装校验尚不可用"
              : "先选择目标游戏与具体文件"}
          </small>
        </div>
        <button className="button primary" disabled>
          安装暂不可用
        </button>
      </footer>
    </section>
  );
}
