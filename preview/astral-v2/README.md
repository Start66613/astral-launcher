# Astral V2：运行与复现

**原 B 排布固定，三森林背景与颜色全部保留，默认启动随机且不连续重复。**

正式入口使用 `app/src/v2/`；评审入口复用同一源组件，以显式样例展示三动画、安装流程与边界状态。V2第一批代码已经作为 `70cc61f` 提交并推送，已正式接入，不再只是独立设计稿。未改技术栈、Rust或共享bridge/types。完整结果见 `docs/ui/astral-v2-design-review.md`。

从 `D:\软件开发\astral-ui` 启动：

```powershell
pnpm --config.verify-deps-before-run=false --dir app exec vite --host 127.0.0.1 --port 5198
node app/node_modules/vite/bin/vite.js --config preview/astral-v2/vite.config.ts
```

- [正式界面 http://127.0.0.1:5198/](http://127.0.0.1:5198/)：浏览器没有本地核心，显示真实未连接/空态；公开版本与资源仍可联网浏览。Tauri中读取实际版本、任务和会话。
- [设计评审室 http://127.0.0.1:5202/](http://127.0.0.1:5202/)：顶部有示例档案标记，场景选择不读写游戏目录。
- [三种启动动画](http://127.0.0.1:5202/#motion)：同事件帧逐步对比，非计时假成功。
- [主题设置](http://127.0.0.1:5202/#settings)、[游戏安装](http://127.0.0.1:5202/#game/1.21.1)、[资源详情](http://127.0.0.1:5202/#resource/sodium)。

外观存储分别为正式 `astral.v2.appearance` 和评审 `astral.v2.preview.preferences`。默认随机，关闭开关后固定所选森林。`?theme=nightfall&mode=dark`仅用于定向比较；设置中手动选择会清除theme参数。页面切换不重新抽取主题。

## 检查

```powershell
pnpm --config.verify-deps-before-run=false --dir app exec tsc --noEmit
pnpm --config.verify-deps-before-run=false --dir app exec vite build
node app/node_modules/typescript/bin/tsc -p preview/astral-v2/tsconfig.json --noEmit
node app/node_modules/vite/bin/vite.js build --config preview/astral-v2/vite.config.ts
node preview/astral-v2/verify.cjs
node preview/astral-v2/verify-production.cjs
```

先启动两个服务。测试借用现有Playwright/Chromium，可用 `PLAYWRIGHT_MODULE`/`CHROMIUM_PATH` 覆盖；不安装项目依赖。2026-10-24本轮实跑：tsc退出码0/0错误，build退出码0/68模块，verify.cjs **74通过/0失败**，verify-production.cjs **24通过/0失败**。截图/结果写入忽略目录。正式桩截图有清楚标记，没有启动真实游戏。

## 隔离 Windows 原生检查

本轮重用此前构建的隔离Tauri程序，verify-native.cjs **8通过/0失败**，1280×800 DPR1。它未包含核心新提交9f77843，不能用这8项声称真实取消和重启恢复已验收。先准备独立 `native-test/AppData/Astral/config.json`，gameDir指向独立 `native-test/game`。仅测试进程设置APPDATA、WEBVIEW2_USER_DATA_FOLDER，避免与用户已装Astral共享数据。Tauri devUrl临时配置指向5198，不修改仓库原配置。

编译曾遇并发C编译D8050，进程级 `CARGO_BUILD_JOBS=2`、`CARGO_PROFILE_DEV_DEBUG=0`后成功。未改Rust和全局配置。仅隔离测试进程启用 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9224`，运行 `node preview/astral-v2/verify-native.cjs` 检查真实WebView/IPC，无fixture。该脚本先断言游戏目录隔离，不安装或删除游戏。

`native-test/`、screenshots、dist与机器结果均忽略Git。不操作 `D:\Astral\Astral.exe`；退出验收时只关闭明确的本工作树测试进程。

## 边界

- **界面已可用**：B排布、三主题/随机、三套真实状态动效、现有接口的版本管理和任务展示。正式设置可选三种启动动效；`#motion`是评审室的并排比较入口。
- **界面有但核心能力缺失、明确禁用**：安全资源安装/完整依赖/世界ID、整合包、MSA、自定义Java、复制配置、回收站恢复、部分Loader与光影组件、限速并发、AI远程分析。不偷用全局downloadTo、不假登录或成功。
- **取消与持久化不是核心缺失**：DSH的9f77843已提供bootId、布尔cancelling、两阶段取消及INTERRUPTED。V2用UI适配层按任务字段门控取消按钮，旧核心隐藏。请求受理不等于停止，只有cancelled才显示已取消；不提供假续传或假重试。
- **尚未真机验收**：真实游戏启动/安装、新核心真实取消与重启对账、系统125%/150% DPI、读屏、高对比度及弱设备。125%/150%仅做浏览器模拟，不算原生系统DPI认证。

源码唯一入口 `app/src/v2/`，`preview/astral-v2/src/`多数为重导出；原三张图统一从 `app/public/forest/`引用。研究、许可、API提案见相邻ASSETS与docs/ui文档。
