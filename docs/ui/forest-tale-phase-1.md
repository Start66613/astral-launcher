# Forest Tale 首页 · 第一阶段交付

交付日期：2026-10-23（Asia/Shanghai）。工作目录 `D:\软件开发\astral-ui`，分支 `feature/forest-ui`，基点 `8ce34f3d56266675b0661f422869843362ffceb5`。

归档说明：本文记录阶段一交付时的状态。后续按用户授权提交源码、素材和验证脚本；本文链接的截图和 JSON 结果保留在本机，由脚本重新生成，不纳入 Git。

开始修改前已核对 Git 根目录与分支，确认不在 main，且 `git status --porcelain=v2 --untracked-files=all` 没有未提交修改。第一阶段仅新增独立原型、素材、截图与文档，并更新本工作树被忽略的本地 `AGENTS.md` 以保存本轮授权和阶段边界。

## 三个方向

**A / 林间晨光（建议作为日常使用方向）**：暖白与鼠尾草绿。完整文字侧栏、宽幅风景视窗、紧邻场景的实例与启动区、开放式收藏列表。操作位置稳定，适合高频使用；场景占据主视觉但不会遮住关键操作。

**B / 萤火夜航**：月色与深松绿。窄导航栏、全幅夜景、大幅留白和底部启动坞。氛围感更强；需要在正式实现阶段继续验证背景更换和不同显示亮度下的文字对比，以及透明材质在低配设备上的成本。

**C / 林野手札**：纸白、橄榄绿和少量赭色。顶栏导航、衬线标题、装帧插画与独立旅程书签。视觉性格最鲜明；正式实现时需保持中文衬线字体的跨设备字形和排版一致性。

三个方案均提供深浅主题、可选择的示例实例、未登录入口、AI 抽屉及窄屏布局。所有示例、未连接与待接入状态均明确说明。原型不会读取游戏目录、发起登录、启动进程、下载文件、读取日志或请求 AI。

## 打开原型

```powershell
node 'D:\软件开发\astral-ui\preview\forest-tale\serve.cjs'
```

浏览器打开 <http://127.0.0.1:5197/>。顶部可切换 A/B/C、主题与动效。本地服务仅绑定 `127.0.0.1`。详见 [原型说明](../../preview/forest-tale/README.md)。

## 修改文件清单

以下路径都相对于 `D:\软件开发\astral-ui`，除 `AGENTS.md` 外均为新增文件：

- `AGENTS.md`：仅本地、已被 Git 排除；更新为第一阶段原型已获授权，正式 UI 等待用户选定方案。
- `preview/forest-tale/index.html`：原型入口。
- `preview/forest-tale/prototype.js`：三个独立首页构图及交互。
- `preview/forest-tale/prototype.css`：设计 Token、深浅主题与响应式样式。
- `preview/forest-tale/serve.cjs`：本地预览服务。
- `preview/forest-tale/verify.cjs`：自动化验证和截图。
- `preview/forest-tale/README.md`：运行与复现说明。
- `preview/forest-tale/test-results.json`：54 项自动检查的逐项结果。
- `preview/forest-tale/assets/daylight.webp`：晨光森林场景，435,094 字节。
- `preview/forest-tale/assets/nightfall.webp`：夜间森林场景，217,712 字节。
- `preview/forest-tale/assets/fieldnotes.webp`：手札水粉场景，536,042 字节。
- `preview/forest-tale/assets/prompts.json`：三个完整提示词；使用内置 `image_gen`，非 API/CLI 模式。
- `preview/forest-tale/screenshots/`：下面列出的 13 张截图。
- `docs/ui/forest-tale-interface-needs.md`：后续交给 DSH 协调的真实数据与状态需求。
- `docs/ui/forest-tale-phase-1.md`：本交付记录。

未修改正式 React 页面、应用 CSS Token、共享类型、桥接层、依赖清单、锁文件或任何 Rust/Tauri 文件。未执行提交、合并、推送、切换 main 或删除分支/工作树。

## 截图清单与视觉复查

截图均由实际 Chromium 渲染产生，桌面尺寸 1440×900；窄屏为 390px 宽的完整页面。路径相对于 `preview/forest-tale/screenshots/`。

- A 默认：[daylight-desktop.png](../../preview/forest-tale/screenshots/daylight-desktop.png)
- A 深色：[daylight-dark-desktop.png](../../preview/forest-tale/screenshots/daylight-dark-desktop.png)
- A 窄屏：[daylight-mobile.png](../../preview/forest-tale/screenshots/daylight-mobile.png)
- A 深色窄屏：[daylight-dark-mobile.png](../../preview/forest-tale/screenshots/daylight-dark-mobile.png)
- B 默认：[nightfall-desktop.png](../../preview/forest-tale/screenshots/nightfall-desktop.png)
- B 浅色：[nightfall-light-desktop.png](../../preview/forest-tale/screenshots/nightfall-light-desktop.png)
- B 窄屏：[nightfall-mobile.png](../../preview/forest-tale/screenshots/nightfall-mobile.png)
- B 浅色窄屏：[nightfall-light-mobile.png](../../preview/forest-tale/screenshots/nightfall-light-mobile.png)
- C 默认：[fieldnotes-desktop.png](../../preview/forest-tale/screenshots/fieldnotes-desktop.png)
- C 深色：[fieldnotes-dark-desktop.png](../../preview/forest-tale/screenshots/fieldnotes-dark-desktop.png)
- C 窄屏：[fieldnotes-mobile.png](../../preview/forest-tale/screenshots/fieldnotes-mobile.png)
- C 深色窄屏：[fieldnotes-dark-mobile.png](../../preview/forest-tale/screenshots/fieldnotes-dark-mobile.png)
- AI 抽屉：[daylight-ai-drawer.png](../../preview/forest-tale/screenshots/daylight-ai-drawer.png)

视觉复查覆盖三个方案的默认桌面、反向主题桌面、默认与反向主题窄屏，以及 AI 抽屉。已修正窄屏深浅主题遮罩覆盖关系、帐号副标题换行和窄屏状态栏截断；适当增大辅助文字字号。

## 测试报告

环境：Windows，Playwright + 已有 Chromium `149.0.7827.55`，未下载新浏览器或安装应用依赖。

自动检查 **54 项通过，0 项失败**。原始结果见 [test-results.json](../../preview/forest-tale/test-results.json)。

- 三个方案 × 深浅两主题，在 1440、1280、960、390、320px 宽度下检查横向溢出；1440px 桌面检查主启动按钮未被裁切、单一主标题和图片解码成功。
- 检查示例实例选择、名称更新、选择弹窗关闭和焦点返回；手札的两个选择入口分别返回原入口。
- 检查窗口栏与评审工具栏主题切换、方案切换和动效开关。
- 检查启动和帐号入口不会伪装真实成功，未完成导航明确告知阶段范围。
- 检查 AI 抽屉的未导入状态、禁用分析按钮、键盘 Tab 焦点约束与 Esc 关闭。
- 检查 `prefers-reduced-motion` 下取消动画。
- 检查运行时无 JavaScript 异常，且没有外部登录、游戏、AI API 或第三方字体请求。
- `node --check` 检查三个 JavaScript 文件通过。

性能方面，本阶段将三张生成 PNG 编码为同尺寸 WebP（quality 88），合计从 8,559,481 字节降到 1,188,848 字节，约减少 86.1%。使用系统字体，未引入运行时框架、动画库或常驻循环。尚未测量真实 Tauri 窗口中的帧率、内存、显卡占用或启动耗时，不将原型浏览器检查表述为性能认证。

未运行正式应用构建或真实 Minecraft 集成测试，因为正式源码、依赖和后端均未变化。本阶段也未完成全量无障碍认证。

## 下一阶段边界

等待用户选择 A/B/C 或指定组合；选定后才将视觉系统实现为 React 组件与正式 CSS Token，并逐页设计版本管理、下载、模组、设置和 AI 诊断。需要后端支持的状态已先写入 [接口需求](forest-tale-interface-needs.md)，由用户协调 DSH。任何合并、提交或推送仍需遵守用户授权。
