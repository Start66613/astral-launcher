import type { NewsEntry, RemoteVersion, SaveEntry, StatEntry, VersionEntry } from "./types";

export const ACCOUNT = {
  name: "Start66613",
  type: "微软账号",
  initial: "S",
};

export const VERSIONS: VersionEntry[] = [
  { id: "1.21.1", loader: "NeoForge 21.1.251 · 46 个模组", mods: 46, hours: 19.8, current: true },
  { id: "1.20.1", loader: "Forge 47.2.0 · 128 个模组", mods: 128, hours: 126.5 },
  { id: "1.21.4", loader: "Fabric 0.16.5 · 12 个模组", mods: 12, hours: 58.2 },
  { id: "1.20.1 原版", loader: "未安装模组", mods: 0, hours: 2.1 },
];

export const SAVES: SaveEntry[] = [
  { name: "Expedition_07", desc: "生存 · 第 7 周目 · 12.4 MB", when: "今天 19:32", size: "12.4 MB" },
  { name: "Skyblock_02", desc: "空岛 · 3 人联机 · 8.1 MB", when: "昨天 22:10", size: "8.1 MB" },
  { name: "Build_Server", desc: "创造 · 建筑工坊 · 41.7 MB", when: "10-19", size: "41.7 MB" },
];

export const NEWS: NewsEntry[] = [
  { title: "下载模块改为可远程开关", desc: "遇到合规调整时无需更新客户端", date: "10-20" },
  { title: "新增 Fabric 0.16 支持", desc: "安装时自动匹配加载器版本", date: "10-16" },
];

export const STATS: StatEntry[] = [
  { value: "12.4", unit: "小时", label: "游玩时长" },
  { value: "37", unit: "次", label: "死亡" },
  { value: "9", unit: "个", label: "解锁成就" },
  { value: "0", unit: "次", label: "崩溃" },
];

export const LATEST: RemoteVersion[] = [
  { id: "26.3", date: "最新正式版 · 发布于 2026-09-15 19:23", loaders: "正式版" },
  { id: "26.4-snapshot-5", date: "最新预览版 · 发布于 2026-10-20 20:15", loaders: "预览版" },
];

export const RELEASES: RemoteVersion[] = [
  { id: "1.21.1", date: "2024-08-08 · 支持 NeoForge / Fabric", loaders: "NeoForge / Fabric" },
  { id: "1.20.1", date: "2023-06-12 · 支持 Forge / Fabric / NeoForge", loaders: "Forge / Fabric / NeoForge", installed: true },
  { id: "1.19.2", date: "2022-08-05 · 支持 Forge / Fabric", loaders: "Forge / Fabric" },
];

export const SNAPSHOTS: RemoteVersion[] = [
  { id: "26.4-snapshot-5", date: "2026-10-20 · 无加载器支持", loaders: "—" },
];

export const OLD: RemoteVersion[] = [
  { id: "Beta 1.7.3", date: "2011-07-08 · 经典版本", loaders: "—" },
];

export const HELP_SECTIONS = [
  {
    title: "指南",
    items: [
      { title: "Minecraft 教程", desc: "由社区编写的入门教程" },
      { title: "整合包制作指南", desc: "介绍对整合包制作可能用到的功能" },
      { title: "资源安装指南", desc: "介绍如何安装下载到的 Mod、存档、材质包、光影" },
    ],
  },
  {
    title: "启动器",
    items: [
      { title: "版本隔离是什么", desc: "每个版本使用独立的 mods 与配置目录" },
      { title: "内存怎么分配", desc: "自动配置与手动配置的取舍" },
      { title: "崩溃了怎么办", desc: "先看 AI 诊断，再看 latest.log" },
    ],
  },
];
