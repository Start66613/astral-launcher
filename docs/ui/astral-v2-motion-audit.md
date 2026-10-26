# Astral V2：全局页面过渡与交互动效审查

日期：2026-10-24。工作区 `D:\软件开发\astral-ui`，分支 `feature/astral-v2-ui`，开始时 HEAD=`af6ae1a`、工作树干净且与远端一致。只读检查 `feature/astral-v2-integration@b572332`，其 V2 页面与本分支没有差异；没有切分支、合并或覆盖集成侧工作。

## 先审计，再修复

对照 `app/src/styles/forest.css` 与 V2 实际运行：

- 旧版 `.page` 有 `page-enter .2s ease-out`，3px位移。旧文件没有被删除，但 V2 未继承统一页面进入机制，navigate/history 只直接换 React 条件分支。
- 旧版普通 `.modal[open]` 有 `modal-enter .18s`，V2通用Modal没有对应动画；关闭时直接卸载，两版均缺少完整退出管理。
- 旧版AI抽屉有 `.24s drawer-enter`；V2保留了右侧入场 `diagnostic-in .22s`，但没有对称退出与中途反向。
- V2品牌/导航/hero的entrance主要是首次入场；`home-b.css` 的workspace `animation:none!important`又覆盖了workspace入场，不能用它代替路由动画。
- V2已有部分按钮颜色反馈和Toggle位移，缺少统一press、卡片、Tab、分类及列表反馈。详情折叠原来由浏览器瞬时开闭。

因此没有只修改duration。修复了路由生命周期、焦点、弹窗presence和局部内容切换，并保留旧文件用于对照。

## 四种动效各自独立

1. **首次启动器入场**：保留品牌/导航entrance，使用一次性的data-app-enter限制重播；入场开关只控制首次进入，不关闭日常导航动画。
2. **日常路由过渡**：RouteDeck按每次实际提交的路由revision触发，240ms、opacity .62→1、Y 6px→0，返回为-6px；节能缩至2px。固定侧栏、标题栏和森林图层不参与位移。
3. **局部交互**：MotionRegion用于Tab、分类内容、文件刷新；MotionDetails用于小型折叠；按钮/卡片/Toggle用统一CSS Token。
4. **游戏启动状态动效**：森林萤火、像素方块、极简光影原实现与真实Task/GameSession驱动保留，不借路由过渡推进状态。

Motion Token：fast=140ms、normal=220ms、page=240ms、dialog=200ms；缓动统一为cubic-bezier(.16,1,.3,1)，页面距离6px。二级页内容可延后18ms形成轻微节奏，最大258ms；固定安装栏不参与该位移。主题图片原650ms交叉淡化保留。

实现使用React状态、CSS与标准Web Animations API，无新依赖；没有采用整页View Transitions快照。当前实际WebView2 UA为Edge/Chromium155，WAAPI存在，原生运行已检查。没有该API时页面、弹窗与折叠即时完成，内容仍可操作；不是所有历史WebView2版本认证。

## 路由、输入与业务保护

- 覆盖启动/下载/设置/更多/版本管理、原版到组件安装、搜索到资源详情，以及返回。
- 已访问页面保留在窗口内，隐藏面板使用hidden/inert，不可见、不可聚焦，不是两套可点击页面。搜索、安装名、Loader构建、目标/文件选择及局部状态留在原组件。
- 路由动画不使用React key强制重建，不额外发IPC或网络请求。首次访问的正常数据请求保留；回到已访问页面不因动画重新查询。
- 在途安装与启动的组件、外层Task/GameSession hook不被动画卸载。没有改安装步骤、认证、目录或启动核心。
- history保存独立的路由索引，popstate/hashchange去重，快速导航采用最后一次意图；二级返回优先回到真实上一项，无历史时回退上级。各页面滚动位置分别恢复。
- 焦点在当前过渡完成后进入h1；旧过渡取消后不能抢焦点。用户已经在新页面输入或操作控件时不抢焦点；对话框打开时也不把焦点移到背后。
- 资源请求期间保留旧文件列表的视觉位置，但明确标为上次结果并禁用选择；成功后换新列表，失败清掉不适用的文件。搜索刷新也保留旧结果并标注，避免空白闪烁或把旧结果误认为新匹配。
- 资源详情标题节点在元数据加载前后保持稳定，避免异步响应使标题焦点丢失。

## 弹窗、抽屉和局部反馈

- 账号、任务、版本选择、安装预览确认、版本编辑/删除与高级配置使用统一ModalPresence：打开200ms淡入/微移，关闭保留原树直到退出完成。
- 关闭期间inert与事件拦截阻止重复提交；快速反向从当前opacity/transform继续，不跳回起点。账号Tab保留编辑输入并固定基本高度，避免弹窗忽高忽低。
- AI抽屉左右方向一致；从启动失败入口直接打开，抽屉中保留真实错误，同时不清空原错误或上传日志。
- 虚拟版本列表的折叠只做轻量内容淡入与箭头反馈，不动画数百行的总高度；DOM仍保持可见行+overscan。
- Loader其他构建、资源发行文件、错误原因/日志详情等小区域以高度+透明度展开，可在中途反向；原生select保留键盘与系统交互，只统一边框/颜色反馈。
- 选中版本/Loader/目标、导航分类、hover/press、卡片、Toggle、任务进度条及错误提示使用统一反馈。未知进度仍为非确定动画，没有假百分比或计时成功。

系统prefers-reduced-motion优先于用户开关；关闭模式无动画但有完整状态。后台与隐藏面板暂停非必要CSS循环；节能关闭弹窗背景模糊并减少位移。没有对大面积森林做持续滤镜动画。

## 实跑验证

- TypeScript：退出码0，0错误。正式Vite build：退出码0，70模块；主JS144.24kB/gzip44.23kB，CSS67.74kB/gzip14.06kB；没有新运行依赖。
- `verify.cjs`：74通过、0失败。原有三主题、三个启动效果、安装流程展示和窗口适配回归保留。
- `verify-production.cjs`：24通过、0失败，真实React/hooks/bridge配隔离IPC桩。任务revision、取消、会话、错误保留等原断言保留。
- `verify-motion.cjs`：29通过、0失败、0未捕获异常。覆盖连续/快速导航、二级返回、浏览器历史、DOM身份与输入、网络请求次数、固定安装栏、账号/任务/版本弹窗、抽屉、虚拟折叠、Loader反向、目标文件刷新、主题历史、动画off/减少动画/节能、intro分离、下载中和启动中切页、1080×640/1280×800及DPI模拟；额外生成开/关两段真实浏览器录像。
- `verify-native.cjs`：8通过、0失败，隔离真实Windows Tauri/WebView2。旧脚本已改为过渡完成后比较静态主题布局。
- `verify-native-motion.cjs`：6通过、0失败、0未捕获异常，真实WebView2/IPC：固定壳层、240ms内容过渡、账号输入、对称关闭、动画off、媒体减少动画模拟、CPU4倍限速+节能导航。实际窗口1280×800、DPR1。

最初回归失败来自旧脚本的瞬时关闭断言和全DOM选择器命中隐藏缓存页；已改为等待退出结束、只选择当前可见路由，未放宽业务/布局/Task断言。录屏对照页两段视频已实际加载并同步播放，0 JS异常。

## 实际查看与复现

- [实际界面](http://127.0.0.1:5198/)：直接连续点击导航、进入下载详情、开关弹窗。
- [动效开启/关闭对照](http://127.0.0.1:5202/motion-review.html)：两段真实React浏览器录像，可同步播放；带IPC/元数据桩标记，没有真的安装或启动游戏。
- 本地录像：`preview/astral-v2/screenshots/motion/motion-on.webm` 与 `motion-off.webm`；截图、JSON结果也在此目录，均被Git忽略。运行verify-motion.cjs可重新生成。
- 真机运动验证需要独立APPDATA、游戏目录及WebView2资料目录，并仅对隔离测试程序开放CDP9224。重用原8项脚本的隔离环境；不要接管用户已装启动器。

## 修改文件与边界

- 新增`app/src/v2/motion.tsx`、`motion.css`：路由、presence、局部折叠及Token。
- `App.tsx`、`state.tsx`：路由/历史/面板、焦点、全局弹窗、Tab；账号只调整UI输入保留，不改认证。
- `Download.tsx`、`LiveDownload.tsx`：返回、分类/折叠、安装栏稳定、请求中旧列表展示；没有修改安装执行函数。
- `HomeB.tsx`、`LiveRuntime.tsx`：版本选择/任务/编辑等弹窗、版本Tab、诊断抽屉上下文。
- `styles.css`、`home-b.css`、`runtime.css`与两个main入口：移除覆盖和重复首次入场，把全局motion样式接到正式与评审入口。
- `verify*.cjs`、`motion-review.html`及本文/README：验证、对照与交付记录。

没有改Rust、共享bridge/types、包清单、锁文件、实例目录或原Forest Tale三案；没有合并/推送main，也没有在集成工作树编辑。代码留在当前UI工作区供审阅。

## 未验证项

Windows系统125%/150%缩放未修改或真机认证；目前为浏览器DPR1.25/1.5及对应有效视口模拟。CPU4倍限速是模拟，不代替真实弱设备；未做长时间访问数百资源详情页的内存压力测试。面板保留到窗口关闭，后续大规模长会话需要继续profile。

没有真实安装或启动Minecraft，没有用这轮动效检查证明核心安装/取消/认证的正确性。新核心取消的Rust联调、实际断网/文件写入、读屏和Windows高对比度仍不在本轮已验证范围。正式浏览器未连接核心时仍为真实空态，不回退生成假的游戏或成功状态。
