import { useEffect, useMemo, useRef, useState } from "react";
import {
  inTauri,
  installVersion,
  listVersions,
  onProgress,
  scanVersions,
  type Progress,
} from "../lib/bridge";
import type { VersionSummary } from "../lib/types";
import { Acc, Card } from "../components/ui";
import Resources from "./Resources";
import type { Section } from "../App";

const KIND_LABEL: Record<string, string> = {
  release: "正式版",
  snapshot: "预览版",
  old_beta: "远古版",
  old_alpha: "远古版",
};

function fmtDate(iso: string) {
  return iso ? iso.slice(0, 10) : "";
}

/** 浏览器里没 Rust，用假数据把界面撑起来 */
const MOCK: VersionSummary[] = [
  { id: "1.21.4", kind: "release", released: "2024-12-03T00:00:00+00:00" },
  { id: "1.21.1", kind: "release", released: "2024-08-08T00:00:00+00:00" },
  { id: "1.20.1", kind: "release", released: "2023-06-12T00:00:00+00:00" },
  { id: "1.19.2", kind: "release", released: "2022-08-05T00:00:00+00:00" },
  { id: "1.21.5-pre2", kind: "snapshot", released: "2025-01-08T00:00:00+00:00" },
  { id: "Beta 1.7.3", kind: "old_beta", released: "2011-07-08T00:00:00+00:00" },
];

function VersionRow({
  v,
  installed,
  busy,
  pct,
  label,
  onInstall,
}: {
  v: VersionSummary;
  installed: boolean;
  busy: boolean;
  pct: number;
  label: string;
  onInstall: () => void;
}) {
  return (
    <li className="row">
      <div className="row-ico">{v.id.slice(0, 1)}</div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="row-name">
          {v.id}
          {installed && <span className="chip acc">已安装</span>}
        </div>
        <div className="row-sub">
          {fmtDate(v.released)} · {KIND_LABEL[v.kind] ?? v.kind}
        </div>
        {busy && (
          <>
            <div className="progress" style={{ marginTop: 6, maxWidth: 260 }}>
              <i style={{ width: `${pct}%` }} />
            </div>
            <div className="row-sub" style={{ marginTop: 3 }}>
              {label}
            </div>
          </>
        )}
      </div>
      <div className="row-r">
        {busy ? (
          <span style={{ fontVariantNumeric: "tabular-nums" }}>{pct}%</span>
        ) : (
          <button className="row-btn primary" onClick={onInstall}>
            {installed ? "补全" : "安装"}
          </button>
        )}
      </div>
    </li>
  );
}

export default function Download({
  section,
  onSection,
}: {
  section: Section;
  onSection: (s: Section) => void;
}) {
  if (section !== "vanilla") {
    return <Resources kind={section} onGoVanilla={() => onSection("vanilla")} />;
  }
  return <Vanilla />;
}

function Vanilla() {
  const [versions, setVersions] = useState<VersionSummary[] | null>(null);
  const [installed, setInstalled] = useState<string[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [prog, setProg] = useState<Progress | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const busyRef = useRef<string | null>(null);

  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);

  useEffect(() => {
    let alive = true;
    if (!inTauri) {
      setVersions(MOCK);
      setInstalled(["1.20.1"]);
      return;
    }
    void listVersions()
      .then((v) => alive && setVersions(v))
      .catch((e) => alive && setErr(String(e)));
    void scanVersions().then((v) => alive && setInstalled(v));
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    let un: (() => void) | undefined;
    void onProgress((v, p) => {
      if (v === busyRef.current) setProg(p);
    }).then((f) => {
      un = f;
    });
    return () => un?.();
  }, []);

  const groups = useMemo(() => {
    const list = versions ?? [];
    return {
      release: list.filter((v) => v.kind === "release"),
      snapshot: list.filter((v) => v.kind === "snapshot"),
      old: list.filter((v) => v.kind === "old_beta" || v.kind === "old_alpha"),
    };
  }, [versions]);

  async function install(id: string) {
    if (busy) return;
    setBusy(id);
    setProg(null);
    setErr(null);
    try {
      await installVersion(id);
      setInstalled((prev) => (prev.includes(id) ? prev : [...prev, id]));
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
      setProg(null);
    }
  }

  const pct = prog
    ? prog.total > 0
      ? Math.round((prog.done / prog.total) * 100)
      : 0
    : 0;
  const latest = groups.release[0];
  const snapshot = groups.snapshot[0];

  return (
    <section className="page">
      <div className="page-head">
        <h1>原版游戏</h1>
        <p>
          Mojang 官方源 · SHA1 校验
          {!inTauri && " · 浏览器预览（假数据）"}
        </p>
      </div>

      {err && (
        <div className="card" style={{ padding: "12px 16px", borderColor: "var(--red)" }}>
          <span style={{ fontSize: 12.5, color: "var(--red)" }}>{err}</span>
        </div>
      )}

      {installed.length > 0 && (
        <div
          className="card"
          style={{ padding: "10px 16px", display: "flex", alignItems: "center", gap: 10 }}
        >
          <span style={{ fontSize: 12.5, color: "var(--t2)" }}>
            本地已装 {installed.length} 个版本：
            <span style={{ color: "var(--t3)" }}>{installed.slice(-6).join(" · ")}</span>
          </span>
        </div>
      )}

      <Card title="最新版本">
        {versions === null ? (
          <div style={{ padding: "14px 16px", fontSize: 12.5, color: "var(--t3)" }}>
            正在拉取版本清单…
          </div>
        ) : (
          <ul>
            {[latest, snapshot].filter(Boolean).map((v) => (
              <VersionRow
                key={v!.id}
                v={v!}
                installed={installed.includes(v!.id)}
                busy={busy === v!.id}
                pct={pct}
                label={prog?.label ?? ""}
                onInstall={() => void install(v!.id)}
              />
            ))}
          </ul>
        )}
      </Card>

      <Acc title="正式版" meta={`${groups.release.length} 个`} defaultOpen>
        <ul>
          {groups.release.slice(0, 12).map((v) => (
            <VersionRow
              key={v.id}
              v={v}
              installed={installed.includes(v.id)}
              busy={busy === v.id}
              pct={pct}
              label={prog?.label ?? ""}
              onInstall={() => void install(v.id)}
            />
          ))}
        </ul>
      </Acc>

      <Acc title="预览版" meta={`${groups.snapshot.length} 个`}>
        <ul>
          {groups.snapshot.slice(0, 10).map((v) => (
            <VersionRow
              key={v.id}
              v={v}
              installed={installed.includes(v.id)}
              busy={busy === v.id}
              pct={pct}
              label={prog?.label ?? ""}
              onInstall={() => void install(v.id)}
            />
          ))}
        </ul>
      </Acc>

      <Acc title="远古版" meta={`${groups.old.length} 个`}>
        <ul>
          {groups.old.slice(0, 10).map((v) => (
            <VersionRow
              key={v.id}
              v={v}
              installed={installed.includes(v.id)}
              busy={busy === v.id}
              pct={pct}
              label={prog?.label ?? ""}
              onInstall={() => void install(v.id)}
            />
          ))}
        </ul>
      </Acc>

      <p className="note">
        安装逻辑基于 portablemc（Apache-2.0）：版本清单 → 元数据 → 依赖库 → 资源索引 → 并行下载 + SHA1 校验。
        文件全部来自 Mojang 官方 CDN，不做二次分发。
      </p>
    </section>
  );
}
