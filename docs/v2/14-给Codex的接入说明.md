# 给 Codex 的接入说明（V2 本轮）

> 分支 `feature/astral-v2-core`，HEAD 见仓库。命 **55/55 全部注册**，`--selftest` 94 项、
> 单元测试 49 项全过。本文只写**这轮新增/变化**的接口，旧的见 `docs/api-contract-bridge-v2.md`。

---

## 1. 新增命令

### `list_component_versions` — 光影组件版本列表

```ts
list_component_versions({ id, gameVersion, loader? }): ComponentVersion[]

interface ComponentVersion {
  versionId: string;      // 安装时必须用这个
  versionNumber: string;  // 展示用，例如 "1.7.6+1.20.1"
  channel: "release" | "beta" | "alpha";
  gameVersions: string[];
  loaders: string[];
  fileName: string;       // 实际文件名
  size: number;
  publishedAt: string;
  recommended: boolean;   // 第一个为 true
}
```

- 已验证：`iris + 1.20.1 + fabric` → 13 个版本，推荐 `1.7.6+1.20.1`。
- 错误：`UNSUPPORTED`（OptiFine 无公开 API）、`INCOMPATIBLE`（Oculus 需要 Forge）。

**三个字段不要混**：`versionId`（`1CMVXDHo`）≠ `versionNumber`（`1.7.2+1.20.1`）
≠ `fileName`（`iris-1.7.2+mc1.20.1.jar`）。本周曾因混用导致 400。

## 2. 变更的命令参数

### `prepare_game_install` 的 request 新增字段

```ts
interface GameInstallRequest {
  name: string;
  gameVersion: string;
  loader?: { kind: string; version: string } | null;
  memoryMb?: number;
  shaderSupport?: "iris" | "oculus" | "optifine" | "none";
  shaderSupportVersion?: string;   // 新增：上面列表里的 versionId
}
```

计划里 `components: PlanComponent[]` 会给出将要安装的组件（含 `versionId` / `fileName`）。
**不传 version 时用推荐版本**（旧行为不变）。

### `download_to` 参数扩展（已标记弃用）

```ts
download_to({ url, kind, filename, instanceId?, sha1?, sha512?, overwrite? })
```

- `kind = "mod" | "datapack"` 时 **`instanceId` 必填**，否则 `INVALID_VALUE`；
- 目标已存在且没传 `overwrite: true` → `FILE_EXISTS`；
- 传了 `sha1`/`sha512` 会在落盘前校验。
- **新 UI 请一律用 `prepare_resource_install` + `start_resource_install`**，
  `download_to` 只留给资源包/光影包这类全局目录。

## 3. 行为变化（会影响 UI 提示）

| 场景 | 以前 | 现在 |
|---|---|---|
| 原版实例装 Fabric Mod | 允许 | `INCOMPATIBLE`（阻塞） |
| 资源安装任务 | 无实例归属 | 绑定 `instanceId`，实例被占用时删除/改名会 `INSTANCE_BUSY` |
| 资源安装到兼容实例（managed=false） | 允许 | `UNSUPPORTED`（共享目录不写） |
| 计划重复执行 | — | `PLAN_EXPIRED`（计划一次性） |
| 取消后已完成 | 显示完成 | `done` + `cancelledLate: true` |
| 同名文件覆盖 | 静默覆盖 | `FILE_EXISTS`，需显式确认 |
| 依赖还有依赖 | 静默只装一层 | 阻塞并列出缺失项目，要求人工处理 |

## 4. 任务事件

`astral://task` 的 `Task` 上已有：`phase`、`stageLabel`、`cancelling`、`retrySpec`。
取消流程请按：

1. 调 `cancel_task` → `cancelling: true`（此时**不要**显示"已取消"）；
2. 等 `state` 变 `cancelled`（真停下）或 `done` + `cancelledLate`（已干完）。

## 5. 资源安装 UI 建议流程

```ts
const plan = await prepareResourceInstall({
  instanceId, kind, projectId, versionId, worldId, overwrite: false,
});
// 展示 plan.issues（blocking 时禁止开始）、plan.dependencies、plan.downloadBytes
const task = await startResourceInstall(plan.planId, plan.revision);
```

- `datapack` 必须带 `worldId`（来自 `listTargetWorlds`）；
- 资源包 / 光影包**不要**传 `gameVersion` 过滤（会过滤空）。

## 6. 尚未提供的能力（不要给入口）

- 依赖递归安装（只装一层，遇到嵌套依赖会阻塞并要求人工处理）
- 大文件下载中途即时取消（只在文件边界停下）
- MSA 登录、AI 诊断

## 7. 错误码补充

`INVALID_PATH`（文件名/路径不安全）、`FILE_EXISTS`（未授权覆盖）、
`DEPENDENCY_CONFLICT`（依赖冲突，含嵌套依赖）、`PLAN_EXPIRED`（计划一次性/过期）、
`INVALID_VALUE`（参数不合法）、`INCOMPATIBLE`（版本/加载器不搭）。
