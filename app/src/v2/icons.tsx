import type { SVGProps } from "react";

// Original Astral pictograms. No launcher code, third-party logos or game textures.
const paths = {
  star: <path d="m12 2 2.8 7.2L22 12l-7.2 2.8L12 22l-2.8-7.2L2 12l7.2-2.8Z" />,
  play: <path d="m8 4 12 8-12 8Z" />,
  game: (
    <>
      <path d="m3 7 9-5 9 5-9 5Z" />
      <path d="M3 7v10l9 5V12m0 10 9-5V7M3 12l4-1 3 3m5-2 3 1 3-2" />
    </>
  ),
  snapshot: (
    <>
      <path d="M9 3h6M10 3v6L4 19q-1 2 2 2h12q3 0 2-2L14 9V3M7 15h10" />
      <path d="M10 18h.1M15 17h.1" />
    </>
  ),
  history: (
    <>
      <path d="M5 4h14M5 20h14M7 4v4l10 8v4M17 4v4L7 16v4" />
    </>
  ),
  special: (
    <>
      <path d="m12 2 8 10-8 10L4 12Z" />
      <path d="M4 12h16M12 2l-3 10 3 10 3-10Z" />
    </>
  ),
  installed: (
    <>
      <path d="m3 7 9-5 9 5-9 5Z" />
      <path d="M3 7v10l9 5 9-5V7M12 12v10m-5-7 3 2 7-5" />
    </>
  ),
  fabric: (
    <>
      <path d="M5 3h14v18H5Z" />
      <path d="M9 3v18M15 3v18M5 7h14M5 12h14M5 17h14" />
    </>
  ),
  forge: (
    <>
      <path d="M3 8h18l-3 5h-5v4h4v3H7v-3h3v-4H5Zm2-5h8v5H5Z" />
    </>
  ),
  neoforge: (
    <>
      <path d="m3 3 6 4h6l6-4-2 12-7 7-7-7Z" />
      <path d="m5 11 5 2-3 3m12-5-5 2 3 3m-7 2h4" />
    </>
  ),
  quilt: (
    <>
      <path d="M3 3h18v18H3Z" />
      <path d="m3 3 9 9 9-9M3 21l9-9 9 9M3 12h18M12 3v18" />
    </>
  ),
  iris: (
    <>
      <path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12Z" />
      <circle cx="12" cy="12" r="4" />
      <path d="M12 8v4l3 2" />
    </>
  ),
  oculus: (
    <>
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="4" />
      <path d="m5 6 4 4m6 4 4 4M5 18l4-4m6-4 4-4" />
    </>
  ),
  optifine: (
    <>
      <rect x="3" y="5" width="8" height="14" rx="4" />
      <path d="M15 19V5h7m-7 7h6" />
    </>
  ),
  api: (
    <>
      <path d="M3 7h6V3h6v4h6v6h-4a3 3 0 1 1-5 2v6H3Z" />
    </>
  ),
  mod: (
    <>
      <path d="M3 3h7v4a3 3 0 1 1 5 2h6v12h-7v-4a3 3 0 1 0-5-2v6H3Z" />
    </>
  ),
  modpack: (
    <>
      <path d="m3 6 9-4 9 4v13l-9 3-9-3Z" />
      <path d="m3 6 9 4 9-4M12 10v12M8 4l9 4v5" />
    </>
  ),
  resourcepack: (
    <>
      <path d="M3 3h18v18H3Z" />
      <path d="M3 9h18M3 15h18M9 3v6m6 0v6m-6 0v6" />
    </>
  ),
  shaderpack: (
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 1v3m0 16v3M1 12h3m16 0h3M4 4l3 3m10 10 3 3M4 20l3-3M17 7l3-3" />
    </>
  ),
  datapack: (
    <>
      <path d="m3 6 9-4 9 4v13l-9 3-9-3Z" />
      <path d="M12 10v12m-9-16 9 4 9-4M7 14l-2 2 2 2m10-6 2 2-2 2" />
    </>
  ),
  download: <path d="M12 3v12m-5-5 5 5 5-5M3 17v4h18v-4" />,
  settings: (
    <>
      <path d="M4 7h16M4 17h16" />
      <circle cx="9" cy="7" r="3" />
      <circle cx="16" cy="17" r="3" />
    </>
  ),
  more: (
    <>
      <circle cx="5" cy="12" r="1" />
      <circle cx="12" cy="12" r="1" />
      <circle cx="19" cy="12" r="1" />
    </>
  ),
  arrow: <path d="M4 12h16m-6-6 6 6-6 6" />,
  back: <path d="M20 12H4m6-6-6 6 6 6" />,
  chevron: <path d="m6 9 6 6 6-6" />,
  search: (
    <>
      <circle cx="10" cy="10" r="7" />
      <path d="m15 15 6 6" />
    </>
  ),
  check: <path d="m4 12 5 5L20 5" />,
  close: <path d="m5 5 14 14M5 19 19 5" />,
  folder: <path d="M3 5h6l3 3h9v13H3Z" />,
  user: (
    <>
      <circle cx="12" cy="8" r="4" />
      <path d="M4 21a8 8 0 0 1 16 0" />
    </>
  ),
  moon: <path d="M20 14A9 9 0 0 1 10 4a9 9 0 1 0 10 10Z" />,
  log: (
    <>
      <path d="M5 3h10l4 4v14H5Z" />
      <path d="M14 3v5h5M8 12h8M8 16h6" />
    </>
  ),
  warning: (
    <>
      <path d="m12 3 10 18H2Z" />
      <path d="M12 9v5m0 3v1" />
    </>
  ),
  refresh: (
    <>
      <path d="M20 4v6h-6M4 20v-6h6" />
      <path d="M5 8a8 8 0 0 1 14-3l1 3M4 16l1 3a8 8 0 0 0 14-3" />
    </>
  ),
} as const;
export type IconKey = keyof typeof paths;
export function Icon({
  name,
  ...props
}: SVGProps<SVGSVGElement> & { name: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="20"
      height="20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {paths[name as IconKey] || paths.game}
    </svg>
  );
}
export function Badge({
  kind,
  large = false,
}: {
  kind: string;
  large?: boolean;
}) {
  return (
    <span className={`glyph glyph-${kind} ${large ? "large" : ""}`}>
      <Icon name={kind} />
    </span>
  );
}
