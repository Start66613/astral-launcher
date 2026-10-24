# Astral V2：运行与复现

**原 B 排布固定，三森林背景与颜色全部保留，默认启动随机且不连续重复。**

正式入口使用 `app/src/v2/`；评审入口复用同一源组件，以显式样例展示三动画、安装流程与边界状态。未改技术栈、Rust或共享bridge/types。完整结果见 `docs/ui/astral-v2-design-review.md`。

从 `D:\软件开发\astral-ui` 启动：

```powershell
pnpm --config.verify-deps-before-run=false --dir app exec vite --host 127.0.0.1 --port 5198
node app/node_modules/vite/bin/vite.js --config preview/astral-v2/vite.config.ts
```

- [正式5198](http://127.0.0.1:5198/#home)：浏览器没有本地核心，显示真实未连接/空态；公开版本与资源仍可联网浏览。Tauri中读取实际版本、任务和会话。
- [设计评审5202](http://127.0.0.1:5202/#home)：顶部有示例档案标记，场景选择不读写游戏目录。
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

先启动两个服务。测试借用现有Playwright/Chromium，可用 `PLAYWRIGHT_MODULE`/`CHROMIUM_PATH` 覆盖；不安装项目依赖。74项设计检查与19项正式IPC桩检查的截图/结果写入忽略目录。正式桩截图有清楚标记，没有启动真实游戏。

## 隔离 Windows 原生检查

本机已用VS2022 DevShell、Rust运行实际Tauri，8项检查通过，1280×800 DPR1。先准备独立 `native-test/AppData/Astral/config.json`，gameDir指向独立 `native-test/game`。仅测试进程设置APPDATA、WEBVIEW2_USER_DATA_FOLDER，避免与用户已装Astral共享数据。Tauri devUrl临时配置指向5198，不修改仓库原配置。

编译曾遇并发C编译D8050，进程级 `CARGO_BUILD_JOBS=2`、`CARGO_PROFILE_DEV_DEBUG=0`后成功。未改Rust和全局配置。仅隔离测试进程启用 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9224`，运行 `node preview/astral-v2/verify-native.cjs` 检查真实WebView/IPC，无fixture。该脚本先断言游戏目录隔离，不安装或删除游戏。

`native-test/`、screenshots、dist与机器结果均忽略Git。不操作 `D:\Astral\Astral.exe`；退出验收时只关闭明确的本工作树测试进程。

## 边界

界面可用不等于所有核心能力已齐。资源安全安装/完整依赖/整合包/MSA/取消恢复/部分Loader与光影组件仍明确不可用；不偷用全局downloadTo、不假登录或成功。系统125%/150%仅做浏览器模拟，未做原生系统DPI认证；真实游戏安装/运行仍待端到端验收。

源码唯一入口 `app/src/v2/`，`preview/astral-v2/src/`多数为重导出；原三张图统一从 `app/public/forest/`引用。研究、许可、API提案见相邻ASSETS与docs/ui文档。
