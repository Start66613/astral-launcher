// Browser verification of the isolated design prototype. No Tauri/game commands.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'C:/Users/ASUS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = __dirname;
const out = path.join(root, 'screenshots');
const results = [];
const errors = [];
let browser;
async function test(name, run) { try { await run(); results.push({ name, passed: true }); } catch (error) { results.push({ name, passed: false, error: error.message }); } }
(async () => {
  fs.mkdirSync(out, { recursive: true });
  browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || 'C:/Users/ASUS/AppData/Local/ms-playwright/chromium_headless_shell-1228/chrome-headless-shell-win64/chrome-headless-shell.exe' });
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  page.setDefaultTimeout(6000);
  page.on('pageerror', error => errors.push(error.message));
  const goto = async (route = 'home', query = '') => { await page.goto(`http://127.0.0.1:5202/?${query}#${route}`); await page.locator('.route-panel:not([hidden]) h1').waitFor(); await page.waitForTimeout(600); assert.equal(await page.locator('vite-error-overlay').count(), 0); };
  const shot = async name => page.screenshot({ path: path.join(out, `${name}.png`) });
  const overflow = () => page.evaluate(() => ({ body: document.documentElement.scrollWidth > innerWidth, main: document.querySelector('main').scrollWidth > document.querySelector('main').clientWidth + 2 }));
  for (const [width, height] of [[1080, 640], [1280, 800], [1600, 900]]) {
    for (const theme of ['daylight', 'nightfall', 'fieldnotes']) for (const mode of ['dark', 'light']) {
      await test(`home ${width}x${height} ${theme} ${mode}`, async () => {
        await page.setViewportSize({ width, height }); await goto('home', `theme=${theme}&mode=${mode}`);
        assert.deepEqual(await overflow(), { body: false, main: false });
        const launch = await page.getByRole('button', { name: '启动游戏', exact: true }).boundingBox();
        assert(launch && launch.y + launch.height <= height, 'Launch action must remain visible');
        assert.equal(await page.locator('.is-home .ambient-art img.on').evaluate(img => img.naturalWidth > 0), true);
        await shot(`home-${theme}-${mode}-${width}`);
      });
    }
  }
  await page.setViewportSize({ width: 1280, height: 800 });
  await test('original B composition stays identical across all six theme/mode combinations', async () => {
    let reference;
    for (const theme of ['nightfall', 'daylight', 'fieldnotes']) for (const mode of ['dark', 'light']) {
      await goto('home', `theme=${theme}&mode=${mode}`);
      assert.equal(await page.locator('[data-layout="original-b"]').count(), 1);
      assert.equal(await page.locator('.landscape,.home-heading,.recent-grid').count(), 0);
      const geometry = await page.evaluate(() => ['.sidebar', '.b-hero h1', '.b-launch-dock', '.b-world-grid'].map(selector => {
        const rect = document.querySelector(selector).getBoundingClientRect();
        return [rect.x, rect.y, rect.width, rect.height].map(value => Math.round(value));
      }));
      assert.equal(geometry[0][2], 88, 'Original B uses an 88px icon rail');
      if (reference) assert.deepEqual(geometry, reference, 'Theme selection must not rearrange B');
      reference = geometry;
    }
  });
  await test('theme, wallpaper and motion persistence without query override', async () => {
    await goto('settings', 'theme=daylight&mode=light');
    if (await page.getByRole('switch', { name: '打开时随机切换森林' }).getAttribute('aria-checked') === 'true') await page.getByRole('switch', { name: '打开时随机切换森林' }).click();
    await page.getByRole('button', { name: /林野手札 暖纸白/ }).click();
    await page.getByRole('switch', { name: '森林壁纸' }).click();
    await page.getByRole('switch', { name: '界面动画', exact: true }).click();
    await page.goto('http://127.0.0.1:5202/#settings'); await page.locator('.route-panel:not([hidden]) h1').waitFor();
    assert.equal(await page.locator('html').getAttribute('data-theme'), 'fieldnotes');
    assert.equal(await page.locator('html').getAttribute('data-motion'), 'off');
    assert.equal(await page.locator('.ambient-art img.on').count(), 0);
    await page.getByRole('switch', { name: '森林壁纸' }).click(); await page.getByRole('switch', { name: '界面动画', exact: true }).click();
    await shot('settings-themes');
  });
  await test('version switching uses distinct preview IDs', async () => {
    await goto('home', 'theme=nightfall&mode=dark'); await page.getByRole('button', { name: '切换', exact: true }).click();
    await page.locator('dialog .choice-list button').filter({ hasText: '旧日时光' }).click();
    assert.match(await page.locator('.current-version').innerText(), /旧日时光/);
    await page.locator('dialog').waitFor({state:'detached'}); assert.equal(await page.locator('dialog').count(), 0);
  });
  await test('account state remains explicitly offline / Microsoft unavailable', async () => {
    await goto('home', 'scenario=no-account'); await page.locator('.account-control').click();
    await page.getByRole('button', { name: 'Microsoft 正版', exact: true }).click();
    assert.equal(await page.getByRole('button', { name: '登录 Microsoft', exact: true }).isDisabled(), true);
    assert.match(await page.locator('dialog').innerText(), /Microsoft 未登录/); await shot('account-unavailable'); await page.keyboard.press('Escape');
  });
  await test('920 catalog entries, virtualized DOM, search and fold', async () => {
    await goto('download/game');
    assert(Number(await page.locator('.version-viewport').getAttribute('data-total-rows')) > 900);
    assert(await page.locator('.catalog-version').count() < 20);
    await shot('download-game');
    await page.locator('.version-viewport').evaluate(el => el.scrollTop = 30000); await page.waitForTimeout(100);
    assert(await page.locator('.catalog-version').count() < 20);
    await page.getByRole('textbox', { name: '搜索 Minecraft 版本' }).fill('1.21.1');
    assert.match(await page.locator('.catalog-version').first().innerText(), /1.21.1/);
    const group = page.locator('.catalog-group').first(); await group.click(); assert.equal(await group.getAttribute('aria-expanded'), 'false');
  });
  await test('arbitrary actual Fabric builds and shader support conflict', async () => {
    await goto('game/1.21.1'); await page.getByRole('button', { name: /Fabric 可选择构建/ }).click();
    await page.getByText('选择其他兼容构建', { exact: false }).click();
    const select = page.getByRole('combobox', { name: 'Fabric 具体版本' }); const options = await select.locator('option').allTextContents(); assert(options.length > 10);
    await select.selectOption({ index: 4 }); await shot('game-install-fabric');
    await page.getByRole('button', { name: /Iris Fabric/ }).click();
    await page.getByRole('combobox', { name: 'Iris 具体版本' }).selectOption({ index: 1 });
    await page.getByRole('button', { name: /纯净原版 无需组件/ }).click();
    assert.equal(await page.getByRole('button', { name: /开始安装/ }).isDisabled(), true);
    assert.match(await page.locator('[role=alert]').innerText(), /需要 Fabric/); await shot('game-install-conflict');
    await page.getByRole('button', { name: /Fabric 可选择构建/ }).click();
    await page.getByRole('checkbox', { name: '安装 Fabric API', exact: true }).check();
    assert.equal(await page.getByRole('button', { name: /开始安装/ }).isDisabled(), true);
    await page.getByRole('combobox', { name: 'Fabric API 具体版本' }).selectOption({ index: 2 });
    assert.equal(await page.getByRole('button', { name: /开始安装/ }).isDisabled(), false);
    await page.getByRole('button', { name: /开始安装/ }).click();
    assert.match(await page.locator('dialog').innerText(), /没有创建任务/); await page.keyboard.press('Escape');
  });
  await test('resource chooses target then exact compatible file; shared target blocked', async () => {
    await goto('resource/sodium');
    assert.equal(await page.getByRole('button', { name: /确认安装/ }).isDisabled(), true);
    assert.equal(await page.locator('.route-panel:not([hidden]) .target-grid button').filter({ hasText: '旧日时光' }).isDisabled(), true);
    await page.locator('.route-panel:not([hidden]) .target-grid button').filter({ hasText: '林间小径' }).click();
    assert(await page.locator('.route-panel:not([hidden]) .file-option').count() >= 2);
    await page.locator('.route-panel:not([hidden]) .file-option').nth(1).click();
    assert.equal(await page.getByRole('button', { name: /确认安装/ }).isDisabled(), false);
    await shot('resource-sodium-file-choice');
    await page.getByRole('button', { name: /确认安装/ }).click();
    assert.match(await page.locator('dialog').innerText(), /尚无核心安全安装计划/); await page.keyboard.press('Escape');
  });
  await test('datapack requires world; modpack has separate independent version flow', async () => {
    await goto('resource/terralith'); await page.locator('.route-panel:not([hidden]) .target-grid button').filter({ hasText: '林间小径' }).click();
    await page.locator('.route-panel:not([hidden]) .file-option').first().click();
    assert.equal(await page.getByRole('button', { name: /确认安装/ }).isDisabled(), true);
    await page.getByRole('combobox', { name: '目标存档' }).selectOption('pine_home');
    assert.equal(await page.getByRole('button', { name: /确认安装/ }).isDisabled(), false); await shot('resource-datapack-world');
    await goto('resource/fabulously-optimized'); assert.equal(await page.locator('.route-panel:not([hidden]) .target-grid').count(), 0);
    assert.equal(await page.getByRole('textbox', { name: '整合包版本名称' }).count(), 1);
    await page.locator('.route-panel:not([hidden]) .file-option').first().click(); await shot('modpack-wizard');
  });
  for (const route of ['download/mod', 'download/modpack', 'download/datapack', 'download/resourcepack', 'download/shaderpack', 'versions', 'settings', 'research', 'more']) {
    await test(`secondary page ${route}`, async () => { await goto(route); assert.deepEqual(await overflow(), { body: false, main: false }); await shot(route.replace('/', '-')); });
  }
  await test('three animation prototypes share state, failure persists, process != session', async () => {
    await goto('motion');
    assert.equal(await page.getByRole('button', { name: '预览启动', exact: true }).count(), 3);
    await page.getByRole('button', { name: '预览启动', exact: true }).first().click();
    assert.equal(await page.locator('.motion-card[data-state=resolve]').count(), 3);
    for (let index = 0; index < 8; index++) {
      await page.getByRole('combobox', { name: '启动事件' }).selectOption(String(index));
      assert.equal(await page.locator(`.motion-card[data-state="${['idle','resolve','jvm','assets','download','process','running','exited'][index]}"]`).count(), 3);
      const progress = page.locator('.mock-preparation progress');
      if (index > 0 && index < 4) assert.equal(await progress.first().getAttribute('value'), null);
      if (index === 4) assert.equal(await progress.first().getAttribute('value'), '21');
      await shot(`motion-${index}`);
    }
    await page.getByRole('button', { name: '模拟启动失败' }).click();
    assert.equal(await page.locator('.motion-card[data-state=error]').count(), 3);
    await page.waitForTimeout(400); assert.equal(await page.locator('.mock-preparation.failed').count(), 3); await shot('motion-error');
    assert.equal(await page.locator('.launch-effect.active').count(), 0);
  });
  for (const scenario of ['empty', 'no-account', 'offline', 'download-error', 'launch-error', 'running', 'conflict']) {
    await test(`scenario ${scenario}`, async () => { await goto(scenario === 'conflict' ? 'game/1.21.1' : scenario === 'download-error' ? 'download/mod' : 'home', `scenario=${scenario}`); assert.deepEqual(await overflow(), { body: false, main: false }); await shot(`scenario-${scenario}`); });
  }
  await test('operating-system reduced motion overrides enabled preference', async () => {
    await context.clearCookies(); await page.emulateMedia({ reducedMotion: 'reduce' }); await goto('motion');
    await page.getByRole('combobox', { name: '启动事件' }).selectOption('4');
    assert.equal(await page.locator('html').getAttribute('data-motion'), 'off');
    assert.equal(await page.locator('.effect-fireflies i').first().evaluate(el => getComputedStyle(el).animationName), 'none');
    await shot('reduced-motion'); await page.emulateMedia({ reducedMotion: 'no-preference' });
  });
  await test('animation off and background visibility signal pause effects', async () => {
    await goto('motion'); await page.getByRole('combobox', { name: '启动事件' }).selectOption('4');
    await page.getByRole('button', { name: '关闭动画', exact: true }).click();
    assert.equal(await page.locator('.effect-voxel i').first().evaluate(el => getComputedStyle(el).animationName), 'none');
    await page.getByRole('button', { name: '开启动画', exact: true }).click();
    await page.evaluate(() => { Object.defineProperty(document, 'hidden', { configurable: true, value: true }); document.dispatchEvent(new Event('visibilitychange')); });
    assert.equal(await page.locator('html').getAttribute('data-paused'), 'true');
    assert.equal(await page.locator('.effect-fireflies i').first().evaluate(el => getComputedStyle(el).animationPlayState), 'paused');
    await page.evaluate(() => { delete document.hidden; document.dispatchEvent(new Event('visibilitychange')); });
    assert.equal(await page.locator('html').getAttribute('data-paused'), 'false');
  });
  await test('no-account launch opens account selection rather than proceeding', async () => {
    await goto('home', 'scenario=no-account'); await page.getByRole('button', { name: '启动游戏', exact: true }).click();
    assert.match(await page.locator('dialog').innerText(), /尚未选择离线档案/); await page.keyboard.press('Escape');
  });
  await test('failed wallpapers fall back without hiding controls', async () => {
    await page.route('**/*.webp', route => route.abort()); await goto('home', 'theme=nightfall');
    assert.equal(await page.locator('.is-home .ambient-art img.on').isHidden(), true); assert.equal(await page.getByRole('button', { name: '启动游戏', exact: true }).isVisible(), true);
    await shot('wallpaper-fallback'); await page.unroute('**/*.webp');
  });
  for (const [width, height] of [[1080, 640], [1280, 800], [1600, 900]]) {
    for (const route of ['download/game', 'game/1.21.1', 'resource/sodium', 'resource/terralith', 'resource/fabulously-optimized', 'versions', 'settings', 'motion']) {
      await test(`layout ${width}x${height} ${route}`, async () => {
        await page.setViewportSize({ width, height }); await goto(route);
        assert.deepEqual(await overflow(), { body: false, main: false });
        if (['game/1.21.1', 'resource/sodium', 'resource/terralith', 'resource/fabulously-optimized'].includes(route)) {
          const button = await page.locator('.route-panel:not([hidden]) .install-footer .button').boundingBox();
          assert(button && button.y >= 0 && button.y + button.height <= height, 'Fixed install action must remain fully visible');
        }
      });
    }
  }
  for (const scale of [1.25, 1.5]) {
    await test(`DPI browser emulation ${scale * 100}% (not native Windows certification)`, async () => {
      const scaled = await browser.newContext({ viewport: { width: Math.round(1600 / scale), height: Math.round(900 / scale) }, deviceScaleFactor: scale });
      const p = await scaled.newPage(); await p.goto('http://127.0.0.1:5202/?theme=nightfall#home'); await p.locator('.route-panel:not([hidden]) h1').waitFor(); await p.waitForTimeout(600);
      assert.equal(await p.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      await p.screenshot({ path: path.join(out, `scale-emulation-${scale * 100}.png`) }); await scaled.close();
    });
  }
  await test('no uncaught JavaScript errors', async () => assert.deepEqual(errors, []));
  fs.writeFileSync(path.join(root, 'test-results.json'), JSON.stringify({ testedAt: new Date().toISOString(), engine: 'Chromium headless on Windows', nativeTauriTested: false, results, errors }, null, 2));
  console.log(JSON.stringify({ passed: results.filter(r => r.passed).length, failed: results.filter(r => !r.passed), errors }, null, 2));
  await browser.close(); if (results.some(r => !r.passed)) process.exitCode = 1;
})().catch(async error => { console.error(error); await browser?.close(); process.exitCode = 1; });
