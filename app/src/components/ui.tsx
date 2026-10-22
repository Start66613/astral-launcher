import type { ReactNode } from "react";

export function Svg({ children }: { children: ReactNode }) {
  return <svg viewBox="0 0 24 24">{children}</svg>;
}

export function Card({
  title,
  meta,
  action,
  children,
}: {
  title?: string;
  meta?: ReactNode;
  action?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="card">
      {title && (
        <div className="card-head">
          <h2>{title}</h2>
          {meta && <span className="meta">{meta}</span>}
          <div className="sp" />
          {action}
        </div>
      )}
      {children}
    </div>
  );
}

export function Acc({
  title,
  meta,
  defaultOpen,
  children,
}: {
  title: string;
  meta?: string;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  return (
    <details className="acc" open={defaultOpen}>
      <summary className="acc-head">
        {title}
        {meta && <span className="chip">{meta}</span>}
        <Svg>
          <path d="m6 9 6 6 6-6" />
        </Svg>
      </summary>
      <div className="acc-body">{children}</div>
    </details>
  );
}

export function Row({
  icon,
  title,
  sub,
  right,
  onClick,
}: {
  icon?: ReactNode;
  title: ReactNode;
  sub?: ReactNode;
  right?: ReactNode;
  onClick?: () => void;
}) {
  return (
    <li className="row" onClick={onClick}>
      {icon !== undefined && <div className="row-ico">{icon}</div>}
      <div>
        <div className="row-name">{title}</div>
        {sub && <div className="row-sub">{sub}</div>}
      </div>
      {right && <div className="row-r">{right}</div>}
    </li>
  );
}
