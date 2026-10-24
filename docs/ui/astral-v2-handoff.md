# Astral V2 收尾交接（2026-10-24）

继续当前用户任务，完成后才交付；用户反复强调 B 排布、三张背景全部保留并启动随机、三套动画和完整 V2，不要仅口头解释。

## 约束

- 仅 `D:\软件开发\astral-ui`、`feature/astral-v2-ui`；HEAD `6868a47`。先核对 top-level/branch/status。不匹配停止。
- 现有所有未提交 V2 文件是 Codex 本轮工作，保留并续做。禁止 reset/clean/擅自提交/推送/合并/改钩子。
- 不编辑 core/main 工作树、Rust、共享 bridge.ts/types.ts/契约、包清单和锁文件。缺失核心能力写 API needs，不能假成功。
- 已批准 B 排布：88px 图标栏、整幅森林、左大衬线标题、下启动坞、两世界卡+AI。A/C 只换背景和 Token，不再换排布。
- 不动用户正在运行的 `D:\Astral\Astral.exe`。
- 本轮无需再增功能范围或重写核心。真实资源安装/依赖求解、MSA、Forge/NeoForge/光影组件、任务取消/持久化、窗口就绪都缺 Rust，准确禁用并交 DSH。

## 已完成

- 六开源项目许可/README/代码/截图研究：docs/ui/astral-v2-research.md；API 提案 docs/ui/astral-v2-api-needs.md。
- 正式前端已切换 V2，共享源 app/src/v2；旧 App 导出 LegacyApp 保留。原 preview/forest-tale 未改。
- B 唯一排布，三森林×明暗 Token；默认每次打开随机且避免连续重复，设置可关/手选，壁纸/动画/入场/节能/系统减少动画/后台暂停。
- 正式首页真实 listInstances/Config 选择/startLaunch/Task/session/exit，由原 useLocalLauncher/useInstances 驱动。Task.done 不算游戏已进入，session 只表示进程存活，错误持续。
- 真实版本管理、差异 patch、删除确认/INSTANCE_BUSY、Mod CRUD、存档/目录、内存/JVM；浏览器没有假档案/实例/任务。
- 下载六类；原版清单实时/虚拟滚动；游戏安装 UI 链 startInstall→真实完成→create→完成→可选具体 Fabric build→完成→重读 health。尚无跨重启原子编排。
- 资源详情实时 Modrinth，目标→具体文件→依赖/世界；最终安装禁用，没有偷用 downloadTo。
- 三套轻量 CSS 启动效果，原型同事件逐步对比，正式真实状态驱动且设置可选；日志本地脱敏抽屉，不上传。
- pnpm tsc 零错误，vite build 成功，分包 JS 主133.55kB/react223.19/事实快照200.50，合计 gzip约133kB；CSS64.84/gzip13.46kB。最新小改（Legacy fallback、设置锚点）后 tsc 已通过，需要最终 build。
- 原型 verify.cjs **74 通过**、正式 React+隔离 IPC fixtures verify-production.cjs **19 通过**；0 JS 异常。最新小改后可再跑一次两脚本。
- 原生真实 Rust/Tauri 已编译并运行。verify-native.cjs **8 通过**，真实 IPC、真实空版本/账号/清单，1280×800 DPR1，不是假 IPC。无真实游戏启动/安装和系统125/150 DPI认证。所有原生数据隔离。

## 剩余收尾（请自主完成）

1. 核对正式源码明显问题；尤其 LiveResource project metadata effect 的 retry 是否在依赖中，确保网络失败重试实际触发。不扩大范围。
2. 旧 docs/ui/astral-v2-design-review.md 和 preview/astral-v2/README.md/ASSETS.md **内容过时**：仍写仅阶段1–3/正式app未改/5198不是V2/未正式接入动画。请重写为真实当前交付，保留研究细节并准确区分 UI可用与核心依赖。
3. 增加简洁最终交付/验收说明，可直接重写 design-review：文件范围、预览URL、测试counts、真实原生范围、未测项、DSH优先事项。不要写“整个产品全部完成”。所有用户大需求逐项对应已做/界面有但核心缺/待实际验收。
4. README 写两入口：正式5198、带明确样例标记的设计室5202，三动画#motion。preview/src 多数是 app/src/v2 的重导出，不再重复三套页面。ASSETS更新 app/public/forest 路径；背景三张hash未变，远程 Modrinth icon仅显示元数据，未入库复制品牌。
5. 最终 tsc/build、74原型+19正式回归；有更改再补适当原生检查。不要在 native 验证期间持续编辑前端导致 HMR 偶发空壳；重新加载可恢复。
6. 查看代表截图，确认真实启动准备 panel 没遮 hero，三主题排布一致。之前 .b-hero flex-shrink:0/min340 修正了启动面板膨胀覆盖文案。
7. git diff --check，确认 bridge/types/Rust/package/lock/原Forest三案均无diff；不提交推送。
8. 最终打开正式5198与动效室5202，交付简明完成清单+文档路径+截图+明确核心未完成能力。

## 运行与验证

已有正式 Vite **5198**（session49349，PID54924），原型 **5202**（session18008）。两个 vite.config watch已忽略 src-tauri/target，避免 Windows EBUSY。

从仓库根：
```
pnpm --config.verify-deps-before-run=false --dir app exec tsc --noEmit
pnpm --config.verify-deps-before-run=false --dir app exec vite build
node preview/astral-v2/verify.cjs
node preview/astral-v2/verify-production.cjs
node preview/astral-v2/verify-native.cjs
```
原型/正式截图和结果已忽略，不提交临时产物。截图在 preview/astral-v2/screenshots，包括 production（IPC桩标识）和 native（实际WebView）。native-test/native-results.json为真实原生结果。

原生 debug EXE 已运行（session36416），仅它启用 CDP **9224**；Playwright connectOverCDP 可操作这个 WebView，未注入 fixture。不要关稳定启动器。
- exe: app/src-tauri/target/debug/astral.exe
- APPDATA: 本仓 preview/astral-v2/native-test/AppData
- WEBVIEW2_USER_DATA_FOLDER: 同目录 WebView2
- WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: --remote-debugging-port=9224
- 游戏目录: 同目录 game。verify-native先断言隔离。
- 构建使用 VS2022 DevShell + CARGO_BUILD_JOBS=2 + CARGO_PROFILE_DEV_DEBUG=0（仅进程env）；之前并发C编译D8050，低并发无调试符号已成功，不改Rust。

测试默认借用现有 Playwright/Chromium（脚本可用env覆盖），没新增包依赖。格式化用 npx prettier@3.6.2，仅工具缓存，无项目依赖/锁修改。

交接启动后 Codex 停止编辑同批文件。完成后请写结果摘要到 docs/ui/astral-v2-workbuddy-result.md，含最终检查输出/截图路径/待办，不包含令牌和私人记录。
