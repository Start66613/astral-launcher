# Forest Tale · v3.2 共享层与实例界面接入

日期：2026-10-23。工作区 `D:\软件开发\astral-ui`；分支 `feature/forest-ui`。开始时目录 / 分支匹配，工作区干净。核心只读基点为远端 `be1d70d9e4590214f24ce85ea543324dab234b0a`，通过临时代理 fetch 后读取契约与桥接代码块。

## 本轮实现

1. `app/src/lib/types.ts`：按桥接文档 §1 / §7 追加真实 DTO 与模组类型，原设计示例 SaveEntry、LauncherConfig 保持。
2. `app/src/lib/bridge.ts`：按 §2 / §7 追加命令、事件、错误和文档原版 mergeTask，类型 import 位于顶部。原 scanVersions / launchVersion / installVersion / downloadTo / getConfig / setConfig / onProgress 的签名与实现未改。
3. 按本轮用户指令调整桥接示例的浏览器分支：新查询、写操作与监听均抛 ApiError(UNSUPPORTED)，不返回默认配置、示例任务或实例。页面在普通浏览器中不调用这些接口，显示“浏览器预览 · 未连接”。
4. `useLocalLauncher.ts` 先探测 getApiInfo，再启用实例 hook 或兼容 hook。版本号不足 / 缺 instances / 旧核心未注册 get_api_info 时回退版本模式；普通读取故障保留可重试错误，不假装旧核心。缺 tasks / sessions 时不调用相应命令、不开放创建或启动操作。
5. `useInstances.ts` 在 App 生命周期内管理任务、实例、Config、会话和监听；`useLegacyLauncher.ts` 保留旧核心兼容行为，只有被选用时才扫描旧版本。
6. `Launch.tsx` 用真实 Instance.id 渲染列表并持久化选择；启动采用 startLaunch，使用真实任务 stage / label；不持久化 PID、不用 PID 推断运行状态。文件不完整时显示原因并引导下载中心。
7. `Versions.tsx` 读取真实 name / gameVersion / loader / health / modsCount / modsDisabled；名称编辑仅提交 name，默认删除移入应用回收站并要求确认与勾选。新建使用 startCreateInstance({ name, gameVersion }) 和 Task 事件，完成后重拉实例；新建元数据不等于装好游戏。
8. `InstanceHealth.tsx` 共用健康展示；只说明“基础文件检查通过”，失败显示 reasons[0] 和可展开完整原因，绝不在前端改变 health。兼容实例标注“兼容版本 · 共享游戏目录”，没有编辑、删除、实例模组写入入口。
9. `App.tsx` 接入统一状态栏与能力探测后的系统查询；`forest.css` 增加会话、健康、实例表单与任务状态样式，保留 B「萤火夜航」构图、深浅主题和响应式布局。

## 启动三态与恢复依据

- **启动中**：当前实例最新 launch Task.state 为 queued / running，显示核心 stage + label。RPC 还未提供任何新任务时只显示“正在提交启动请求”。
- **运行中**：启动任务完成只代表进程已创建；必须再次查询 getActiveSession(instanceId)，得到匹配 instanceId / sessionId 且 state=running 的会话。刷新时允许直接恢复核心确认的活动会话，即使 Task 已超出保留范围。
- **已退出**：匹配 instanceId + sessionId 的 game-exit，或查询返回真实 exited 会话。null 再查 listSessions({ instanceId, limit: 1 })；没有真实结束记录就不显示退出。exitCode=null 显示“已结束，退出码未知”。
- 初次加载先完成 onTask / onGameExit / onInstanceChanged，再 getTasks → getActiveInstance → 实例与配置 → 当前实例 getActiveSession。任务返回、事件和快照统一调用桥接文档原版 mergeTask：revision 取大，终态不能退回非终态。
- 对会话按实例和请求代次隔离；退出可早于 Task 返回，先按 sessionId 缓存，待对应结果出现再关联。退出终态不会被迟到的 running 查询复活；旧实例 / 旧会话事件不清掉新会话。
- onInstanceChanged 主动重拉，不仅依赖写操作返回；首次快照期间的变更会补拉。窗口聚焦也会补查。监听失败释放已建立监听，重试时重新建立；卸载时释放，迟到的监听创建结果也清理。
- 没有确认的会话、查询失败、未选实例分别展示未知 / 错误 / 未启动状态。切换只采纳 setActiveInstance 返回的 Config，不乐观指定当前实例，不默认选择第一项。

## 暂时保留的 legacy 路径

- 下载中心 `Download.tsx`：listVersions / scanVersions / installVersion / onProgress，继续阻塞到安装完成；本轮按要求未任务化。
- 社区资源 / 模组浏览 `Resources.tsx`：Modrinth 搜索与 legacy downloadTo。新模组桥接已导出，但本轮未做本地模组管理或实例定向安装；现有下载仅代表共享目录文件落地，不代表安装到独立实例或整合包解包成功。
- 设置 `Settings.tsx`：getConfig / setConfig / openPath；保留现有原生配置与外观交互，不在本轮扩展 JVM / Java 配置。
- 旧核心首页 / 版本管理：能力不足时使用 useLegacyLauncher，启动只说明真实 PID 已创建，不显示持续运行。浏览器没有示例实例或假登录。
- AI 抽屉继续用户手选日志的本机预览；远程分析和自动修复未实现。存档 / Stats 桥接虽已追加，本轮没有新增统计界面，因此不渲染 deaths / achievements 等未知指标。

## 验证与截图

共享层追加后立即执行类型检查，零错误；两次代码提交前均执行 TypeScript 与 Vite 构建。

本机 pnpm 默认的运行前自动安装检查会触发 esbuild 许可错误，并自动插入无效占位项。本轮仅撤回工具自行插入的两行，原 app/pnpm-workspace.yaml 与锁文件保持不变；通过本次命令参数禁止自动安装预检，使用已有依赖：

```powershell
Set-Location -LiteralPath 'D:\软件开发\astral-ui\app'
pnpm --config.verify-deps-before-run=false exec tsc --noEmit
pnpm --config.verify-deps-before-run=false exec vite build
```

浏览器验证使用本机 Playwright / Chromium 149，脚本支持 ASTRAL_PLAYWRIGHT_MODULE、ASTRAL_BROWSER_EXECUTABLE、ASTRAL_REACT_URL，不修改应用依赖：

```powershell
Set-Location -LiteralPath 'D:\软件开发\astral-ui'
node preview/forest-tale/react/verify.cjs
node preview/forest-tale/react/verify-core.cjs
```

- 原有界面回归：75 项通过，0 失败，覆盖六页深浅主题 / 五种宽度、浏览器诚实状态、日志隐私、模态与 legacy 操作。
- v3.2 接入验证：32 项通过，0 失败；运行真实 bridge.ts 与 mergeTask，在 Tauri IPC 边界提供隔离测试响应 / 事件。覆盖订阅顺序、参数、能力降级、相同游戏版本的不同实例、Config 选中权威、错误、乱序 / 快速退出、刷新恢复、编辑 / 删除 / 创建及深浅主题 / 窄屏。
- 所有六张状态截图标注“接口桩验收 · 未启动真实游戏 / 未操作真实文件”，不属于真实原生运行证据。已目视检查状态文案、布局、原因展开与兼容卡片。

本机截图（相对仓库根目录）：

- [启动中](../../preview/forest-tale/react/screenshots/core-v3.2/starting.png)
- [运行中](../../preview/forest-tale/react/screenshots/core-v3.2/running.png)
- [已退出](../../preview/forest-tale/react/screenshots/core-v3.2/exited.png)
- [空实例](../../preview/forest-tale/react/screenshots/core-v3.2/empty-instances.png)
- [兼容实例](../../preview/forest-tale/react/screenshots/core-v3.2/compatibility-instance.png)
- [文件不完整](../../preview/forest-tale/react/screenshots/core-v3.2/incomplete-files.png)

结果保存在原回归的 react/test-results.json 和新测试的 react/screenshots/core-v3.2/test-results.json。截图与结果被现有忽略规则排除，不提交 Git；验证脚本可重新生成。

## 原生联调边界

核心分支只做了 fetch / git show 读取；没有修改、合并或切换核心分支，没有编辑 Rust、Tauri、包清单、依赖和锁文件。本轮只在 UI 分支应用用户授权的共享层代码块及前端实现。

UI 工作树中的 Rust 仍为原基点。直接运行本分支现有 Tauri 时会检测到旧核心并走兼容路径；完整 v3.2 原生能力需要后续经授权的集成构建。测试没有启动 Minecraft、真实新建 / 删除实例、改用户配置或写游戏文件，不能替代与 v3.2 核心的原生端到端验收。
