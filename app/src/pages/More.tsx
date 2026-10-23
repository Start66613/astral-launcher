import { useState } from "react";
import { HELP_SECTIONS } from "../lib/data";
import { Acc, Row } from "../components/ui";

export default function More() {
  const [q, setQ] = useState("");
  const kw = q.trim();

  const sections = HELP_SECTIONS.map((s) => ({
    ...s,
    items: s.items.filter(
      (it) => !kw || it.title.includes(kw) || it.desc.includes(kw)
    ),
  })).filter((s) => s.items.length > 0);

  return (
    <section className="page">
      <div className="page-head">
        <div><span className="eyebrow">A LITTLE HELP ALONG THE WAY</span><h1 id="page-title" tabIndex={-1}>帮助与说明</h1>
        <p>遇到岔路时，先在这里找找方向。</p></div>
      </div>

      <div className="search">
        <svg viewBox="0 0 24 24">
          <circle cx="11" cy="11" r="7" />
          <path d="m20 20-4-4" />
        </svg>
        <input
          placeholder="搜索帮助"
          aria-label="搜索帮助"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
      </div>

      {sections.map((s, i) => (
        <Acc key={s.title} title={s.title} defaultOpen={i === 0}>
          <ul>
            {s.items.map((it) => (
              <Row key={it.title} icon="?" title={it.title} sub={it.desc} />
            ))}
          </ul>
        </Acc>
      ))}

      {sections.length === 0 && (
        <div className="card" style={{ padding: "28px 16px", textAlign: "center" }}>
          <span style={{ color: "var(--t3)", fontSize: 13 }}>
            没有找到「{kw}」相关的条目
          </span>
        </div>
      )}
    </section>
  );
}
