/* Phase 1 only. No Tauri bridge, requests, real accounts, or persistent app state. */
(() => {
  'use strict';

  const paths = {
    star: '<path d="m12 2 2.8 7.2L22 12l-7.2 2.8L12 22l-2.8-7.2L2 12l7.2-2.8Z"/>',
    home: '<path d="m3 10 9-7 9 7v10H3Z"/><path d="M9 20v-7h6v7"/>',
    layers: '<path d="m12 3 10 5-10 5L2 8Z"/><path d="m3 12 9 5 9-5M3 16l9 5 9-5"/>',
    download: '<path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5"/>',
    mods: '<path d="M8 3h4v4a3 3 0 1 1 5 2h4v6h-4a3 3 0 1 1-5 2v4H3v-7h4a3 3 0 1 0 1-5Z"/>',
    settings: '<path d="m10 3-1 3-3 1-3 3 2 3-1 4 4 1 2 3 4-2 4 1 1-4 2-3-3-3-1-4-4 1Z"/><circle cx="12" cy="12" r="3"/>',
    spark: '<path d="m12 3 2.3 6.7L21 12l-6.7 2.3L12 21l-2.3-6.7L3 12l6.7-2.3ZM20 2v4m-2-2h4"/>',
    arrow: '<path d="M4 12h16m-6-6 6 6-6 6"/>',
    chevron: '<path d="m8 4 8 8-8 8"/>',
    down: '<path d="m6 9 6 6 6-6"/>',
    user: '<circle cx="12" cy="8" r="4"/><path d="M4 22v-3a8 8 0 0 1 16 0v3"/>',
    moon: '<path d="M21 13A9 9 0 0 1 11 3a9 9 0 1 0 10 10Z"/>',
    sun: '<circle cx="12" cy="12" r="4"/><path d="M12 1v2m0 18v2M1 12h2m18 0h2M4 4l2 2m12 12 2 2M4 20l2-2M18 6l2-2"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    play: '<path d="m8 5 11 7-11 7Z"/>',
    check: '<path d="m5 12 4 4L20 5"/>',
    close: '<path d="m6 6 12 12M6 18 18 6"/>',
    folder: '<path d="M3 5h6l3 3h9v12H3Z"/>',
    leaf: '<path d="M20 3C10 2 3 6 4 13c1 7 12 9 15-1 1-3 1-6 1-9ZM5 21 16 9"/>',
    compass: '<circle cx="12" cy="12" r="9"/><path d="m16 8-3 5-5 3 3-5Z"/>',
    log: '<path d="M6 3h9l4 4v14H6Z"/><path d="M14 3v5h5M9 12h7M9 16h5"/>',
    arrowUp: '<path d="M5 19 19 5M5 5h14v14"/>',
  };
  const icon = (name, extra = '') => `<svg class="icon ${extra}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths[name] || paths.star}</svg>`;
  const concepts = {
    daylight: { letter: 'A', label: '林间晨光', mode: 'light', description: '清爽的鼠尾草绿，舒展的风景视窗。让日常操作一眼可见。' },
    nightfall: { letter: 'B', label: '萤火夜航', mode: 'dark', description: '深松绿与月光，沉浸式场景、安静的侧栏和聚焦的启动坞。' },
    fieldnotes: { letter: 'C', label: '林野手札', mode: 'light', description: '像翻开一本林间手札。纸本、衬线字与独立的旅程书签。' },
  };
  const instances = [
    { name: '森间漫游', version: '1.21.1', loader: 'Fabric', image: 'daylight', note: '给日常留一点旷野' },
    { name: '纯净原野', version: '1.21.1', loader: '原版', image: 'fieldnotes', note: '从第一块木头开始' },
    { name: '山谷工坊', version: '1.20.1', loader: 'Forge', image: 'nightfall', note: '把想象慢慢搭起来' },
  ];
  const params = new URLSearchParams(location.search);
  let concept = concepts[params.get('concept')] ? params.get('concept') : 'daylight';
  let mode = ['light', 'dark'].includes(params.get('theme')) ? params.get('theme') : concepts[concept].mode;
  let selected = 0;
  let instanceOpenerIndex = 0;
  let motionOff = params.get('motion') === 'off';
  const root = document.querySelector('#prototype');
  const dialog = document.querySelector('#interaction-dialog');
  const ai = document.querySelector('#ai-dialog');
  if (params.get('present') === '1') document.body.classList.add('presentation');

  const account = () => `<button type="button" class="account" data-action="account"><span class="avatar">${icon('user')}</span><span><b>未登录</b><small>连接 Microsoft 帐号</small></span>${icon('chevron')}</button>`;
  const nav = (horizontal = false) => `<nav class="app-nav ${horizontal ? 'horizontal-nav' : ''}" aria-label="启动器导航">${[['home','首页'],['layers','版本管理'],['download','下载中心'],['mods','模组中心'],['settings','设置']].map(([i,t],n)=>`<button type="button" class="nav-item ${n === 0 ? 'active' : ''}" data-action="${n === 0 ? 'home' : 'page'}" data-page="${t}" ${n === 0 ? 'aria-current="page"' : ''}>${icon(i)}<span>${t}</span>${n === 0 ? '<i class="nav-active-dot"></i>' : ''}</button>`).join('')}</nav>`;
  const chrome = () => `<header class="window-bar"><span class="window-label">${icon('star')} Astral Launcher <small>森林物语</small></span><div class="window-end"><button type="button" class="window-theme icon-button" data-action="theme" aria-label="切换深色或浅色主题">${icon(mode === 'light' ? 'moon' : 'sun')}</button><span class="prototype-tag">视觉原型</span><div class="window-controls" aria-hidden="true"><span>—</span><span>□</span><span>×</span></div></div></header>`;
  const status = () => `<footer class="app-status"><span><i></i> 本地设计预览 <em>·</em> 未连接游戏服务</span><span>Forest Tale <em>/</em> ${concepts[concept].letter} <span class="status-divider">·</span> <span class="status-version">Astral 0.1.0</span></span></footer>`;
  const brand = () => `<div class="app-brand"><span class="brand-emblem">${icon('star')}</span><div><b>Astral<span>.</span></b><small>YOUR LITTLE WORLD</small></div></div>`;
  const side = () => `<aside class="sidebar">${brand()}${nav()}<div class="sidebar-bottom"><button type="button" class="sidebar-ai" data-action="ai">${icon('spark')}<span>AI 诊断</span>${icon('arrowUp')}</button>${account()}</div></aside>`;
  const thumbnail = (item, cls = '') => `<img class="instance-thumb ${cls}" src="./assets/${item.image}.webp" alt="" width="52" height="52" loading="eager" />`;
  const launchButton = (text = '启动游戏') => `<button type="button" class="primary-button launch-button" data-action="launch">${icon('play')}<span>${text}</span>${icon('arrow')}</button>`;
  const instanceRows = (compact = false) => `<div class="world-list ${compact ? 'compact-worlds' : ''}">${instances.map((item,i)=>`<button type="button" class="world-row ${selected === i ? 'selected' : ''}" data-action="select" data-index="${i}" aria-pressed="${selected === i}">${thumbnail(item)}<span class="world-copy"><b>${item.name}</b><small>${item.version} <i>·</i> ${item.loader} <i>·</i> 示例实例</small></span><span class="world-trailing">${selected === i ? '<span class="selected-label">当前选择</span>' : icon('chevron')}</span></button>`).join('')}</div>`;
  const sectionHead = (title, sub, action = true) => `<div class="section-heading"><h2>${title}<small>${sub}</small></h2>${action ? '<button type="button" class="text-button" data-action="page" data-page="版本管理">管理实例 '+icon('arrow')+'</button>' : ''}</div>`;
  const aiCard = () => `<button type="button" class="ai-card" data-action="ai"><span class="ai-card-head">${icon('spark')}<span>旅途中的小帮手</span>${icon('arrowUp')}</span><strong>有点小状况？<br />一起找找原因。</strong><span class="ai-card-foot">AI 诊断 <i>·</i> 日志尚未导入 ${icon('arrow')}</span><span class="ai-orbit" aria-hidden="true">${icon('star')}</span></button>`;

  function daylight() {
    const item = instances[selected];
    return `${side()}<main class="daylight-main" aria-labelledby="home-heading"><header class="page-heading"><div><span class="eyebrow">A LITTLE ADVENTURE, EVERY DAY</span><h1 id="home-heading" tabindex="-1">今天，也去看看世界。</h1></div><span class="subtle-label">${icon('folder')} 尚未连接游戏目录</span></header><section class="daylight-hero" aria-label="森林物语主题场景"><img src="./assets/daylight.webp" alt="晨光下的方块森林、河流与林间木屋" width="1672" height="941" fetchpriority="high"/><div class="hero-veil"></div><div class="daylight-hero-copy"><span class="scene-label">${icon('leaf')} FOREST TALE</span><h2>把日常留在身后，<br />去森林里走走。</h2><p>一片森林，一间小屋。<br />还有属于你的、无限可能的世界。</p><button type="button" class="hero-link" data-action="story">遇见森林物语 ${icon('arrow')}</button></div><span class="landscape-coordinate">01 <i></i> THE OVERWORLD</span></section><section class="day-launch" aria-label="当前启动实例">${thumbnail(item)}<div class="launch-selection"><span class="eyebrow">下一站 · 示例实例</span><h2 data-selected-name>${item.name}</h2><p>${item.version} <i>·</i> ${item.loader}</p></div><button type="button" class="switch-instance" data-action="instances">切换实例 ${icon('down')}</button><div class="launch-separator"></div>${launchButton()}</section><section class="daylight-lower"><div class="library">${sectionHead('你的世界','03 / 示例')}${instanceRows(true)}</div>${aiCard()}</section></main>`;
  }

  function nightfall() {
    const item = instances[selected];
    return `<div class="night-scene" aria-hidden="true"></div>${side()}<main class="night-main" aria-labelledby="home-heading"><div class="night-kicker"><span class="scene-label">${icon('moon')} FOREST TALE / AFTER HOURS</span><button type="button" class="night-account" data-action="account">${icon('user')} 未登录 ${icon('down')}</button></div><section class="night-hero"><div><span class="night-overline">慢一点，世界会等你。</span><h1 id="home-heading" tabindex="-1">今夜，<br />住进一片森林。</h1><p>月光落在树梢，炉火亮在窗边。<br />去一个，只属于你的地方。</p><button type="button" class="hero-link" data-action="story">关于这片森林 ${icon('arrow')}</button></div><div class="night-margin-note"><span>01 / THE QUIET WORLD</span><div class="note-line"></div><p>在喧嚣之外<br />给想象留一盏灯</p></div></section><div class="night-bottom"><section class="night-launch-dock" aria-label="当前启动实例"><div class="dock-caption"><span class="eyebrow">YOUR NEXT ADVENTURE</span><span>示例实例 <i>·</i> 未连接</span></div><div class="dock-launch-row">${thumbnail(item)}<div class="launch-selection"><h2 data-selected-name>${item.name}</h2><p>${item.version} <i>·</i> ${item.loader}</p></div><button type="button" class="switch-instance" data-action="instances">切换 ${icon('down')}</button>${launchButton('进入世界')}</div></section><section class="night-worlds">${sectionHead('换一个世界','示例收藏',false)}<div class="night-world-grid">${instances.slice(1).map((world,i)=>`<button type="button" class="night-world-card" data-action="select" data-index="${i+1}"><img src="./assets/${world.image}.webp" alt="" width="200" height="130"/><span><b>${world.name}</b><small>${world.version} / ${world.loader}</small></span>${icon('arrowUp')}</button>`).join('')}<button type="button" class="night-ai" data-action="ai">${icon('spark')}<b>遇到问题时<br />我在这里。</b><span>AI 诊断 ${icon('arrow')}</span></button></div></section></div></main>`;
  }

  function fieldnotes() {
    const item = instances[selected];
    return `<main class="journal-main" aria-labelledby="home-heading"><header class="journal-topnav">${brand()}${nav(true)}<button type="button" class="journal-ai-button icon-button" data-action="ai" aria-label="打开 AI 诊断">${icon('spark')}</button></header><div class="journal-spread"><section class="journal-story"><div class="journal-edition"><span>THE FOREST ISSUE</span><span>VOL. 01 / 森林物语</span></div><h1 id="home-heading" tabindex="-1">在方块之间，<br />找回慢下来的时间<span>。</span></h1><p class="journal-subtitle">A little room for a bigger world.</p><figure class="journal-figure"><img src="./assets/fieldnotes.webp" alt="纸本水粉风格的方块森林与林间木屋" width="1448" height="1086" fetchpriority="high"/><figcaption><span>${icon('compass')} 林野手札 <i>—</i> 第一站，森林</span><span>FIG. 01</span></figcaption><span class="image-stamp" aria-hidden="true">A QUIET<br /><b>PLACE</b></span></figure><div class="journal-library-heading">${sectionHead('世界收藏','每一个都值得回来',false)}<button type="button" class="text-button" data-action="instances">翻阅收藏 ${icon('arrow')}</button></div><div class="journal-world-links">${instances.map((world,i)=>`<button type="button" data-action="select" data-index="${i}" aria-pressed="${selected === i}"><span class="journal-world-number">0${i+1}</span><span><b>${world.name}</b><small>${world.version} · ${world.loader}</small></span>${icon(selected === i ? 'check' : 'arrowUp')}</button>`).join('')}</div></section><aside class="journal-bookmark" aria-label="旅程书签"><section class="bookmark-card"><header><span class="eyebrow">旅程书签</span><span class="bookmark-number">01</span></header><div class="bookmark-leaf" aria-hidden="true">${icon('leaf')}</div><p class="bookmark-overline">YOUR NEXT CHAPTER</p><h2 data-selected-name>${item.name}</h2><p class="bookmark-description">${item.note}</p><dl><div><dt>游戏版本</dt><dd>${item.version}</dd></div><div><dt>加载器</dt><dd>${item.loader}</dd></div><div><dt>实例状态</dt><dd class="sample-state">示例 · 未连接</dd></div></dl>${launchButton('开始这段旅程')}<button type="button" class="bookmark-switch secondary-button" data-action="instances">换一本世界 ${icon('down')}</button><div class="bookmark-account">${account()}</div></section><button type="button" class="journal-note" data-action="ai"><span>${icon('spark')} 旅途小记</span><p>偶尔迷路也没关系，<br />我们可以一起找方向。</p><small>打开 AI 诊断 ${icon('arrow')}</small></button></aside></div></main>`;
  }

  function syncUrl() {
    const url = new URL(location.href);
    url.searchParams.set('concept', concept);
    url.searchParams.set('theme', mode);
    if (motionOff) url.searchParams.set('motion', 'off'); else url.searchParams.delete('motion');
    history.replaceState(null, '', url);
  }
  function render() {
    document.body.dataset.concept = concept;
    document.body.dataset.mode = mode;
    document.body.dataset.motion = motionOff ? 'off' : 'on';
    document.querySelectorAll('.concept-switch button').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.concept === concept)));
    document.querySelector('.review-tools [data-action="theme"]').innerHTML = icon(mode === 'light' ? 'moon' : 'sun');
    const motion = document.querySelector('[data-action="motion"]');
    motion.textContent = motionOff ? '动效关' : '动效开';
    motion.setAttribute('aria-pressed', String(motionOff));
    document.querySelector('#concept-description').innerHTML = `<b>${concepts[concept].letter} / ${concepts[concept].label}</b><span>${concepts[concept].description}</span>`;
    root.innerHTML = `<div class="launcher-shell">${chrome()}<div class="workspace">${({daylight,nightfall,fieldnotes})[concept]()}</div>${status()}</div>`;
    document.querySelectorAll('[data-action="close-dialog"],[data-action="close-ai"]').forEach(button => button.innerHTML = icon('close'));
    document.querySelector('.large-spark').innerHTML = icon('spark');
    document.querySelector('.log-icon').innerHTML = icon('log');
    document.title = `${concepts[concept].label} · Forest Tale · Astral Launcher`;
  }
  function openDialog(title, content) {
    document.querySelector('#dialog-content').innerHTML = `<h2 id="dialog-title">${title}</h2>${content}`;
    dialog.showModal();
  }
  function announce(text) { document.querySelector('#announcer').textContent = text; }
  document.addEventListener('click', event => {
    const conceptButton = event.target.closest('.concept-switch button');
    if (conceptButton) {
      concept = conceptButton.dataset.concept;
      mode = concepts[concept].mode;
      render(); syncUrl(); announce(`已切换到${concepts[concept].label}方案`);
      conceptButton.focus();
      return;
    }
    const target = event.target.closest('[data-action]');
    if (!target) return;
    const action = target.dataset.action;
    if (action === 'theme') { mode = mode === 'light' ? 'dark' : 'light'; render(); syncUrl(); announce(`已切换到${mode === 'dark' ? '深色' : '浅色'}主题`); document.querySelector(document.body.classList.contains('presentation') ? '.window-theme' : '.review-tools [data-action="theme"]')?.focus(); }
    if (action === 'motion') { motionOff = !motionOff; render(); syncUrl(); announce(motionOff ? '动效已关闭' : '动效已开启'); }
    if (action === 'home') document.querySelector('#home-heading').focus();
    if (action === 'select') { selected = Number(target.dataset.index); render(); announce(`已选择示例实例：${instances[selected].name}`); root.querySelector(`[data-action="select"][data-index="${selected}"]`)?.focus(); }
    if (action === 'instances') { instanceOpenerIndex = [...root.querySelectorAll('[data-action="instances"]')].indexOf(target); openDialog('选择下一站', '<p class="dialog-lead">以下均为视觉评审使用的示例实例，尚未扫描你的游戏目录。</p><div class="instance-picker">'+instances.map((item,i)=>`<button type="button" data-action="pick" data-index="${i}" aria-pressed="${selected === i}">${thumbnail(item)}<span><b>${item.name}</b><small>${item.version} · ${item.loader} · 示例</small></span>${icon(selected === i ? 'check' : 'chevron')}</button>`).join('')+'</div>'); }
    if (action === 'pick') { selected = Number(target.dataset.index); dialog.close(); render(); announce(`已选择示例实例：${instances[selected].name}`); root.querySelectorAll('[data-action="instances"]')[instanceOpenerIndex]?.focus(); }
    if (action === 'launch') openDialog('先留住这份期待', '<p class="dialog-lead">这是首页视觉原型，尚未连接游戏目录，也未执行游戏启动。</p><div class="honest-state">'+icon('folder')+'<span><b>当前状态：未连接</b><small>未读取本地实例，未验证帐号，未创建游戏进程。</small></span></div><p class="dialog-note">你选定视觉方案后，再接入真实的实例、帐号与启动状态。</p>');
    if (action === 'account') openDialog('连接你的帐号', '<p class="dialog-lead">当前未登录。此处用于评审帐号入口的位置与层级，不会发起登录或显示已验证身份。</p><div class="honest-state">'+icon('user')+'<span><b>Microsoft 帐号</b><small>正式登录流程将在后续与 DSH 协作接入。</small></span></div>');
    if (action === 'page') openDialog(target.dataset.page, '<p class="dialog-lead">本阶段只评审三个首页方向。选定视觉方案后，再沿用它的组件、布局和动效逐页设计。</p><p class="dialog-note">当前没有执行导航之外的安装、下载或配置操作。</p>');
    if (action === 'story') openDialog('森林物语 · Forest Tale', '<p class="dialog-lead">让启动器成为进入世界前的一次深呼吸。</p><p>森林的绿、木屋的暖与舒展的留白，围绕同一个清晰动作：选择世界，开始旅程。</p><p class="dialog-note">场景为本次概念设计生成的插画，不代表真实存档截图或可下载整合包。</p>');
    if (action === 'ai') ai.showModal();
    if (action === 'close-ai') ai.close();
    if (action === 'close-dialog') dialog.close();
    if (action === 'log-info') { target.outerHTML = '<p class="log-import-note" role="status">第二阶段会设计日志选择与脱敏确认流程。当前不会读取文件，也不会生成诊断结论。</p>'; }
  });
  for (const modal of [dialog, ai]) modal.addEventListener('click', event => { if (event.target === modal) { const r = modal.getBoundingClientRect(); if (event.clientX < r.left || event.clientX > r.right || event.clientY < r.top || event.clientY > r.bottom) modal.close(); } });
  render();
})();
