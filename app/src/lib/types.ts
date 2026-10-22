export type Tab = "launch" | "download" | "settings" | "more";
export type Theme = "light" | "dark";
export type Accent = "blue" | "green" | "purple" | "orange";
export type Radius = "sharp" | "default" | "soft";
export type Motion = "on" | "off";

export interface Skin {
  theme: Theme;
  accent: Accent;
  radius: Radius;
  motion: Motion;
}

export interface SystemInfo {
  os: string;
  cpuCores: number;
  totalMemGB: number;
  freeMemGB: number;
  /** true 表示运行在浏览器里，数据是假的 */
  demo: boolean;
}

export interface VersionEntry {
  id: string;
  loader: string;
  mods: number;
  hours: number;
  current?: boolean;
}

export interface SaveEntry {
  name: string;
  desc: string;
  when: string;
  size: string;
}

export interface NewsEntry {
  title: string;
  desc: string;
  date: string;
}

export interface StatEntry {
  value: string;
  unit: string;
  label: string;
}

export interface RemoteVersion {
  id: string;
  date: string;
  loaders: string;
  installed?: boolean;
}

/** Mojang 版本清单里的一条 */
export interface VersionSummary {
  id: string;
  /** release | snapshot | old_beta | old_alpha */
  kind: string;
  released: string;
}

export interface LauncherConfig {
  gameDir: string;
  /** official | bmclapi */
  source: string;
  username: string;
}
