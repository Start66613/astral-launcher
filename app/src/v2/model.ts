import catalog from "./data/version-catalog.json";
import resourceCatalog from "./data/resource-catalog.json";
import loaderCatalog from "./data/loader-catalog.json";
import { mergeTask } from "../lib/bridge";
import type { Task, GameSession } from "../lib/types";

export { catalog, resourceCatalog, loaderCatalog };
export const THEMES = [
  {
    id: "daylight",
    name: "森林晨光",
    subtitle: "鼠尾草绿 · 清晨薄雾",
    image: "/forest/daylight.webp",
  },
  {
    id: "nightfall",
    name: "深林秘境",
    subtitle: "深松绿 · 月色萤火",
    image: "/forest/nightfall.webp",
  },
  {
    id: "fieldnotes",
    name: "林野手札",
    subtitle: "暖纸白 · 橄榄与赭色",
    image: "/forest/fieldnotes.webp",
  },
] as const;
export type ThemeId = (typeof THEMES)[number]["id"];
export type Category =
  | "game"
  | "mod"
  | "modpack"
  | "datapack"
  | "resourcepack"
  | "shaderpack";
export const CATEGORIES: { id: Category; name: string; detail: string }[] = [
  { id: "game", name: "原版游戏", detail: "从一个新的世界开始" },
  { id: "mod", name: "Mod", detail: "更多玩法，更多可能" },
  { id: "modpack", name: "整合包", detail: "准备好的一整段冒险" },
  { id: "datapack", name: "数据包", detail: "为你的存档添加新规则" },
  { id: "resourcepack", name: "资源包", detail: "换一种质感看世界" },
  { id: "shaderpack", name: "光影包", detail: "让光线穿过树梢" },
];
export type PreviewVersion = {
  id: string;
  name: string;
  gameVersion: string;
  loader: string;
  loaderVersion: string;
  shared: boolean;
  mods: number;
  lastUsed: string;
};
export const SAMPLE_VERSIONS: PreviewVersion[] = [
  {
    id: "preview_pine",
    name: "林间小径",
    gameVersion: "1.21.1",
    loader: "fabric",
    loaderVersion: "0.20.1",
    shared: false,
    mods: 18,
    lastUsed: "最近使用 · 示例",
  },
  {
    id: "preview_vanilla",
    name: "原版生存",
    gameVersion: "26.3",
    loader: "game",
    loaderVersion: "",
    shared: false,
    mods: 0,
    lastUsed: "最近使用 · 示例",
  },
  {
    id: "preview_old",
    name: "旧日时光",
    gameVersion: "1.8.9",
    loader: "game",
    loaderVersion: "",
    shared: true,
    mods: 0,
    lastUsed: "共享游戏目录 · 示例",
  },
  {
    id: "preview_neo",
    name: "机械与森林",
    gameVersion: "1.21.1",
    loader: "neoforge",
    loaderVersion: "21.1.0",
    shared: false,
    mods: 24,
    lastUsed: "兼容性展示 · 示例",
  },
];
export const date = (input: string) => input?.slice(0, 10) || "未知";
export const size = (bytes: number) =>
  bytes >= 1024 * 1024
    ? `${(bytes / 1024 / 1024).toFixed(1)} MB`
    : `${Math.round(bytes / 1024)} KB`;
export const kindIcon = (kind: string) =>
  kind === "release"
    ? "game"
    : kind === "snapshot"
      ? "snapshot"
      : kind.startsWith("old_")
        ? "history"
        : "special";
export type Resource = (typeof resourceCatalog.projects)[number];
export type ResourceFileVersion = Resource["versions"][number];
export function compatibleFiles(project: Resource, target?: PreviewVersion) {
  if (!target || target.shared) return [];
  return project.versions.filter(
    (version) =>
      version.gameVersions.includes(target.gameVersion) &&
      ((project.kind !== "mod" &&
        project.kind !== "support" &&
        project.kind !== "component") ||
        version.loaders.includes(target.loader)),
  );
}
export const LAUNCH_STEPS = [
  {
    id: "idle",
    name: "等待启动",
    detail: "手动推进同一组事件，比较三种视觉。",
    stage: "queued",
  },
  {
    id: "resolve",
    name: "检查版本",
    detail: "解析版本 1.21.1",
    stage: "resolve",
  },
  { id: "jvm", name: "准备 Java", detail: "查找 Java 21", stage: "jvm" },
  {
    id: "assets",
    name: "检查游戏依赖",
    detail: "校验 1264 个资源文件",
    stage: "assets",
  },
  {
    id: "download",
    name: "补全依赖文件",
    detail: "下载 21/61 · 当前阶段",
    stage: "download",
  },
  {
    id: "process",
    name: "进程已创建",
    detail: "等待查询游戏会话，尚未确认游戏窗口。",
    stage: "done",
  },
  {
    id: "running",
    name: "游戏进程运行中",
    detail: "会话已确认，当前接口不检测游戏窗口或主菜单。",
    stage: "done",
  },
  {
    id: "exited",
    name: "游戏进程已退出",
    detail: "匹配的退出事件 · 退出码 0",
    stage: "done",
  },
] as const;
export function launchState(index: number, failure = false) {
  const step = failure
    ? {
        id: "error",
        name: "启动未完成",
        detail: "NETWORK · 示例：依赖文件下载失败，请检查网络后重试。",
        stage: "download",
      }
    : LAUNCH_STEPS[index];
  let tasks = new Map<string, Task>();
  const finalRevision = failure
    ? Math.max(1, Math.min(index, 4))
    : Math.min(index, 5);
  for (let revision = 1; revision <= finalRevision; revision++) {
    const item = LAUNCH_STEPS[revision];
    tasks = mergeTask(tasks, {
      taskId: failure ? "preview_failure_01" : "preview_launch_01",
      revision,
      kind: "launch",
      instanceId: "preview_pine",
      gameVersion: "1.21.1",
      title: "启动 林间小径",
      stage: item.stage,
      label: item.detail,
      done: revision === 4 ? 21 : 0,
      total: revision === 4 ? 61 : 0,
      bytes: revision === 4 ? 36000000 : 0,
      totalBytes: revision === 4 ? 80000000 : 0,
      state: "running",
      error: null,
      result:
        revision === 5 && !failure
          ? {
              kind: "launch",
              sessionId: "preview_session_01",
              pid: 4242,
              startedAt: "2026-10-24T00:00:00Z",
            }
          : null,
      startedAt: "2026-10-24T00:00:00Z",
      endedAt: null,
      retryOf: null,
    });
  }
  const previous = [...tasks.values()].at(-1);
  if (previous && (failure || index >= 5))
    tasks = mergeTask(tasks, {
      ...previous,
      revision: finalRevision + 1,
      state: failure ? "error" : "done",
      endedAt: "2026-10-24T00:00:01Z",
      error: failure
        ? {
            code: "NETWORK",
            message: step.detail,
            detail: "仅为设计验收事件，不含用户日志。",
          }
        : null,
    });
  const task = [...tasks.values()].at(-1) ?? null;
  const session: GameSession | null =
    !failure && index >= 6
      ? {
          sessionId: "preview_session_01",
          instanceId: "preview_pine",
          taskId: "preview_launch_01",
          pid: 4242,
          startedAt: "2026-10-24T00:00:00Z",
          state: index === 7 ? "exited" : "running",
          exitCode: index === 7 ? 0 : null,
          endedAt: index === 7 ? "2026-10-24T00:02:00Z" : null,
          durationMs: index === 7 ? 120000 : null,
        }
      : null;
  return { ...step, task, session, active: !failure && index > 0 && index < 6 };
}
export const RESEARCH = [
  {
    name: "PCL CE",
    license: "主程序：自定义许可；其余：Apache-2.0",
    url: "https://github.com/PCL-Community/PCL-CE",
    file: "https://github.com/PCL-Community/PCL-CE/blob/dev/Plain%20Craft%20Launcher%202/Pages/PageDownload/PageDownloadInstall.xaml",
    learn: "最新版本入口、分组折叠、先选择版本再安装组件。",
    adopt:
      "用三组清晰的组件选择替代拥挤选项；保留推荐入口，也允许选其他真实构建。",
  },
  {
    name: "XMCL",
    license: "MIT",
    url: "https://github.com/Voxelum/x-minecraft-launcher",
    file: "https://github.com/Voxelum/x-minecraft-launcher/blob/master/xmcl-keystone-ui/src/components/StoreProjectInstallVersionDialog.vue",
    learn: "资源详情、兼容版本筛选、依赖和虚拟滚动。",
    adopt: "先选目标游戏，再选发行文件；把不兼容原因写在目标版本旁。",
  },
  {
    name: "HMCL",
    license: "GPL-3.0，含附加声明要求",
    url: "https://github.com/HMCL-dev/HMCL",
    file: "https://github.com/HMCL-dev/HMCL/blob/main/HMCL/src/main/java/org/jackhuang/hmcl/ui/download/AbstractInstallersPage.java",
    learn: "安装向导、互斥组件、Java 自动选择与单版本设置。",
    adopt: "主加载器互斥；不把光影支持组件与光影包混为一类。",
  },
  {
    name: "Prism Launcher",
    license: "GPL-3.0-only；品牌素材另有许可",
    url: "https://github.com/PrismLauncher/PrismLauncher",
    file: "https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/pages/instance/VersionPage.ui",
    learn: "以版本为边界管理组件、Mod、存档、日志。",
    adopt: "版本管理做二级页面；打开目录和数据保护规则随操作展示。",
  },
  {
    name: "Refract",
    license: "GPL-3.0",
    url: "https://github.com/RefractMC/Refract_MC",
    file: "https://github.com/RefractMC/Refract_MC/blob/main/apps/renderer/src/renderer/src/components/instances/EditInstanceDialog.tsx",
    learn: "React/Tauri 壳层、主题状态、统一编辑与复用组件。",
    adopt: "保留 Astral 技术栈，主题数据驱动；不移植其 React 组件或品牌资产。",
  },
  {
    name: "Helios",
    license: "MIT",
    url: "https://github.com/dscalzi/HeliosLauncher",
    file: "https://github.com/dscalzi/HeliosLauncher/blob/master/app/assets/js/scripts/landing.js",
    learn: "账号紧靠启动区、Java 准备和启动反馈连贯呈现。",
    adopt: "账号与启动按钮组成同一操作区，错误直接保留在准备面板。",
  },
];
