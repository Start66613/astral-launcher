# Forest Tale · 第二阶段 React 界面首版

日期：2026-10-23（Asia/Shanghai）。方向：用户选定的 **B / 萤火夜航**。

归档说明：本文记录阶段二首版完成时的状态。本轮已获用户明确授权提交并推送现有 UI 工作；截图、对比图和 JSON 测试结果保留在本机，由验证脚本重新生成，不纳入 Git。接口接入的新计划另见后续文档，本文所述核心能力以当时 UI 分支内容为准。

工作区 `D:\软件开发\astral-ui`，分支 `feature/forest-ui`，HEAD 保持 `8ce34f3`。开始前确认目录和分支匹配、没有已跟踪或暂存修改；26 个未跟踪文件全部为上一阶段交付，已保留。没有编辑其他工作区，也未提交、合并或推送。

## 已完成

- 将 B 的全幅夜景、窄侧栏、中文衬线标题、底部启动区实现为正式 React 界面；首页、版本管理、下载、模组、设置、帮助使用同一导航外壳。
- 新建可复用的 `Icon`、原生 `<dialog>` 模态组件和 `useLocalLauncher` 展示状态 hook。支持模态焦点约束、Esc 关闭和关闭后焦点返回。
- 建立深松绿/薄雾两套 Token、四个克制的强调色、圆角选项和减少动效。外观偏好在本机保存，非法偏好值安全回退；系统减少动效优先。
- 首页仅使用当前已有桥接接口返回的本地版本 ID。浏览器保持未连接、未登录和不可启动；不再渲染旧的虚构帐号、游玩统计、已安装版本或启动成功。
- 当前核心的启动路径继续可用：等待真实 PID 返回，才显示“进程已创建”；不声称游戏持续运行。防止重复启动，失败展示真实错误且可重试。
- 新版本管理页支持实际本地版本搜索、选择和返回首页。当前为版本模式；独立实例创建、复制、删除明确暂不可用。
- 下载页移除浏览器假清单和假安装；保留原生版本安装并增加本地筛选、分批展示。阶段进度明确标为当前阶段。下载页和已访问的资源分类在切页后保留状态，避免任务界面因路由切换丢失。
- 社区资源继续沿用已有 Modrinth 查询。浏览器只浏览，禁止下载；真实下载完成显示“已下载”，不会把 `.mrpack` 下载伪装成整合包已安装。
- 设置只展示实际可用项。外观即时生效；下载源和离线用户名改为明确保存，失败不会展示已保存。浏览器不修改游戏配置、不展示假设备内存。
- AI 抽屉支持用户主动选择 `.log/.txt`，最大 2 MB，仅在内存中预览前 60 行/最多 16,000 字符，可清除；不持久化或上传。分析服务未连接，按钮禁用，无虚构分析或修复结果。

## 运行与构建

正式 React 预览：<http://127.0.0.1:5198/>。

```powershell
Set-Location -LiteralPath 'D:\软件开发\astral-ui\app'
node node_modules/vite/bin/vite.js --host 127.0.0.1 --port 5198 --strictPort
```

第一阶段三个方案仍在 `preview/forest-tale/`，原预览端口为 5197。本次新界面使用独立端口，没有终止或占用 DSH 的开发服务。

本机依赖按已有锁文件安装。已有 pnpm 启动检查提示 esbuild 构建脚本未获执行；未修改构建许可策略，改用项目已安装的 Node 工具入口完成以下检查：

```powershell
node node_modules/typescript/bin/tsc --noEmit
node node_modules/vite/bin/vite.js build
```

两项均通过。安装工具自动插入的工作区配置占位项已逐行移除，`package.json`、锁文件和 `pnpm-workspace.yaml` 与原版本一致。

最终构建：52 个模块；主 JS 268.70 kB（gzip 84.27 kB），CSS 32.72 kB（gzip 7.46 kB），复用的夜景 WebP 217,712 字节。使用系统字体，不新增动画库、持续动画循环或常驻背景模糊；只在模态打开时使用遮罩模糊。这些是构建与实现检查，不是实际设备帧率/内存认证。

## 修改文件

以下路径相对于 `D:\软件开发\astral-ui`。

已修改：

- `app/index.html`：默认森林深色主题、页面标题与图标。
- `app/src/App.tsx`：六个视图、路由、外观偏好和状态展示。
- `app/src/components/TitleBar.tsx`：标题栏、主题切换与真实窗口操作入口。
- `app/src/components/Sidebar.tsx`：窄导航栏。
- `app/src/components/AiDrawer.tsx`：本地日志预览和未连接状态。
- `app/src/pages/Launch.tsx`：正式 B 首页。
- `app/src/pages/Download.tsx`：真实清单、过滤、安装反馈及状态保留。
- `app/src/pages/Resources.tsx`：资源浏览、请求取消、下载状态真实性与可用性。
- `app/src/pages/Settings.tsx`：外观、真实配置保存与设备信息。
- `app/src/pages/More.tsx`：帮助页视觉与搜索标签。
- `app/src/styles/tokens.css`：全局森林主题 Token。
- `app/src/main.tsx`：加载正式 `forest.css`；旧 `app.css` 保留但不再使用。
- `AGENTS.md`：仅本地、被 Git 排除；记录选定 B 和当前阶段。

新增：

- `app/src/components/Icon.tsx`
- `app/src/components/Modal.tsx`
- `app/src/hooks/useLocalLauncher.ts`
- `app/src/pages/Versions.tsx`
- `app/src/styles/forest.css`
- `app/public/forest/nightfall.webp`：复制本次第一阶段生成的夜景素材，未重写原素材。
- `preview/forest-tale/react/verify.cjs`
- `preview/forest-tale/react/test-results.json`
- `preview/forest-tale/react/screenshots/`：15 张截图/对比图。
- `docs/ui/forest-tale-phase-2.md`：本记录。

## 测试与截图

Playwright + Chromium `149.0.7827.55`，**75 项通过，0 项失败**。完整结果：[test-results.json](../../preview/forest-tale/react/test-results.json)。

- 六个页面 × 深浅主题 × 1440、1280、960、390、320px 宽度，60 项布局检查，无横向溢出。
- 浏览器未连接、未登录、无法启动；返回资源结果时下载按钮仍禁用。
- 导航历史和标题焦点；外观保存与非法本地偏好回退；减少动效。
- AI 抽屉焦点约束、Esc 返回、读取与清除日志、文件大小限制，以及没有日志上传请求。
- **隔离的接口桩测试**：只显示扫描返回的 ID；启动前等待真实结果、失败清除旧成功；安装跨页保留且完成前不显示成功；配置保存失败不伪装成功；读取失败不进入就绪状态。
- 没有未捕获的运行时 JavaScript 异常。

接口桩只存在于 Playwright 的模块拦截中，不随产品运行。测试没有启动真实 Minecraft、写真实游戏配置或执行真实下载；不能替代 Tauri 原生集成测试。

默认浏览器截图没有灌入示例版本：

- [B 原型与 React 首版对比](../../preview/forest-tale/react/screenshots/comparison-b-react.png)
- [正式深色首页](../../preview/forest-tale/react/screenshots/launch-dark-desktop.png)
- [正式浅色首页](../../preview/forest-tale/react/screenshots/launch-light-desktop.png)
- [深色窄屏首页](../../preview/forest-tale/react/screenshots/launch-dark-narrow.png)
- [浅色窄屏首页](../../preview/forest-tale/react/screenshots/launch-light-narrow.png)
- [版本管理未连接状态](../../preview/forest-tale/react/screenshots/versions-dark-desktop.png)
- [下载中心](../../preview/forest-tale/react/screenshots/download-dark-desktop.png)
- [模组中心](../../preview/forest-tale/react/screenshots/mods-dark-desktop.png)（测试中公开搜索返回为空）
- [深色设置](../../preview/forest-tale/react/screenshots/settings-dark-desktop.png)
- [浅色设置](../../preview/forest-tale/react/screenshots/settings-light-desktop.png)
- [深色窄屏设置](../../preview/forest-tale/react/screenshots/settings-dark-narrow.png)
- [浅色窄屏设置](../../preview/forest-tale/react/screenshots/settings-light-narrow.png)
- [帮助页](../../preview/forest-tale/react/screenshots/more-dark-desktop.png)
- [AI 抽屉](../../preview/forest-tale/react/screenshots/ai-dark-desktop.png)
- [版本管理接口桩截图](../../preview/forest-tale/react/screenshots/versions-native-test-fixture.png)（带显式测试标识，非真实本地实例）

已目视复查首页、深浅主题、窄屏、设置、版本卡片和 AI 抽屉。修正浅色首页右侧文案在暗部场景上的对比，并为 `color-mix()` 提供基础颜色回退。

复现：启动 5198 服务，安装可用的 Playwright/Chromium，然后运行 `node preview/forest-tale/react/verify.cjs`。支持 `ASTRAL_PLAYWRIGHT_MODULE` 和 `ASTRAL_BROWSER_EXECUTABLE` 指定本机已有测试工具，支持 `ASTRAL_REACT_URL` 修改服务地址。

## 核心边界与后续

本轮未修改 Rust/Tauri 文件、共享 `bridge.ts/types.ts`、依赖及锁文件；没有修改核心契约，没有合并 `feature/core-features`。

当前 UI 分支的实际核心仍是原有版本接口，因此保留兼容调用。独立实例 CRUD、实例目录隔离、新任务 ID/事件、帐号认证和远程 AI 分析，需要核心实现就绪并协调共享桥接后接入，不把仅有契约的能力宣称为已实现。

现有共享 `scanVersions()` 会把扫描错误转换为空列表，界面因此使用“暂未发现可用版本”的保守表达，不据此展示健康或已就绪。现有进程接口只有 PID，故不展示刷新后的持续运行状态。

下一阶段按真实数据与使用反馈逐页精修，并在经授权的核心联调环境中验证任务恢复、实例隔离、失败重试及真实设备性能。继续保持所有修改仅在 `feature/forest-ui`，合并和推送仍需用户确认。
