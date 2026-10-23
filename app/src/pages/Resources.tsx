import { useEffect, useRef, useState } from "react";
import { downloadTo, inTauri, type ResourceKind } from "../lib/bridge";
import Icon from "../components/Icon";

const API = "https://api.modrinth.com/v2";

interface Hit {
  project_id: string;
  title: string;
  description: string;
  icon_url: string | null;
  downloads: number;
  categories: string[];
}

interface ResourceVersion { files?: { primary?: boolean; url: string; filename: string }[] }

const TYPE: Record<string, string> = {
  mod: "mod",
  modpack: "modpack",
  resourcepack: "resourcepack",
  shaderpack: "shader",
  datapack: "datapack",
};

const GAME_VERSIONS = ["", "1.21.4", "1.21.1", "1.20.1", "1.19.2", "1.18.2", "1.16.5"];
const LOADERS = ["", "fabric", "forge", "neoforge", "quilt"];

function fmtDownloads(n: number) {
  if (n >= 1e8) return `${(n / 1e8).toFixed(1)} 亿`;
  if (n >= 1e4) return `${Math.round(n / 1e4)} 万`;
  return String(n);
}

export default function Resources({
  kind,
  onGoVanilla,
}: {
  kind: ResourceKind;
  onGoVanilla: () => void;
}) {
  const [q, setQ] = useState("");
  const [ver, setVer] = useState("");
  const [loader, setLoader] = useState("");
  const [hits, setHits] = useState<Hit[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [pct, setPct] = useState(0);
  const [done, setDone] = useState<string[]>([]);
  const downloadLock = useRef(false);

  useEffect(() => {
    let alive = true;
    const controller = new AbortController();
    setHits(null);
    setErr(null);
    const t = window.setTimeout(() => {
      setHits(null);
      setErr(null);
      const facets: string[][] = [[`project_type:${TYPE[kind] ?? "mod"}`]];
      if (ver) facets.push([`versions:${ver}`]);
      if (loader && (kind === "mod" || kind === "modpack"))
        facets.push([`categories:${loader}`]);
      const url =
        `${API}/search?limit=20&index=relevance` +
        `&query=${encodeURIComponent(q)}` +
        `&facets=${encodeURIComponent(JSON.stringify(facets))}`;
      fetch(url, { signal: controller.signal })
        .then((r) => {
          if (!r.ok) throw new Error(`HTTP ${r.status}`);
          return r.json();
        })
        .then((j) => alive && setHits(j.hits ?? []))
        .catch((e) => alive && setErr(String(e)));
    }, 320);
    return () => {
      alive = false;
      controller.abort();
      window.clearTimeout(t);
    };
  }, [q, ver, loader, kind]);

  async function install(hit: Hit) {
    if (!inTauri || downloadLock.current) return;
    downloadLock.current = true;
    setBusy(hit.project_id);
    setPct(0);
    setErr(null);
    try {
      const params = new URLSearchParams();
      if (ver) params.set("game_versions", JSON.stringify([ver]));
      if (loader && (kind === "mod" || kind === "modpack"))
        params.set("loaders", JSON.stringify([loader]));
      const response = await fetch(
        `${API}/project/${hit.project_id}/version?${params.toString()}`
      );
      if (!response.ok) throw new Error(`下载信息读取失败：HTTP ${response.status}`);
      const vs: ResourceVersion[] = await response.json();
      const file = (vs?.[0]?.files ?? []).find(f => f.primary) ?? vs?.[0]?.files?.[0];
      if (!file) throw new Error("这个筛选条件下没有可用文件");
      await downloadTo(file.url, kind, file.filename, setPct);
      setDone((d) => [...d, hit.project_id]);
    } catch (e) {
      setErr(String(e));
    } finally {
      downloadLock.current = false;
      setBusy(null);
      setPct(0);
    }
  }

  return (
    <section className="page">
      <div className="page-head">
        <div><span className="eyebrow">A LITTLE MORE POSSIBILITY</span><h1 id="page-title" tabIndex={-1}>{kind === "mod" ? "模组中心" : kind === "modpack" ? "整合包" : kind === "resourcepack" ? "资源包" : kind === "shaderpack" ? "光影包" : "数据包"}</h1>
        <p>探索来自 Modrinth 社区的更多可能。</p></div>
        <button className="link" onClick={onGoVanilla} style={{ marginLeft: "auto" }}>
          回原版游戏
        </button>
      </div>

      <p className="inline-note">{!inTauri ? "可浏览公开资源；下载需要在桌面端完成。" : "当前下载到游戏目录；独立实例的模组管理暂不可用。"}{kind === "modpack" && " 整合包下载后尚不会自动解包或创建实例。"}</p>

      <div className="toolbar">
        <div className="search" style={{ maxWidth: 300 }}>
          <Icon name="search" />
          <input
            placeholder="搜索名称，如 sodium / jei"
            aria-label="搜索社区资源"
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </div>
        <select className="sel" aria-label="筛选游戏版本" value={ver} onChange={(e) => setVer(e.target.value)}>
          {GAME_VERSIONS.map((v) => (
            <option key={v} value={v}>
              {v || "全部版本"}
            </option>
          ))}
        </select>
        {(kind === "mod" || kind === "modpack") && (
          <select className="sel" aria-label="筛选模组加载器" value={loader} onChange={(e) => setLoader(e.target.value)}>
            {LOADERS.map((v) => (
              <option key={v} value={v}>
                {v || "全部加载器"}
              </option>
            ))}
          </select>
        )}
      </div>

      {err && (
        <div className="card" style={{ padding: "10px 14px" }}>
          <span style={{ fontSize: 12.5, color: "var(--red)" }}>{err}</span>
        </div>
      )}

      {hits === null && !err && (
        <div className="card" style={{ padding: "18px 16px", color: "var(--t3)", fontSize: 12.5 }}>
          正在搜索…
        </div>
      )}

      {hits?.length === 0 && (
        <div className="card" style={{ padding: "18px 16px", color: "var(--t3)", fontSize: 12.5 }}>
          没找到结果，换个关键词试试
        </div>
      )}

      <div className="res-grid">
        {hits?.map((h) => (
          <article className="res-card" key={h.project_id}>
            {h.icon_url ? (
              <img className="res-ico" src={h.icon_url} alt="" loading="lazy" />
            ) : (
              <div className="res-ico res-ico-empty">{h.title.slice(0, 1)}</div>
            )}
            <div className="res-main">
              <div className="res-title">{h.title}</div>
              <p className="res-desc">{h.description}</p>
              <div className="res-meta">
                {fmtDownloads(h.downloads)} 次下载
                {h.categories.length > 0 && ` · ${h.categories.slice(0, 2).join(" / ")}`}
              </div>
            </div>
            <button
              className="row-btn primary"
              disabled={!inTauri || busy !== null}
              title={!inTauri ? "请在桌面端下载" : "下载到当前游戏目录"}
              onClick={() => void install(h)}
            >
              {done.includes(h.project_id)
                ? "已下载"
                : busy === h.project_id
                ? `${pct}%`
                : "下载"}
            </button>
          </article>
        ))}
      </div>
    </section>
  );
}
