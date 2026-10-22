import { useEffect, useState } from "react";
import { downloadTo, type ResourceKind } from "../lib/bridge";

const API = "https://api.modrinth.com/v2";

interface Hit {
  project_id: string;
  title: string;
  description: string;
  icon_url: string | null;
  downloads: number;
  categories: string[];
}

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

  useEffect(() => {
    let alive = true;
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
      fetch(url)
        .then((r) => {
          if (!r.ok) throw new Error(`HTTP ${r.status}`);
          return r.json();
        })
        .then((j) => alive && setHits(j.hits ?? []))
        .catch((e) => alive && setErr(String(e)));
    }, 320);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [q, ver, loader, kind]);

  async function install(hit: Hit) {
    if (busy) return;
    setBusy(hit.project_id);
    setPct(0);
    setErr(null);
    try {
      const params = new URLSearchParams();
      if (ver) params.set("game_versions", JSON.stringify([ver]));
      if (loader && (kind === "mod" || kind === "modpack"))
        params.set("loaders", JSON.stringify([loader]));
      const vs = await fetch(
        `${API}/project/${hit.project_id}/version?${params.toString()}`
      ).then((r) => r.json());
      const file = (vs?.[0]?.files ?? []).find((f: any) => f.primary) ?? vs?.[0]?.files?.[0];
      if (!file) throw new Error("这个筛选条件下没有可用文件");
      await downloadTo(file.url, kind, file.filename, setPct);
      setDone((d) => [...d, hit.project_id]);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
      setPct(0);
    }
  }

  return (
    <section className="page">
      <div className="page-head">
        <h1>{kind === "mod" ? "Mod" : kind === "modpack" ? "整合包" : kind === "resourcepack" ? "资源包" : kind === "shaderpack" ? "光影包" : "数据包"}</h1>
        <p>来源 Modrinth · 直接下载到游戏目录</p>
        <button className="link" onClick={onGoVanilla} style={{ marginLeft: "auto" }}>
          回原版游戏
        </button>
      </div>

      <div className="toolbar">
        <div className="search" style={{ maxWidth: 300 }}>
          <svg viewBox="0 0 24 24">
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-4-4" />
          </svg>
          <input
            placeholder="搜索名称，如 sodium / jei"
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </div>
        <select className="sel" value={ver} onChange={(e) => setVer(e.target.value)}>
          {GAME_VERSIONS.map((v) => (
            <option key={v} value={v}>
              {v || "全部版本"}
            </option>
          ))}
        </select>
        {(kind === "mod" || kind === "modpack") && (
          <select className="sel" value={loader} onChange={(e) => setLoader(e.target.value)}>
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
              disabled={busy === h.project_id}
              onClick={() => void install(h)}
            >
              {done.includes(h.project_id)
                ? "已安装"
                : busy === h.project_id
                ? `${pct}%`
                : "安装"}
            </button>
          </article>
        ))}
      </div>
    </section>
  );
}
