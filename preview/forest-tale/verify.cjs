// Run with Playwright available via NODE_PATH or ASTRAL_PLAYWRIGHT_MODULE.
// Tests never import Tauri, modify game data, or call a real login/launch API.
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require(process.env.ASTRAL_PLAYWRIGHT_MODULE || 'playwright');
const base = process.env.ASTRAL_PREVIEW_URL || 'http://127.0.0.1:5197';
const output = path.join(__dirname, 'screenshots');
const concepts = ['daylight','nightfall','fieldnotes'];
const cases = [];
const issues = [];
async function check(name, run) {
  try { await run(); cases.push({name, status:'passed'}); }
  catch (error) { cases.push({name, status:'failed', message:error.message}); }
}
async function main() {
  await fs.mkdir(output, {recursive:true});
  const browser = await chromium.launch({headless:true, ...(process.env.ASTRAL_BROWSER_EXECUTABLE ? {executablePath:process.env.ASTRAL_BROWSER_EXECUTABLE} : {})});
  const page = await browser.newPage({viewport:{width:1440,height:900}, deviceScaleFactor:1});
  const requests = [];
  page.on('pageerror', error => issues.push(error.message));
  page.on('request', request => { if (!request.url().startsWith(base) && !request.url().startsWith('data:')) requests.push(request.url()); });
  const defaultMode = name => name === 'nightfall' ? 'dark' : 'light';
  async function open(concept, mode=defaultMode(concept), present=true) {
    await page.goto(`${base}/?concept=${concept}&theme=${mode}&motion=off${present ? '&present=1' : ''}`);
    await page.locator('#home-heading').waitFor();
    await page.evaluate(() => Promise.all([...document.images].map(image => image.decode())));
  }
  async function noOverflow() {
    const state = await page.evaluate(() => {
      const elements = [...document.querySelectorAll('.workspace, main, .day-launch, .night-launch-dock, .journal-spread')];
      return { document:document.documentElement.scrollWidth <= innerWidth + 1, over:elements.filter(el=>el.scrollWidth > el.clientWidth + 2).map(el=>el.className) };
    });
    assert.equal(state.document,true,'page has horizontal overflow');
    assert.deepEqual(state.over,[],'content has horizontal overflow');
  }
  try {
    for (const concept of concepts) {
      for (const mode of ['light','dark']) {
        await page.setViewportSize({width:1440,height:900});
        await open(concept,mode);
        await check(`${concept}/${mode}: desktop layout and assets`, async()=>{
          await noOverflow();
          assert.equal(await page.locator('h1').count(),1);
          assert.equal(await page.locator('img').evaluateAll(images=>images.every(image=>image.complete && image.naturalWidth>0)),true);
          const button = await page.locator('[data-action="launch"]').boundingBox();
          assert.ok(button && button.y>=0 && button.y+button.height<=900,'primary action is clipped');
          assert.equal(await page.locator('body').getAttribute('data-mode'),mode);
        });
        await page.screenshot({path:path.join(output,`${concept}-${mode===defaultMode(concept)?'desktop':mode+'-desktop'}.png`)});
        for (const width of [1280,960,390,320]) {
          await page.setViewportSize({width,height:width>=960?800:844});
          await check(`${concept}/${mode}: ${width}px layout`, noOverflow);
          if(width===390) await page.screenshot({path:path.join(output,`${concept}-${mode===defaultMode(concept)?'mobile':mode+'-mobile'}.png`),fullPage:true});
        }
      }
      await page.setViewportSize({width:1440,height:900});
      await open(concept);
      await check(`${concept}: window theme control preserves keyboard focus`,async()=>{
        await page.locator('.window-theme').click();
        assert.equal(await page.locator('.window-theme').evaluate(button=>button===document.activeElement),true);
        await page.locator('.window-theme').click();
      });
      await check(`${concept}: instance picker updates the selected sample`,async()=>{
        await page.locator('[data-action="instances"]').first().click();
        assert.equal(await page.locator('#interaction-dialog').evaluate(dialog=>dialog.open),true);
        await page.locator('[data-action="pick"][data-index="1"]').click();
        assert.equal(await page.locator('[data-selected-name]').textContent(),'纯净原野');
        assert.equal(await page.locator('#interaction-dialog').evaluate(dialog=>dialog.open),false);
        assert.equal(await page.locator('[data-action="instances"]').first().evaluate(button=>button===document.activeElement),true,'picker must restore focus');
        if(await page.locator('[data-action="instances"]').count()>1) {
          await page.locator('[data-action="instances"]').nth(1).click();
          await page.locator('[data-action="pick"][data-index="2"]').click();
          assert.equal(await page.locator('[data-action="instances"]').nth(1).evaluate(button=>button===document.activeElement),true,'second picker entry must retain its own focus');
        }
      });
      await check(`${concept}: launch remains an honest disconnected preview`,async()=>{
        await page.locator('[data-action="launch"]').click();
        assert.match(await page.locator('#dialog-content').textContent(),/未执行游戏启动/);
        assert.match(await page.locator('#dialog-content').textContent(),/未验证帐号/);
        await page.keyboard.press('Escape');
        assert.equal(await page.locator('#interaction-dialog').evaluate(dialog=>dialog.open),false);
      });
      await check(`${concept}: account remains signed out`,async()=>{
        await page.locator('[data-action="account"]').first().click();
        assert.match(await page.locator('#dialog-content').textContent(),/当前未登录/);
        await page.keyboard.press('Escape');
      });
      await check(`${concept}: AI drawer, disabled analysis, keyboard focus trap`,async()=>{
        await page.locator('[data-action="ai"]').first().click();
        assert.equal(await page.locator('#ai-dialog').evaluate(dialog=>dialog.open),true);
        assert.equal(await page.locator('#ai-dialog .primary-button').isDisabled(),true);
        assert.match(await page.locator('#ai-dialog').textContent(),/还没有导入日志/);
        for(let i=0;i<8;i++) await page.keyboard.press('Tab');
        assert.equal(await page.evaluate(()=>document.querySelector('#ai-dialog').contains(document.activeElement)),true);
        if(concept==='daylight') await page.screenshot({path:path.join(output,'daylight-ai-drawer.png')});
        await page.keyboard.press('Escape');
        assert.equal(await page.locator('#ai-dialog').evaluate(dialog=>dialog.open),false);
      });
      await check(`${concept}: unfinished navigation is explicitly explained`,async()=>{
        await page.locator('[data-action="page"][data-page="下载中心"]').first().click();
        assert.match(await page.locator('#dialog-content').textContent(),/本阶段只评审三个首页方向/);
        await page.keyboard.press('Escape');
      });
      await open(concept,defaultMode(concept),false);
      await check(`${concept}: theme, motion, and switch controls`,async()=>{
        const old = await page.locator('body').getAttribute('data-mode');
        await page.locator('.review-tools [data-action="theme"]').click();
        assert.notEqual(await page.locator('body').getAttribute('data-mode'),old);
        assert.equal(await page.locator('.review-tools [data-action="theme"]').evaluate(button=>button===document.activeElement),true);
        await page.locator('[data-action="motion"]').click();
        assert.equal(await page.locator('body').getAttribute('data-motion'),'on');
        await page.locator('[data-action="motion"]').click();
        assert.equal(await page.locator('body').getAttribute('data-motion'),'off');
        for (const direction of concepts) {
          await page.locator(`.concept-switch [data-concept="${direction}"]`).click();
          assert.equal(await page.locator('body').getAttribute('data-concept'),direction);
          assert.equal(await page.locator(`.concept-switch [data-concept="${direction}"]`).getAttribute('aria-pressed'),'true');
        }
      });
    }
    await check('prefers-reduced-motion suppresses animated dialogs',async()=>{
      await page.emulateMedia({reducedMotion:'reduce'});
      await page.goto(`${base}/?concept=daylight`);
      await page.locator('[data-action="launch"]').click();
      assert.equal(await page.locator('#interaction-dialog').evaluate(dialog=>getComputedStyle(dialog).animationName),'none');
      await page.keyboard.press('Escape');
    });
    await check('no runtime JavaScript errors',async()=>assert.deepEqual(issues,[]));
    await check('no external API, login, game, or third-party font requests',async()=>assert.deepEqual(requests,[]));
    const report = {browser:await browser.version(),viewport:'1440×900 desktop; 1280/960×800; 390/320×844 narrow',passed:cases.filter(c=>c.status==='passed').length,failed:cases.filter(c=>c.status==='failed').length,cases};
    await fs.writeFile(path.join(__dirname,'test-results.json'),JSON.stringify(report,null,2)+'\n');
    console.log(JSON.stringify(report,null,2));
    if(report.failed) process.exitCode=1;
  } finally { await browser.close(); }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
