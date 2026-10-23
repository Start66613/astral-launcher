# Forest Tale · 首页视觉提案

第一阶段的独立原型，尚未替换 `app/src/`。所有实例都是明确标注的示例；没有接入帐号、游戏目录、启动、下载或 AI 服务。

归档说明：第二阶段已在 `app/src/` 实现用户选定的 B 方案，本目录保留三个独立原型。`screenshots/`、`react/screenshots/` 和对应的 `test-results.json` 是本机验证产物，已加入 Git 忽略规则；运行各阶段验证脚本可重新生成，源码和正式 WebP 素材仍提交。

## 本地预览

需要 Node.js，无需安装依赖。在 PowerShell 运行：

```powershell
node 'D:\软件开发\astral-ui\preview\forest-tale\serve.cjs'
```

打开 <http://127.0.0.1:5197/>。服务只监听本机 `127.0.0.1`，按 `Ctrl+C` 停止。若端口已被本原型服务占用，直接打开现有地址；不要终止不明进程。

- A 林间晨光：<http://127.0.0.1:5197/?concept=daylight>
- B 萤火夜航：<http://127.0.0.1:5197/?concept=nightfall>
- C 林野手札：<http://127.0.0.1:5197/?concept=fieldnotes>

顶部切换方案、深浅主题与动效。URL 参数 `theme=light|dark` 固定主题，`motion=off` 关闭动效，`present=1` 隐藏外层评审工具栏。沉浸视图仍可使用窗口栏的主题按钮。

可体验实例选择、版本收藏、AI 抽屉、帐号入口和启动按钮反馈。非首页导航会说明后续阶段范围，不会伪装成已实现页面。

## 文件

- `index.html`：原型入口及对话框/抽屉容器。
- `prototype.js`：三个首页构图、示例状态、主题与键盘交互。
- `prototype.css`：独立设计 Token、深浅主题、响应式和动效。
- `assets/*.webp`：三张本次生成的森林插画；并非真实存档截图。
- `assets/prompts.json`：完整生成提示词、工具及素材编码方式。
- `serve.cjs`：零依赖的本地静态预览服务。
- `verify.cjs`：Playwright 浏览器验证与截图脚本。
- `test-results.json`：当前验证结果。
- `screenshots/`：13 张浏览器实渲截图。

## 复现测试

先启动上述服务，再让测试脚本能够找到本机安装的 Playwright：

```powershell
# 已正常安装 Playwright 及匹配 Chromium 时：
node 'D:\软件开发\astral-ui\preview\forest-tale\verify.cjs'
```

也支持通过 `ASTRAL_PLAYWRIGHT_MODULE` 指定 Playwright 模块路径，通过 `ASTRAL_BROWSER_EXECUTABLE` 指定已有 Chromium 可执行文件，通过 `ASTRAL_PREVIEW_URL` 指定预览地址。这些仅用于测试，不会修改应用依赖、锁文件或其他工作树。

本次使用 Codex 已有 Playwright 与本机 Chromium 149 完成验证，没有为原型安装项目依赖。结果为 54 项通过、0 项失败。覆盖五种宽度、深浅主题、选择反馈、模态焦点、Esc 关闭、减少动效及无外部服务请求；不代表真实 Minecraft/Tauri 集成测试或完整无障碍认证。

完整交付记录：`D:\软件开发\astral-ui\docs\ui\forest-tale-phase-1.md`。
