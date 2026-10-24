# Astral V2：开源项目设计研究与现状差距

研究日期：2026-10-24。Astral 基线：`6868a47f0c86bf426a58dd4ae4ace59f1339a896`（最新 origin/main）。本轮只选读相关 README、许可证、界面源码与功能实现，不宣称完整审计六个仓库。未导入其代码、图标、截图、品牌或背景。下文源码链接用于核对交互依据，截图仅用于观察；历史截图不代表当前版本具备全部能力。

## 1. 先核对许可

- **PCL CE**：[根 LICENSE](https://github.com/PCL-Community/PCL-CE/blob/dev/LICENSE) 为 Apache-2.0，但 [主程序目录 LICENCE](https://github.com/PCL-Community/PCL-CE/blob/dev/Plain%20Craft%20Launcher%202/LICENCE) 单独规定《PCL 分发有限许可》，另附合理使用指南。不能据根许可证将主程序视为可自由移植的 Apache 源码。本轮只参考通用任务组织，不复用其控件实现、卡片造型、图片、专有文案或品牌。
- **XMCL**：[MIT](https://github.com/Voxelum/x-minecraft-launcher/blob/master/LICENSE)。本轮也不复制其代码或素材，避免引入不必要的第三方组件来源。
- **HMCL**：[GPL-3.0](https://github.com/HMCL-dev/HMCL/blob/main/LICENSE)；[README](https://github.com/HMCL-dev/HMCL/blob/main/docs/README_zh_Hans.md) 另说明版权声明要求，选读代码头包含 GPL-3.0-or-later。若将来拟移植代码，应逐文件核对并评估相应义务；当前没有移植。
- **Prism**：[COPYING.md](https://github.com/PrismLauncher/PrismLauncher/blob/develop/COPYING.md) 与 [README](https://github.com/PrismLauncher/PrismLauncher) 区分 GPL-3.0-only 代码、CC BY-SA 4.0 标志及其他第三方材料。Astral 不使用 Prism 标志或图片。
- **Refract**：[GPL-3.0 LICENSE](https://github.com/RefractMC/Refract_MC/blob/main/LICENSE)。只研究壳层分工、状态组织和玩家操作路径，不迁入 React 组件。
- **Helios**：[MIT LICENSE.txt](https://github.com/dscalzi/HeliosLauncher/blob/master/LICENSE.txt)。参考启动过程与账号操作邻接关系，未复用模板、品牌、图片或代码。

这些是本轮复用边界记录，不代表对未来派生或分发方式作法律结论。

## 2. 六个项目的具体观察与 Astral 决策

### PCL CE：把版本选择与安装配置分开

选读 [原版版本列表逻辑](https://github.com/PCL-Community/PCL-CE/blob/dev/Plain%20Craft%20Launcher%202/Pages/PageDownload/PageDownloadClient.xaml.cs)、[组件安装页](https://github.com/PCL-Community/PCL-CE/blob/dev/Plain%20Craft%20Launcher%202/Pages/PageDownload/PageDownloadInstall.xaml)、[启动左侧](https://github.com/PCL-Community/PCL-CE/blob/dev/Plain%20Craft%20Launcher%202/Pages/PageLaunch/PageLaunchLeft.xaml)。列表按类型组织，最新入口与长列表分开；组件组可收起；账号、启动与启动准备区关系紧密。

视觉参考：[项目公开的收藏页截图](https://github.com/user-attachments/assets/34686b6b-e8a2-4220-bcb1-95064c51f361)。实际观察到分类侧栏、可折叠资源组和列表操作；它不是安装页截图，安装流程判断来自上述 XAML 与逻辑文件。

**采用**：最新正式/快照入口、分组计数与折叠、点击版本进入安装配置、保留其他具体构建选择。**改进**：Astral 以“主加载器／光影支持／依赖推荐”三组表达关系，避免初学者把 Iris 与光影包混淆；不照搬 PCL 外观。所研究版本可能移除旧组件支持，不能据历史截图认定当前支持 Quilt。

### XMCL：明确目标，再选择发行文件

选读 [发行版本选择对话框](https://github.com/Voxelum/x-minecraft-launcher/blob/master/xmcl-keystone-ui/src/components/StoreProjectInstallVersionDialog.vue)、[Modrinth 详情](https://github.com/Voxelum/x-minecraft-launcher/blob/master/xmcl-keystone-ui/src/components/MarketProjectDetailModrinth.vue) 和 [README](https://github.com/Voxelum/x-minecraft-launcher)。其技术栈与 Astral 不同，不能当作 Tauri/React 示例。发行选择提供游戏版本、渠道筛选与明确版本确认，长列表使用虚拟化。

视觉参考：[官方站点仓库首页截图](https://raw.githubusercontent.com/Voxelum/xmcl-page/master/.vitepress/theme/assets/home.png)。观察到当前游戏上下文、启动入口与 Mod／资源包／存档管理的关联。

**采用**：资源详情保留具体版本列表、加载器和游戏版本约束。**改进**：Astral 固定先选“装到哪个游戏”，再给文件列表；不兼容目标附原因；全部资源使用同一套确认底栏。

### HMCL：安装向导与单版本运行设置

选读 [安装向导基础页](https://github.com/HMCL-dev/HMCL/blob/main/HMCL/src/main/java/org/jackhuang/hmcl/ui/download/AbstractInstallersPage.java)、[附加安装器](https://github.com/HMCL-dev/HMCL/blob/main/HMCL/src/main/java/org/jackhuang/hmcl/ui/download/AdditionalInstallersPage.java)、[游戏设置](https://github.com/HMCL-dev/HMCL/blob/main/HMCL/src/main/java/org/jackhuang/hmcl/ui/game/GameSettingsPage.java)。观察向导控制、安装名称检查、组件约束与 Java 自动／指定设置组织。

视觉参考：[官方自动安装文档](https://docs.hmcl.net/launcher/auto-installing.html) 的 [组件选择截图](https://docs.hmcl.net/assets/img/docs/auto-installing/AutoInstaller_ModLoader.png)。组件图标和选中版本便于扫描，但多种组件并列需要玩家理解依赖。

**采用**：先配置再执行、显示具体构建、按版本设置 Java／内存／JVM。**改进**：主加载器互斥，光影支持独立；依赖标注必需／可选，未知稳定性不自造“稳定”徽章。

### Prism：以游戏版本为数据边界

选读 [版本组件编辑](https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/pages/instance/VersionPage.ui)、[Mod 文件夹管理](https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/pages/instance/ModFolderPage.cpp)、[官方 Mod 管理说明](https://www.prismlauncher.org/wiki/help-pages/loader-mods/)。组件、Mod、存档、日志均位于同一游戏配置上下文；Mod 更新策略区分正式／Beta／Alpha。

视觉参考：[官方 Mod 列表截图](https://www.prismlauncher.org/img/screenshots/LoaderModsDark.png)。表格信息密度高，启停、移除、目录操作集中，适合长期管理。

**采用**：版本管理做启动页的二级入口，按“总览／组件／Mod／存档资源／运行／日志”组织。**改进**：以“独立游戏目录”向玩家解释隔离；共享目录给清楚的风险说明。复制配置不默认复制存档，核心未支持时禁用。

### Refract：React 壳层与资源卡片复用

选读 [AppShell](https://github.com/RefractMC/Refract_MC/blob/main/apps/renderer/src/renderer/src/components/layout/AppShell.tsx)、[Sidebar](https://github.com/RefractMC/Refract_MC/blob/main/apps/renderer/src/renderer/src/components/layout/Sidebar.tsx)、[版本编辑](https://github.com/RefractMC/Refract_MC/blob/main/apps/renderer/src/renderer/src/components/instances/EditInstanceDialog.tsx) 和 [README](https://github.com/RefractMC/Refract_MC)。观察 Tauri／renderer 分工、主题状态、异步选择和版本编辑复用。

视觉参考：[仓库 screenshot.png](https://raw.githubusercontent.com/RefractMC/Refract_MC/main/logo/screenshot.png)。观察到分类筛选、渠道入口与统一资源卡片；高密度卡片需要限制长简介高度。

**采用**：布局壳层统一、主题状态集中、资源卡片复用。**改进**：Astral 只保留四项主导航；Mod 纳入下载；卡片不提供跳过目标与兼容性检查的一键真安装。

### Helios：启动按钮应连续表达准备过程

选读 [landing.ejs](https://github.com/dscalzi/HeliosLauncher/blob/master/app/landing.ejs)、[landing.js](https://github.com/dscalzi/HeliosLauncher/blob/master/app/assets/js/scripts/landing.js)、[README 与截图](https://github.com/dscalzi/HeliosLauncher#readme)。Java 查找与校验、依赖准备、启动反馈位于连贯流程。README 截图经浏览器查看；原始 Imgur 链接连接不稳定，改在 GitHub README 中检查图像，未将其用作 Astral 素材。

**采用**：账号靠近启动按钮，准备状态在同一区域展示。**改进**：不用固定时间或预置百分比推动完成；Task.done 只表示进程创建，必须查 GameSession。当前核心不能确认游戏窗口／主菜单，不展示“已进入世界”。

## 3. Astral 现状：保留已完成能力，修正缺口

本地实际阅读：`docs/api-contract.md`（v3.3）、`app/src/lib/{bridge,types}.ts`、`app/src/hooks/useInstances.ts`、`app/src-tauri/src/{task,session,loader,core,lib}.rs`，以及现有页面、CSS Token、三案原型和素材记录。

### 已有且应复用

- Tauri 2 / React 19 / TypeScript / Rust / portablemc 技术栈不变。
- 当前首页和版本管理已有真实实例查询、当前版本、启动任务、会话与退出恢复逻辑；不是从零重写的对象。
- `mergeTask` 按 revision 合并并保持终态粘滞；`useInstances` 已有订阅与恢复顺序，应复用和回归，不能另做计时器启动状态机。
- 版本 CRUD、独立 `mods/config/saves`、健康原因、Mod 查询／启停／删除，以及 Fabric 构建目录已存在。
- 三套原创 WebP 共约 1.19 MB，配色各有特征，全部保留；仅统一布局，不建立三套页面。

### 设计与功能差距

1. **结构**：版本管理和 Mod 的一级入口增加导航负担。V2 主导航改为启动／下载／设置／更多，管理随当前游戏上下文进入。
2. **安装路径**：现有资源页面直接挑第一个发行文件并通过 legacy `downloadTo` 下载，缺少明确目标、文件选择、依赖和世界选择。其浏览器 fallback 还会回传假完成；V2 原型完全不调用它。正式阶段应停用这种路径，保留 legacy 签名兼容其他旧页面。
3. **原版安装**：已有 `start_install`、创建配置和安装加载器命令，但缺少“验证计划→下载组件→创建可用配置”的安全编排。不能在前端把多个局部成功拼成整体安装成功。
4. **加载器差异**：契约 §11.9 文字提 Fabric/Quilt，实际 `loader.rs::list` 仅查询 Fabric 并返回 `kind=fabric`。Quilt 的查询／构建映射需 DSH 对齐；Forge/NeoForge 未实现。当前 LoaderCatalog 仅字符串版本，稳定性／依赖／冲突信息不足。
5. **资源能力**：Mod CRUD 不等于完整安装。实例级资源下载、安全路径、依赖解析、哈希、原子写入、数据包世界选择和整合包编排尚缺。
6. **状态真实性**：任务 ID 当前进程内递增，任务集合未持久化；跨重启不能假设可恢复。`cancel_task` 没保证 worker 停止写入，暂不开放取消。启动某些 progress.stage 为 done 时任务本身仍未结束，必须以 Task.state 判断。
7. **会话**：`get_active_session` 校验进程生存，不检测窗口／游戏主菜单。Task.done→查询会话→显示“游戏进程运行中”；退出须匹配 instanceId+sessionId；没有会话不得亮运行态。
8. **设置与账号**：memoryMb/jvmArgs 已消费；javaPath 虽存储但启动未消费。账号正版授权、全局下载并发、启动后窗口策略、日志／诊断仍需核心或宿主实现，不能从普通用户名推定正版状态。

## 4. 本轮落地与明确边界

`preview/astral-v2/` 是独立 React 高保真评审程序：四导航、三主题、Minecraft 安装配置、资源详情安装、数据包存档选择、整合包独立向导、版本管理、账号状态、设置及三种启动动效。版本目录和资源元数据为实际读取的公开快照；玩家档案、存档、任务和会话为显著标记的设计样例。

未修改正式 `app/src`、共享 `bridge.ts/types.ts` 或 Rust；本轮不进行真实登录、游戏启动、安装和文件管理。设计确认前不进入阶段 4。接口建议见 [astral-v2-api-needs.md](./astral-v2-api-needs.md)，阶段交付与测试见 [astral-v2-design-review.md](./astral-v2-design-review.md)。

## 5. 研究版本锁定

研究时取得的分支 HEAD，便于后续复查；链接中的分支可能继续移动：

- PCL CE `dev`：`915ac29795efbdf6373417389e181e1504117048`
- XMCL `master`：`b64e90f69bda9c4c049af27c63c488489784c72c`
- HMCL `main`：`0d20910ea302a907d6a01fec9244ab10d2993d5e`
- Prism `develop`：`c6ec97a0b82437569eee23db1714c0c92f520a09`
- Refract `main`：`8cd20b1aed54f1d32363b36fb1f2618a601cf78b`
- Helios `master`：`86e4316b963b54ff052be9ec80f316b8f842cf87`
