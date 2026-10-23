import type { SVGProps } from "react";

const paths = {
  star: <path d="m12 2 2.8 7.2L22 12l-7.2 2.8L12 22l-2.8-7.2L2 12l7.2-2.8Z" />,
  home: <><path d="m3 10 9-7 9 7v10H3Z" /><path d="M9 20v-7h6v7" /></>,
  layers: <><path d="m12 3 10 5-10 5L2 8Z" /><path d="m3 12 9 5 9-5M3 16l9 5 9-5" /></>,
  download: <path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5" />,
  mods: <path d="M8 3h4v4a3 3 0 1 1 5 2h4v6h-4a3 3 0 1 1-5 2v4H3v-7h4a3 3 0 1 0 1-5Z" />,
  settings: <><circle cx="12" cy="12" r="3" /><path d="m10 3-1 3-3 1-3 3 2 3-1 4 4 1 2 3 4-2 4 1 1-4 2-3-3-3-1-4-4 1Z" /></>,
  spark: <path d="m12 3 2.3 6.7L21 12l-6.7 2.3L12 21l-2.3-6.7L3 12l6.7-2.3ZM20 2v4m-2-2h4" />,
  arrow: <path d="M4 12h16m-6-6 6 6-6 6" />,
  arrowUp: <path d="M5 19 19 5M5 5h14v14" />,
  down: <path d="m6 9 6 6 6-6" />,
  user: <><circle cx="12" cy="8" r="4" /><path d="M4 22v-3a8 8 0 0 1 16 0v3" /></>,
  moon: <path d="M21 13A9 9 0 0 1 11 3a9 9 0 1 0 10 10Z" />,
  sun: <><circle cx="12" cy="12" r="4" /><path d="M12 1v2m0 18v2M1 12h2m18 0h2M4 4l2 2m12 12 2 2M4 20l2-2M18 6l2-2" /></>,
  plus: <path d="M12 5v14M5 12h14" />,
  play: <path d="m8 5 11 7-11 7Z" />,
  check: <path d="m5 12 4 4L20 5" />,
  close: <path d="m6 6 12 12M6 18 18 6" />,
  folder: <path d="M3 5h6l3 3h9v12H3Z" />,
  leaf: <path d="M20 3C10 2 3 6 4 13c1 7 12 9 15-1 1-3 1-6 1-9ZM5 21 16 9" />,
  compass: <><circle cx="12" cy="12" r="9" /><path d="m16 8-3 5-5 3 3-5Z" /></>,
  log: <><path d="M6 3h9l4 4v14H6Z" /><path d="M14 3v5h5M9 12h7M9 16h5" /></>,
  search: <><circle cx="10.5" cy="10.5" r="6.5" /><path d="m16 16 5 5" /></>,
  refresh: <><path d="M20 7v5h-5M4 17v-5h5" /><path d="M5.2 8a7.5 7.5 0 0 1 12.5-3L20 8M4 16l2.3 3A7.5 7.5 0 0 0 19 16" /></>,
  help: <><circle cx="12" cy="12" r="9" /><path d="M9.5 9a2.5 2.5 0 1 1 3.6 2.2c-.8.4-1.1 1-1.1 2M12 17h.01" /></>,
  minimize: <path d="M5 12h14" />,
  maximize: <rect x="5" y="5" width="14" height="14" rx="1" />,
} as const;

export type IconName = keyof typeof paths;
export default function Icon({ name, className = "", ...props }: SVGProps<SVGSVGElement> & { name: IconName }) {
  return <svg viewBox="0 0 24 24" className={`icon ${className}`} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" {...props}>{paths[name]}</svg>;
}
