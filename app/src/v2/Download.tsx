import { useMemo, useRef, useState } from "react";
import { Badge, Icon } from "./icons";
import {
  CATEGORIES,
  catalog,
  resourceCatalog,
  loaderCatalog,
  compatibleFiles,
  date,
  size,
  kindIcon,
  type Resource,
  type ResourceFileVersion,
} from "./model";
import { usePreview } from "./state";
import { Empty, Modal, PageTitle } from "./App";

export function DownloadPage({ category }: { category: string }) {
  const { navigate } = usePreview();
  const selected =
    CATEGORIES.find((item) => item.id === category) || CATEGORIES[0];
  return (
    <section className="content-page download-page" data-page="download">
      <PageTitle
        title="为下一次冒险做准备。"
        subtitle="从一个新世界，到让它与众不同的每一个细节。"
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
        <GameCatalog />
      ) : (
        <ResourceCatalog key={selected.id} category={selected.id} />
      )}
    </section>
  );
}

export function GameCatalog({ source }: { source?: typeof catalog }) {
  const listing = source || catalog;
  const { navigate, versions, runtime } = usePreview();
  const [query, setQuery] = useState("");
  const [type, setType] = useState("all");
  const [folded, setFolded] = useState<string[]>([]);
  const [scroll, setScroll] = useState(0);
  const viewport = useRef<HTMLDivElement>(null);
  const rowHeight = 62;
  const groups = [
    ["release", "正式版"],
    ["snapshot", "快照版"],
    ["old", "远古版本"],
    ["special", "特殊版本"],
  ];
  const filtered = useMemo(
    () =>
      listing.versions.filter(
        (version) =>
          version.id.toLowerCase().includes(query.toLowerCase()) &&
          (type === "all" ||
            (type === "old"
              ? version.kind.startsWith("old_")
              : type === "special"
                ? !["release", "snapshot", "old_alpha", "old_beta"].includes(
                    version.kind,
                  )
                : version.kind === type)),
      ),
    [query, type, listing],
  );
  const rows = useMemo(
    () =>
      groups.flatMap(([id, title]) => {
        const members = filtered.filter((version) =>
          id === "old"
            ? version.kind.startsWith("old_")
            : id === "special"
              ? !["release", "snapshot", "old_alpha", "old_beta"].includes(
                  version.kind,
                )
              : version.kind === id,
        );
        if (!members.length) return [];
        return [
          {
            key: `group-${id}`,
            group: id,
            title,
            count: members.length,
            version: null as (typeof listing.versions)[number] | null,
          },
          ...(!folded.includes(id)
            ? members.map((version) => ({
                key: version.id,
                group: "",
                title: "",
                count: 0,
                version,
              }))
            : []),
        ];
      }),
    [filtered, folded],
  );
  const start = Math.max(0, Math.floor(scroll / rowHeight) - 3);
  const visible = rows.slice(start, start + 18);
  function resetList() {
    setScroll(0);
    viewport.current?.scrollTo(0, 0);
  }
  return (
    <>
      <div className="latest-grid">
        {[
          [
            listing.latest.release,
            "最新正式版",
            "release",
            "适合开始一段稳定的旅程",
          ],
          [
            listing.latest.snapshot,
            "最新快照版",
            "snapshot",
            "抢先体验，建议使用独立测试存档",
          ],
        ].map(([id, title, kind, detail]) => (
          <button
            key={kind}
            className={`latest-card latest-${kind}`}
            onClick={() => navigate(`game/${encodeURIComponent(id)}`)}
          >
            <Badge kind={kindIcon(kind)} large />
            <div>
              <span className="small-label">{title}</span>
              <h2>{id}</h2>
              <p>{detail}</p>
            </div>
            <Icon name="arrow" />
          </button>
        ))}
      </div>
      <div className="catalog-toolbar">
        <label className="search">
          <Icon name="search" />
          <input
            aria-label="搜索 Minecraft 版本"
            placeholder="搜索版本，例如 1.21.1"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              resetList();
            }}
          />
        </label>
        <select
          aria-label="版本类型"
          value={type}
          onChange={(event) => {
            setType(event.target.value);
            resetList();
          }}
        >
          <option value="all">全部版本类型</option>
          {groups.map(([id, name]) => (
            <option key={id} value={id}>
              {name}
            </option>
          ))}
        </select>
        <span>{filtered.length} 个版本</span>
      </div>
      <div className="catalog-columns">
        <span>版本</span>
        <span>发布时间</span>
        <span>状态 / 操作</span>
      </div>
      <div
        className="version-viewport"
        ref={viewport}
        onScroll={(event) => setScroll(event.currentTarget.scrollTop)}
        data-total-rows={rows.length}
        aria-label="Minecraft 版本列表"
      >
        <div style={{ height: rows.length * rowHeight, position: "relative" }}>
          {visible.map((row, offset) => (
            <div
              key={row.key}
              className="virtual-row"
              style={{
                height: rowHeight,
                transform: `translateY(${(start + offset) * rowHeight}px)`,
              }}
            >
              {row.version ? (
                <button
                  className="catalog-version"
                  onClick={() =>
                    navigate(`game/${encodeURIComponent(row.version!.id)}`)
                  }
                >
                  <Badge kind={kindIcon(row.version.kind)} />
                  <span>
                    <strong>{row.version.id}</strong>
                    <small>
                      {row.version.kind === "release"
                        ? "正式版"
                        : row.version.kind === "snapshot"
                          ? "快照版 · 测试内容"
                          : "历史版本"}
                    </small>
                  </span>
                  <time>{date(row.version.released)}</time>
                  <span>
                    {versions.some(
                      (version) => version.gameVersion === row.version!.id,
                    ) ? (
                      <span className="tag installed">
                        <Icon name="installed" />
                        {runtime ? "已安装" : "示例已安装"}
                      </span>
                    ) : (
                      <span className="row-install">
                        选择组件
                        <Icon name="arrow" />
                      </span>
                    )}
                  </span>
                </button>
              ) : (
                <button
                  className="catalog-group"
                  aria-expanded={!folded.includes(row.group)}
                  onClick={() => {
                    setFolded((previous) =>
                      previous.includes(row.group)
                        ? previous.filter((id) => id !== row.group)
                        : [...previous, row.group],
                    );
                    resetList();
                  }}
                >
                  <Icon
                    name="chevron"
                    style={{
                      transform: folded.includes(row.group)
                        ? "rotate(-90deg)"
                        : undefined,
                    }}
                  />
                  <strong>{row.title}</strong>
                  <span>{row.count}</span>
                </button>
              )}
            </div>
          ))}
        </div>
        {!filtered.length && (
          <Empty
            title="没有找到这个版本"
            detail="试试完整版本号，或切换版本类型。"
          />
        )}
      </div>
      <p className="source-note">
        Mojang 官方清单{runtime ? " · 读取于" : "快照 ·"}{" "}
        {date(listing.capturedAt)} · {listing.versions.length}{" "}
        个公开版本，虚拟滚动。特殊版本仅在官方元数据提供时显示。
        <a href={listing.source} target="_blank" rel="noreferrer">
          来源
          <Icon name="arrow" />
        </a>
      </p>
    </>
  );
}

function ResourceCatalog({ category }: { category: string }) {
  const { navigate, scenario } = usePreview();
  const [query, setQuery] = useState("");
  const [loader, setLoader] = useState("all");
  const [channel, setChannel] = useState("all");
  const projects = resourceCatalog.projects.filter(
    (project) =>
      (category === "mod"
        ? ["mod", "support", "component"].includes(project.kind)
        : category === project.kind) &&
      `${project.title} ${project.description}`
        .toLowerCase()
        .includes(query.toLowerCase()) &&
      project.versions.some(
        (version) =>
          (loader === "all" || version.loaders.includes(loader)) &&
          (channel === "all" || version.type === channel),
      ),
  );
  return (
    <>
      <header className="section-heading resource-heading">
        <div>
          <h2>{CATEGORIES.find((item) => item.id === category)?.name}</h2>
          <span>
            {category === "datapack"
              ? "安装时需要选择一个存档。"
              : category === "modpack"
                ? "为整合包创建独立版本，保护原有世界。"
                : "先选择喜欢的内容，再决定装到哪个游戏版本。"}
          </span>
        </div>
        <span className="tag">Modrinth 公开元数据快照</span>
      </header>
      <div className="catalog-toolbar">
        <label className="search">
          <Icon name="search" />
          <input
            placeholder={`搜索${CATEGORIES.find((item) => item.id === category)?.name}`}
            aria-label="搜索资源"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        {category === "mod" && (
          <select
            aria-label="筛选加载器"
            value={loader}
            onChange={(event) => setLoader(event.target.value)}
          >
            <option value="all">全部加载器</option>
            <option value="fabric">Fabric</option>
            <option value="neoforge">NeoForge</option>
            <option value="quilt">Quilt</option>
          </select>
        )}
        <select
          aria-label="资源发行渠道"
          value={channel}
          onChange={(event) => setChannel(event.target.value)}
        >
          <option value="all">全部发行渠道</option>
          <option value="release">正式版</option>
          <option value="beta">Beta</option>
          <option value="alpha">Alpha</option>
        </select>
        <span>MC 1.21.1</span>
      </div>
      {scenario === "download-error" && (
        <div className="banner error">
          <Icon name="warning" />
          <span>
            示例网络错误：清单刷新失败。保留已有内容和错误详情，恢复连接后再请求。
          </span>
          <button onClick={() => navigate("resource/sodium")}>
            查看保留的详情
          </button>
        </div>
      )}
      <div className="resource-grid">
        {projects.map((project) => (
          <button
            className="project-card"
            key={project.id}
            onClick={() => navigate(`resource/${project.slug}`)}
          >
            <div className="project-cover">
              <Badge
                kind={
                  project.kind === "support"
                    ? "iris"
                    : project.kind === "component"
                      ? "api"
                      : project.kind
                }
                large
              />
              <span className="tag">
                {project.kind === "support"
                  ? "光影支持组件"
                  : project.kind === "component"
                    ? "依赖组件"
                    : CATEGORIES.find((item) => item.id === category)?.name}
              </span>
            </div>
            <div>
              <h3>{project.title}</h3>
              <p>{project.description}</p>
              <footer>
                <span>
                  {new Intl.NumberFormat("zh", { notation: "compact" }).format(
                    project.downloads,
                  )}{" "}
                  次下载
                </span>
                <span>
                  {date(project.updated)}
                  <Icon name="arrow" />
                </span>
              </footer>
            </div>
          </button>
        ))}
      </div>
      {!projects.length && (
        <Empty
          title="当前快照没有匹配内容"
          detail="这是有限的设计样本，不代表线上没有相关资源。正式版需要实时查询与分页。"
        />
      )}
      <p className="source-note">
        {date(resourceCatalog.capturedAt)} 读取的公开事实数据；仅缓存部分 1.21.1
        发行文件。项目图标采用 Astral 原创分类符号，不借用项目品牌素材。
      </p>
    </>
  );
}

const LOADERS = [
  ["game", "纯净原版"],
  ["fabric", "Fabric"],
  ["forge", "Forge"],
  ["neoforge", "NeoForge"],
  ["quilt", "Quilt"],
];
export function GameInstaller({ version }: { version: string }) {
  const { navigate, scenario } = usePreview();
  const [loader, setLoader] = useState("game");
  const [build, setBuild] = useState(
    loaderCatalog.fabric.find((item) => item.stable)?.version || "",
  );
  const [support, setSupport] = useState("none");
  const iris = resourceCatalog.projects.find((item) => item.slug === "iris")!;
  const [irisBuild, setIrisBuild] = useState("");
  const [api, setApi] = useState(false);
  const [apiBuild, setApiBuild] = useState("");
  const [name, setName] = useState(`Minecraft ${version}`);
  const [confirm, setConfirm] = useState(false);
  const fabricAvailable = version === loaderCatalog.gameVersion;
  const irisFiles = iris.versions.filter(
    (file) =>
      file.gameVersions.includes(version) && file.loaders.includes("fabric"),
  );
  const conflict =
    scenario === "conflict" || (support === "iris" && loader !== "fabric");
  const blocked =
    conflict ||
    !name.trim() ||
    (support === "iris" && !irisBuild) ||
    (api && loader === "fabric" && !apiBuild) ||
    scenario === "offline";
  return (
    <section className="content-page installer-page" data-page="game-installer">
      <button className="breadcrumb" onClick={() => navigate("download/game")}>
        <Icon name="back" />
        下载 / 原版游戏
      </button>
      <div className="installer-heading">
        <Badge
          kind={kindIcon(
            catalog.versions.find((item) => item.id === version)?.kind ||
              "release",
          )}
          large
        />
        <PageTitle
          title={`Minecraft ${version}`}
          subtitle="选好组件，再开始你的新世界。"
        />
        <span className="tag">安装配置</span>
      </div>
      <div className="install-layout">
        <div className="install-main">
          <section className="surface install-basics">
            <label className="field">
              安装名称
              <input
                value={name}
                maxLength={80}
                onChange={(event) => setName(event.target.value)}
                aria-label="安装名称"
              />
            </label>
            <div className="field">
              <span>目标目录</span>
              <div className="path">
                <Icon name="folder" />
                <code>D:/Games/Astral/instances/（核心分配安全 ID）</code>
              </div>
              <small>
                示例目录 · 下载完成后自动出现在“我的版本”。不覆盖已有目录。
              </small>
            </div>
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">01</span>
              <div>
                <h2>Mod 加载器</h2>
                <p>选择一种，或保留纯净原版。</p>
              </div>
            </header>
            <div className="loader-options">
              {LOADERS.map(([id, title]) => {
                const unavailable =
                  id !== "game" && (id !== "fabric" || !fabricAvailable);
                return (
                  <button
                    className={loader === id ? "selected" : ""}
                    key={id}
                    aria-pressed={loader === id}
                    disabled={unavailable}
                    onClick={() => setLoader(id)}
                    title={
                      unavailable
                        ? id === "fabric"
                          ? "未缓存此 Minecraft 版本的构建清单，正式版需实时查询"
                          : "核心构建查询或安装链尚未完整提供"
                        : undefined
                    }
                  >
                    <Badge kind={id} />
                    <strong>{title}</strong>
                    <small>
                      {id === "game"
                        ? "无需组件"
                        : unavailable
                          ? "待核心支持"
                          : "可选择构建"}
                    </small>
                    <span className="radio-dot" />
                  </button>
                );
              })}
            </div>
            {loader === "fabric" && (
              <div className="build-picker">
                <div>
                  <span className="tag">
                    所选构建 ·{" "}
                    {loaderCatalog.fabric.find((item) => item.version === build)
                      ?.stable
                      ? "稳定"
                      : "测试"}
                  </span>
                  <strong>{build}</strong>
                </div>
                <p className="inline-note">
                  推荐入口：
                  {loaderCatalog.fabric.find((item) => item.stable)?.version ||
                    "暂无稳定标记"}{" "}
                  · 依据官方 stable 字段；仍需核心校验。
                </p>
                <details>
                  <summary>
                    选择其他兼容构建 · {loaderCatalog.fabric.length} 个
                  </summary>
                  <label className="field">
                    具体版本
                    <select
                      aria-label="Fabric 具体版本"
                      value={build}
                      onChange={(event) => setBuild(event.target.value)}
                    >
                      {loaderCatalog.fabric.map((item) => (
                        <option key={item.version} value={item.version}>
                          {item.version} ·{" "}
                          {item.stable ? "稳定（Fabric 元数据）" : "测试"}
                        </option>
                      ))}
                    </select>
                  </label>
                </details>
                <a href={loaderCatalog.source} target="_blank" rel="noreferrer">
                  Fabric 官方元数据
                  <Icon name="arrow" />
                </a>
              </div>
            )}
            {!fabricAvailable && (
              <p className="inline-note">
                此原型只缓存 MC 1.21.1 的 Fabric
                构建。其他版本需查询真实目录，不能推断为“不兼容”。
              </p>
            )}
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">02</span>
              <div>
                <h2>光影支持组件</h2>
                <p>让游戏能够加载光影；这里选择的不是光影包。</p>
              </div>
            </header>
            <div className="support-options">
              {[
                ["none", "不安装", "保持当前配置"],
                ["iris", "Iris", "Fabric · 文件来自公开目录"],
                ["oculus", "Oculus", "待核心支持"],
                ["optifine", "OptiFine", "待授权与兼容性支持"],
              ].map(([id, title, detail]) => (
                <button
                  key={id}
                  aria-pressed={support === id}
                  className={support === id ? "selected" : ""}
                  disabled={
                    id !== "none" && (id !== "iris" || !irisFiles.length)
                  }
                  onClick={() => {
                    setSupport(id);
                    setIrisBuild("");
                  }}
                >
                  <Badge kind={id === "none" ? "close" : id} />
                  <span>
                    <strong>{title}</strong>
                    <small>{detail}</small>
                  </span>
                  <span className="radio-dot" />
                </button>
              ))}
            </div>
            {support === "iris" && (
              <label className="field nested">
                选择 Iris 发行文件
                <select
                  aria-label="Iris 具体版本"
                  value={irisBuild}
                  onChange={(event) => setIrisBuild(event.target.value)}
                >
                  <option value="">请选择兼容构建</option>
                  {irisFiles.map((file) => (
                    <option key={file.id} value={file.id}>
                      {file.version} · {file.type}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {conflict && (
              <div className="banner error" role="alert">
                <Icon name="warning" />
                {scenario === "conflict"
                  ? "冲突场景：选定组件的兼容性校验未通过。需要核心返回具体依赖与冲突，安装入口保持关闭。"
                  : "Iris 的这些发行文件需要 Fabric，请切换主加载器或选择“不安装”。"}
              </div>
            )}
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">03</span>
              <div>
                <h2>依赖与推荐组件</h2>
                <p>必需和可选分别展示，不悄悄安装额外内容。</p>
              </div>
            </header>
            <label className="dependency-row">
              <Badge kind="api" />
              <span>
                <strong>Fabric API</strong>
                <small>
                  {loader === "fabric"
                    ? "可选推荐；不是每个 Fabric 游戏的必需依赖。"
                    : "需要 Fabric 才能使用。"}
                </small>
              </span>
              <span className="tag">可选</span>
              <input
                type="checkbox"
                checked={api && loader === "fabric"}
                disabled={loader !== "fabric"}
                onChange={(event) => setApi(event.target.checked)}
                aria-label="安装 Fabric API"
              />
            </label>
            {api && loader === "fabric" && (
              <label className="field nested">
                Fabric API 具体版本
                <select
                  aria-label="Fabric API 具体版本"
                  value={apiBuild}
                  onChange={(event) => setApiBuild(event.target.value)}
                >
                  <option value="">请选择发行文件</option>
                  {resourceCatalog.projects
                    .find((item) => item.slug === "fabric-api")!
                    .versions.filter((file) =>
                      file.gameVersions.includes(version),
                    )
                    .map((file) => (
                      <option key={file.id} value={file.id}>
                        {file.version} · {file.type}
                      </option>
                    ))}
                </select>
              </label>
            )}
            {support === "iris" &&
              irisBuild &&
              iris.versions
                .find((file) => file.id === irisBuild)
                ?.dependencies.map((dependency, index) => (
                  <div className="dependency-row" key={index}>
                    <Badge kind="mod" />
                    <span>
                      <strong>
                        {resourceCatalog.projects.find(
                          (item) => item.id === dependency.project_id,
                        )?.title || "依赖组件 · 名称待解析"}
                      </strong>
                      <small>
                        {dependency.dependency_type === "required"
                          ? "必需"
                          : dependency.dependency_type === "incompatible"
                            ? "不兼容"
                            : "可选"}{" "}
                        · 已安装状态和具体约束待核心校验
                      </small>
                    </span>
                    <span className="tag">
                      {dependency.dependency_type === "required"
                        ? "必需"
                        : "待检查"}
                    </span>
                  </div>
                ))}
            {support === "iris" && (
              <div className="dependency-row">
                <Badge kind="mod" />
                <span>
                  <strong>Iris 文件依赖</strong>
                  <small>
                    选中具体文件后，由核心解析必需依赖和已有冲突。未校验前不标记通过。
                  </small>
                </span>
                <span className="tag">待校验</span>
              </div>
            )}
          </section>
        </div>
        <aside className="install-side surface">
          <span className="kicker">A NEW CHAPTER</span>
          <h2>
            新的旅程，
            <br />
            由你选择。
          </h2>
          <div className="summary-scene" />
          <dl>
            <div>
              <dt>Minecraft</dt>
              <dd>{version}</dd>
            </div>
            <div>
              <dt>加载器</dt>
              <dd>{loader === "game" ? "纯净原版" : `Fabric ${build}`}</dd>
            </div>
            <div>
              <dt>光影支持</dt>
              <dd>{support === "none" ? "不安装" : "Iris"}</dd>
            </div>
            <div>
              <dt>游戏目录</dt>
              <dd>独立保存</dd>
            </div>
          </dl>
          <p>已有世界不会被新配置覆盖。整合包和资源文件有各自的安装流程。</p>
          <span className="tag">安装编排待核心接入</span>
        </aside>
      </div>
      <footer className="install-footer">
        <div>
          <strong>{name || "请填写安装名称"}</strong>
          <small>
            {conflict
              ? "请先解决组件冲突"
              : "独立版本 · 此原型仅预览安装确认，不执行下载"}
          </small>
        </div>
        <button
          className="button primary"
          disabled={blocked}
          onClick={() => setConfirm(true)}
        >
          <Icon name="download" />
          开始安装
          <Icon name="arrow" />
        </button>
      </footer>
      {confirm && (
        <Modal title="确认安装 · 设计预览" close={() => setConfirm(false)}>
          <div className="banner">
            <Icon name="game" />
            <span>
              {name} / {version}
              <br />
              {loader === "game" ? "纯净原版" : `Fabric ${build}`}{" "}
              {support === "iris" ? "+ Iris" : ""}
            </span>
          </div>
          <p className="body-copy">
            正式版本将在这里调用 Rust 安装计划与兼容性校验，返回任务 ID
            后才进入队列。本轮没有创建任务、下载文件或安装成功。
          </p>
          <button
            className="button primary full"
            onClick={() => setConfirm(false)}
          >
            返回配置
          </button>
        </Modal>
      )}
    </section>
  );
}

export function ResourceInstaller({ slug }: { slug: string }) {
  const project = resourceCatalog.projects.find((item) => item.slug === slug);
  if (!project)
    return <Empty title="未缓存这个项目" detail="原型仅展示公开元数据样本。" />;
  return <ResourceDetail key={slug} project={project} />;
}
function ResourceDetail({ project }: { project: Resource }) {
  const { navigate, versions, scenario } = usePreview();
  const [targetId, setTargetId] = useState("");
  const [fileId, setFileId] = useState("");
  const [channel, setChannel] = useState("all");
  const [world, setWorld] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [packName, setPackName] = useState(project.title);
  const target = versions.find((item) => item.id === targetId);
  const modpack = project.kind === "modpack";
  const datapack = project.kind === "datapack";
  const files = modpack ? project.versions : compatibleFiles(project, target);
  const selectedFile = files.find((item) => item.id === fileId);
  const visibleFiles = files.filter(
    (file) => channel === "all" || file.type === channel,
  );
  const kind =
    project.kind === "support" || project.kind === "component"
      ? "mod"
      : project.kind;
  const path = modpack
    ? "新的独立版本目录（由核心安全分配）"
    : !target
      ? "先选择游戏版本"
      : datapack
        ? world
          ? `${target.name}/saves/${world}/datapacks`
          : "再选择要安装的存档"
        : `${target.name}/${kind === "mod" ? "mods" : kind === "resourcepack" ? "resourcepacks" : "shaderpacks"}`;
  function targetReason(version: (typeof versions)[number]) {
    if (version.shared) return "共享游戏目录 · 为保护其他版本，不允许写入";
    if (!compatibleFiles(project, version).length)
      return version.gameVersion !== "1.21.1"
        ? "快照仅含 1.21.1 文件，其他版本待实时查询"
        : "当前快照没有匹配此加载器的发行文件";
    return "有匹配元数据的文件 · 仍需核心校验依赖";
  }
  const blocked =
    (!modpack && (!target || target.shared)) ||
    !selectedFile ||
    (datapack && !world) ||
    scenario === "conflict" ||
    scenario === "offline" ||
    (modpack && !packName.trim());
  return (
    <section
      className="content-page installer-page resource-detail-page"
      data-page="resource-installer"
    >
      <button
        className="breadcrumb"
        onClick={() => navigate(`download/${kind}`)}
      >
        <Icon name="back" />
        下载 /{" "}
        {modpack
          ? "整合包"
          : datapack
            ? "数据包"
            : CATEGORIES.find((item) => item.id === kind)?.name}
      </button>
      <header className="project-heading">
        <Badge
          kind={
            project.kind === "support"
              ? "iris"
              : project.kind === "component"
                ? "api"
                : kind
          }
          large
        />
        <div>
          <div className="kicker">MODRINTH / {kind.toUpperCase()}</div>
          <h1 tabIndex={-1}>{project.title}</h1>
          <p>{project.description}</p>
          <div className="project-meta">
            <span>
              作者：
              {project.authors.length
                ? project.authors.join(" / ")
                : "信息未缓存，见项目主页"}
            </span>
            <span>
              {new Intl.NumberFormat("zh-CN").format(project.downloads)} 次下载
            </span>
            <span>更新于 {date(project.updated)}</span>
            <a href={project.projectUrl} target="_blank" rel="noreferrer">
              作者与项目主页
              <Icon name="arrow" />
            </a>
          </div>
        </div>
      </header>
      <div className="install-layout">
        <div className="install-main">
          <section className="surface component-section">
            <header>
              <span className="step-number">01</span>
              <div>
                <h2>
                  {modpack ? "为整合包创建独立版本" : "选择安装到哪个游戏版本"}
                </h2>
                <p>
                  {modpack
                    ? "专用安装向导：不把整合包覆盖到已有版本或世界。"
                    : "先选目标，再看这个目标可用的发行文件。"}
                </p>
              </div>
            </header>
            {modpack ? (
              <>
                <label className="field">
                  新版本名称
                  <input
                    aria-label="整合包版本名称"
                    value={packName}
                    onChange={(event) => setPackName(event.target.value)}
                  />
                </label>
                <div className="banner">
                  <Icon name="folder" />
                  独立的
                  mods、config、saves；包内覆盖文件需先安全检查。完成真实安装后才加入“我的版本”。
                </div>
              </>
            ) : (
              <div className="target-grid">
                {versions.map((version) => {
                  const available =
                    compatibleFiles(project, version).length > 0;
                  return (
                    <button
                      key={version.id}
                      className={targetId === version.id ? "selected" : ""}
                      disabled={!available}
                      aria-pressed={targetId === version.id}
                      onClick={() => {
                        setTargetId(version.id);
                        setFileId("");
                        setWorld("");
                      }}
                    >
                      <Badge kind={version.loader} />
                      <div>
                        <strong>{version.name}</strong>
                        <span>
                          {version.gameVersion} ·{" "}
                          {version.loader === "game"
                            ? "纯净原版"
                            : `${version.loader} ${version.loaderVersion}`}
                        </span>
                        <small>{targetReason(version)}</small>
                      </div>
                      <span className="radio-dot" />
                    </button>
                  );
                })}
                {!versions.length && (
                  <Empty
                    title="先下载一个游戏版本"
                    detail="资源需要明确的安装目标，不能写入未知目录。"
                    action={
                      <button
                        className="button primary"
                        onClick={() => navigate("download/game")}
                      >
                        下载游戏
                      </button>
                    }
                  />
                )}
              </div>
            )}
            {datapack && target && (
              <label className="field nested">
                选择目标存档
                <select
                  aria-label="目标存档"
                  value={world}
                  onChange={(event) => setWorld(event.target.value)}
                >
                  <option value="">请选择存档（示例档案）</option>
                  <option value="pine_home">松林小屋 · 示例</option>
                  <option value="far_base">远行基地 · 示例</option>
                </select>
                <small>只安装到此存档的 datapacks，不能写到游戏根目录。</small>
              </label>
            )}
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">02</span>
              <div>
                <h2>选择资源文件版本</h2>
                <p>
                  {!target && !modpack
                    ? "先选择上面的目标游戏版本。"
                    : `${files.length} 个匹配清单文件；可自由选择具体版本。`}
                </p>
              </div>
            </header>
            <div className="segmented wrap file-channels">
              {[
                ["all", "全部"],
                ["release", "正式版"],
                ["beta", "Beta"],
                ["alpha", "Alpha"],
              ].map(([id, title]) => (
                <button
                  key={id}
                  disabled={!target && !modpack}
                  aria-pressed={channel === id}
                  onClick={() => setChannel(id)}
                >
                  {title}
                </button>
              ))}
            </div>
            <div className="file-list">
              {visibleFiles.map((file) => (
                <FileRow
                  key={file.id}
                  file={file}
                  selected={file.id === fileId}
                  select={() => setFileId(file.id)}
                />
              ))}
              {(target || modpack) && !visibleFiles.length && (
                <p className="muted">
                  当前快照中没有此渠道的匹配文件。不会自动安装其他版本。
                </p>
              )}
            </div>
          </section>
          <section className="surface component-section">
            <header>
              <span className="step-number">03</span>
              <div>
                <h2>依赖与冲突检查</h2>
                <p>下载前解释每一项变化。</p>
              </div>
            </header>
            {selectedFile ? (
              <>
                <div
                  className={`banner ${scenario === "conflict" ? "error" : "warn"}`}
                >
                  <Icon name="warning" />
                  {scenario === "conflict"
                    ? "冲突样例：选定文件与已有组件存在版本约束冲突，不能继续安装。"
                    : "版本与加载器仅通过元数据初筛。完整依赖、冲突和路径校验仍需 Rust 返回计划，此处不标记“兼容已验证”。"}
                </div>
                {selectedFile.dependencies.length ? (
                  selectedFile.dependencies.map((dependency, index) => (
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
                          {resourceCatalog.projects.find(
                            (item) => item.id === dependency.project_id,
                          )?.title || "依赖名称待解析"}{" "}
                          · 已安装状态待核心扫描
                        </small>
                      </span>
                      <span className="tag">存在状态待扫描</span>
                    </div>
                  ))
                ) : (
                  <p className="muted">
                    该发行元数据没有声明依赖；这不等于已经通过安装安全检查。
                  </p>
                )}
              </>
            ) : (
              <p className="muted">
                选择一个具体发行文件后，展示它的依赖、可选内容与冲突。
              </p>
            )}
          </section>
        </div>
        <aside className="surface install-side resource-side">
          <span className="kicker">INSTALL PREVIEW</span>
          <Badge kind={kind} large />
          <h2>
            每个资源，
            <br />
            都有正确的位置。
          </h2>
          <dl>
            <div>
              <dt>目标版本</dt>
              <dd>{modpack ? packName : target?.name || "尚未选择"}</dd>
            </div>
            <div>
              <dt>发行文件</dt>
              <dd>{selectedFile?.version || "尚未选择"}</dd>
            </div>
            <div>
              <dt>目录</dt>
              <dd className="break">{path}</dd>
            </div>
            <div>
              <dt>文件大小</dt>
              <dd>
                {selectedFile
                  ? size(
                      selectedFile.files.find((file) => file.primary)?.size ||
                        selectedFile.files[0]?.size ||
                        0,
                    )
                  : "—"}
              </dd>
            </div>
          </dl>
          <p>
            来源 Modrinth · {date(resourceCatalog.capturedAt)} 快照
            <br />
            不下载文件，不修改真实游戏目录。
          </p>
          <a href={project.projectUrl} target="_blank" rel="noreferrer">
            项目主页与许可
            <Icon name="arrow" />
          </a>
        </aside>
      </div>
      <footer className="install-footer">
        <div>
          <strong>
            {selectedFile
              ? `${project.title} · ${selectedFile.version}`
              : "选择目标与发行文件"}
          </strong>
          <small>{path} · 核心安装计划待接入</small>
        </div>
        <button
          className="button primary"
          disabled={blocked}
          onClick={() => setConfirm(true)}
        >
          <Icon name="download" />
          {modpack ? "预览整合包安装" : "确认安装"}
          <Icon name="arrow" />
        </button>
      </footer>
      {confirm && (
        <Modal
          title={modpack ? "整合包向导 · 确认预览" : "资源安装 · 确认预览"}
          close={() => setConfirm(false)}
        >
          <p className="body-copy">
            {project.title} → {modpack ? packName : target?.name}
            <br />
            文件：{selectedFile?.version}
            <br />
            位置：{path}
          </p>
          <div className="banner warn">
            <Icon name="warning" />
            尚无核心安全安装计划，不创建任务或显示成功。正式流程需要处理依赖、哈希、目录隔离和原子写入。
          </div>
          <button
            className="button secondary full"
            onClick={() => setConfirm(false)}
          >
            返回检查
          </button>
        </Modal>
      )}
    </section>
  );
}
function FileRow({
  file,
  selected,
  select,
}: {
  file: ResourceFileVersion;
  selected: boolean;
  select: () => void;
}) {
  return (
    <button
      className={`file-option ${selected ? "selected" : ""}`}
      aria-pressed={selected}
      onClick={select}
    >
      <span className="radio-dot" />
      <div>
        <strong>{file.version}</strong>
        <p>
          {date(file.published)} ·{" "}
          {size(
            file.files.find((item) => item.primary)?.size ||
              file.files[0]?.size ||
              0,
          )}{" "}
          · {file.loaders.join(" / ")}
        </p>
        <small>
          Minecraft {file.gameVersions.slice(0, 4).join(" / ")}
          {file.gameVersions.length > 4 ? " 等" : ""}
        </small>
      </div>
      <div>
        <span className={`tag channel-${file.type}`}>
          {file.type === "release"
            ? "正式版"
            : file.type === "beta"
              ? "Beta"
              : "Alpha"}
        </span>
        {file.featured && <span className="tag">项目方精选</span>}
      </div>
    </button>
  );
}
