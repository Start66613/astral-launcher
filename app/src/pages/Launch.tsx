import { NEWS, SAVES, STATS, VERSIONS } from "../lib/data";
import type { SystemInfo } from "../lib/types";
import { Card, Row, Svg } from "../components/ui";

export default function Launch({
  onOpenAi,
  system,
}: {
  onOpenAi: () => void;
  system: SystemInfo | null;
}) {
  return (
    <section className="page">
      <div className="page-head">
        <h1>欢迎回来</h1>
        <p>
          10 月 22 日 星期四 · 上次游玩 3 小时前
          {system?.demo && " · 演示数据"}
        </p>
      </div>

      <div className="cols">
        <div className="col">
          <Card title="版本" meta={`${VERSIONS.length} 个`} action={<button className="link">管理</button>}>
            <ul>
              {VERSIONS.map((v) => (
                <Row
                  key={v.id}
                  icon={v.id.slice(0, 1)}
                  title={
                    <>
                      {v.id}
                      {v.current && <span className="chip acc">当前</span>}
                    </>
                  }
                  sub={v.loader}
                  right={
                    <>
                      <span>{v.hours} h</span>
                      <button className="row-btn">启动</button>
                    </>
                  }
                />
              ))}
            </ul>
          </Card>

          <Card
            title="存档"
            meta="最近游玩"
            action={<button className="link">打开文件夹</button>}
          >
            <ul>
              {SAVES.map((s) => (
                <Row
                  key={s.name}
                  icon={s.name.slice(0, 1)}
                  title={s.name}
                  sub={s.desc}
                  right={
                    <>
                      <span>{s.when}</span>
                      <button className="row-btn">继续</button>
                    </>
                  }
                />
              ))}
            </ul>
          </Card>
        </div>

        <div className="col">
          <Card title="公告" meta={`${NEWS.length} 条`} action={<button className="link">全部</button>}>
            <ul className="news">
              {NEWS.map((n) => (
                <li key={n.title}>
                  <span className="dot" />
                  <div>
                    <b>{n.title}</b>
                    <span>{n.desc}</span>
                  </div>
                  <em>{n.date}</em>
                </li>
              ))}
            </ul>
          </Card>

          <Card title="本周" meta="近 7 天">
            <div className="stats">
              {STATS.map((s) => (
                <div className="stat" key={s.label}>
                  <em>{s.value}</em>
                  <i>{s.unit}</i>
                  <p>{s.label}</p>
                </div>
              ))}
            </div>
          </Card>

          <div className="card ai-card">
            <div className="ai-top">
              <div className="ai-ico">
                <Svg>
                  <path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" />
                </Svg>
              </div>
              <div>
                <b>AI 诊断</b>
                <p>上次启动崩溃 1 次，报告已保存</p>
              </div>
            </div>
            <button className="btn btn-soft" onClick={onOpenAi}>
              查看崩溃分析
            </button>
          </div>
        </div>
      </div>

      <div className="card" style={{ marginTop: 14 }}>
        <div className="card-head">
          <h2>快捷操作</h2>
          <span className="meta">常用目录</span>
        </div>
        <div className="quick">
          <button className="qbtn">
            打开游戏目录
            <small>1.21.1</small>
          </button>
          <button className="qbtn">
            截图文件夹
            <small>142 张</small>
          </button>
          <button className="qbtn">
            最近日志
            <small>latest.log</small>
          </button>
          <button className="qbtn">
            游戏设置
            <small>options.txt</small>
          </button>
        </div>
      </div>
    </section>
  );
}
