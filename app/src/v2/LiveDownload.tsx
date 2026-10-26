import { MotionRegion, MotionDetails } from "./motion";
import { useEffect, useState } from "react";
import { Badge, Icon } from "./icons";
import { CATEGORIES, catalog, date, size } from "./model";
import { usePreview } from "./state";
import { Empty, PageTitle } from "./App";
import { GameCatalog } from "./Download";
import { MODRINTH, publicJson, type Project } from "./live-api";
import { inTauri, listVersions } from "../lib/bridge";

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
