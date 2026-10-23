// UI tests only. Native operations are mocked at the module boundary.
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require(process.env.ASTRAL_PLAYWRIGHT_MODULE || 'playwright');
const base = process.env.ASTRAL_REACT_URL || 'http://127.0.0.1:5198';
const out = path.join(__dirname, 'screenshots');
const cases = [];
const errors = [];
async function check(name, run) {
  try { await run(); cases.push({name,status:'passed'}); }
  catch(error) { cases.push({name,status:'failed',message:error.message}); }
}
const nativeModule = `
const state = () => window.__forestTest;
export const inTauri = true;
export async function getSystemInfo() { return {os:'UI TEST DEVICE',cpuCores:8,totalMemGB:16,freeMemGB:8,demo:false}; }
export async function getConfig() { if(state().failConfig) throw new Error('PERMISSION: UI test read failure'); return {...state().config}; }
export async function setConfig(config) { state().calls.push({method:'setConfig',config}); if(state().failSave) throw new Error('DISK_FULL: UI test save failure'); state().config={...config}; }
export async function scanVersions() { state().calls.push({method:'scanVersions'}); return [...state().versions]; }
export async function listVersions() { return [{id:'1.21.4',kind:'release',released:'2024-12-03'},{id:'1.21.1',kind:'release',released:'2024-08-08'},{id:'1.20.1',kind:'release',released:'2023-06-12'},{id:'test-snapshot',kind:'snapshot',released:'2026-01-01'}]; }
export async function onProgress(callback) { state().listeners.add(callback); return () => state().listeners.delete(callback); }
export function launchVersion(version) { state().calls.push({method:'launchVersion',version}); return new Promise((resolve,reject)=>{state().pendingLaunch={resolve,reject};}); }
export function installVersion(version) { state().calls.push({method:'installVersion',version}); return new Promise((resolve,reject)=>{state().pendingInstall={resolve,reject};}).then(()=>{state().versions.push(version);}); }
export async function downloadTo(url,kind,filename,onPct) { state().calls.push({method:'downloadTo',url,kind,filename}); onPct?.(100); return filename; }
export async function openPath(path) { state().calls.push({method:'openPath',path}); }
export async function minimizeWindow() {} export async function toggleMaximize() {} export async function closeWindow() {}
`;

async function main() {
  await fs.mkdir(out,{recursive:true});
  const browser = await chromium.launch({headless:true,...(process.env.ASTRAL_BROWSER_EXECUTABLE ? {executablePath:process.env.ASTRAL_BROWSER_EXECUTABLE} : {})});
  const context = await browser.newContext({viewport:{width:1440,height:900},deviceScaleFactor:1});
  await context.route('https://api.modrinth.com/**',route=>route.fulfill({contentType:'application/json',body:JSON.stringify({hits:[]})}));
  const page = await context.newPage();
  page.on('pageerror',error=>errors.push(error.message));
  async function open(view='launch',theme='dark') {
    await page.goto(`${base}/?theme=${theme}&motion=off#${view}`);
    await page.locator('h1:visible').waitFor();
    await page.evaluate(()=>document.fonts.ready);
  }
  async function geometry(target=page) {
    const measurements = await target.evaluate(()=>({
      doc:document.documentElement.scrollWidth <= innerWidth + 1,
      over:[...document.querySelectorAll('.content,.page,.forest-home,.dock-main,.home-world-grid,.settings-layout,.rail-nav')].filter(el=>el.getClientRects().length && el.scrollWidth > el.clientWidth + 2).map(el=>el.className),
    }));
    assert.equal(measurements.doc,true,'document overflows horizontally');
    assert.deepEqual(measurements.over,[],'content overflows horizontally');
  }
  async function nav(label,target=page) { await target.getByRole('navigation',{name:'主导航'}).getByRole('button',{name:label,exact:true}).click(); }
  async function nativePage(extra={}) {
    const ctx = await browser.newContext({viewport:{width:1440,height:900}});
    await ctx.addInitScript(options=>{window.__forestTest={config:{gameDir:'C:/UI_TEST_ONLY/game',source:'official',username:'TestOffline'},versions:['1.21.1','1.20.1'],calls:[],listeners:new Set(),...options};},extra);
    await ctx.route(/\/src\/lib\/bridge\.ts(?:\?.*)?$/,route=>route.fulfill({contentType:'application/javascript',body:nativeModule}));
    await ctx.route('https://api.modrinth.com/**',route=>route.fulfill({contentType:'application/json',body:JSON.stringify({hits:[]})}));
    const target = await ctx.newPage();
    target.on('pageerror',error=>errors.push(error.message));
    await target.goto(`${base}/?theme=dark&motion=off`);
    await target.waitForFunction(()=>window.__forestTest.calls.some(call=>call.method==='scanVersions'));
    return {ctx,target};
  }
  try {
    for(const theme of ['dark','light']) for(const view of ['launch','versions','download','mods','settings','more']) {
      await open(view,theme);
      for(const width of [1440,1280,960,390,320]) {
        await page.setViewportSize({width,height:width===1440?900:width>=960?800:844});
        await check(`${view}/${theme}/${width}px: responsive geometry`,()=>geometry());
        if(width===1440 && (theme==='dark'||view==='launch'||view==='settings')) await page.screenshot({path:path.join(out,`${view}-${theme}-desktop.png`)});
        if(width===390 && (view==='launch'||view==='settings')) await page.screenshot({path:path.join(out,`${view}-${theme}-narrow.png`)});
      }
    }
    await page.setViewportSize({width:1440,height:900});
    await open();
    await check('browser never invents versions, identity, or launch success',async()=>{
      assert.equal(await page.locator('.launch-button').isDisabled(),true);
      const text=await page.locator('#root').innerText();
      assert.match(text,/桌面功能未连接/); assert.doesNotMatch(text,/Start66613|已修复|已创建快照|进程已创建/);
      await page.locator('.account-pill').click();
      assert.match(await page.locator('dialog[open]').innerText(),/没有已验证的登录身份/);
      await page.keyboard.press('Escape');
    });
    await check('navigation history and heading focus',async()=>{
      await nav('版本管理'); await nav('设置'); await page.goBack();
      await page.waitForURL(url=>url.hash==='#versions');
      assert.equal(await page.locator('h1:visible').innerText(),'版本管理');
      assert.equal(await page.locator('h1:visible').evaluate(el=>el===document.activeElement),true);
    });
    await check('appearance changes persist, with valid fallback for malformed storage',async()=>{
      await page.locator('.theme-toggle').click();
      assert.equal(await page.locator('html').getAttribute('data-theme'),'light');
      await page.goto(base); assert.equal(await page.locator('html').getAttribute('data-theme'),'light');
      await page.evaluate(()=>localStorage.setItem('astral.skin','not-json'));
      await page.goto(`${base}/?theme=invalid&accent=invalid`);
      assert.equal(await page.locator('html').getAttribute('data-theme'),'dark');
      assert.equal(await page.locator('html').getAttribute('data-accent'),'green');
    });
    await check('settings exposes only available controls in browser mode',async()=>{
      await nav('设置');
      assert.equal(await page.getByLabel('离线用户名',{exact:true}).isDisabled(),true);
      assert.equal(await page.getByRole('button',{name:'保存设置',exact:true}).isDisabled(),true);
      await page.getByRole('button',{name:'林间薄雾',exact:true}).click();
      assert.equal(await page.locator('html').getAttribute('data-theme'),'light');
      await page.getByRole('button',{name:'减少动效',exact:true}).click();
      assert.equal(await page.locator('html').getAttribute('data-motion'),'off');
    });
    await check('browser resource downloads stay disabled even when search returns results',async()=>{
      await page.route('https://api.modrinth.com/v2/search**',route=>route.fulfill({contentType:'application/json',body:JSON.stringify({hits:[{project_id:'ui-test-only',title:'UI TEST RESOURCE',description:'Test fixture, no real package',icon_url:null,downloads:42,categories:[]}]})}));
      await page.goto(`${base}/?motion=off#mods`);
      await page.getByText('UI TEST RESOURCE',{exact:true}).waitFor();
      assert.equal(await page.getByRole('button',{name:'下载',exact:true}).isDisabled(),true);
      await page.unroute('https://api.modrinth.com/v2/search**');
    });
    await open();
    await check('AI dialog traps focus, closes with Escape, and restores focus',async()=>{
      const trigger=page.getByRole('button',{name:'AI 诊断',exact:true});
      await trigger.click();
      assert.equal(await page.locator('dialog[open]').count(),1);
      assert.equal(await page.getByRole('button',{name:'开始分析',exact:true}).isDisabled(),true);
      for(let i=0;i<9;i++) await page.keyboard.press('Tab');
      assert.equal(await page.evaluate(()=>document.querySelector('.ai-drawer').contains(document.activeElement)),true);
      await page.screenshot({path:path.join(out,'ai-dark-desktop.png')});
      await page.keyboard.press('Escape');
      assert.equal(await trigger.evaluate(el=>el===document.activeElement),true);
    });
    await check('local log preview never uploads or fabricates analysis',async()=>{
      const requests=[];
      const track=request=>{if(!request.url().startsWith(base)&&!request.url().startsWith('data:')) requests.push(request.url());};
      page.on('request',track);
      await page.getByRole('button',{name:'AI 诊断',exact:true}).click();
      await page.getByLabel('选择本地日志').setInputFiles({name:'ui-test.log',mimeType:'text/plain',buffer:Buffer.from('INFO UI TEST ONLY\nNo real game has been started.')});
      await page.getByLabel('本地日志预览').waitFor();
      assert.match(await page.getByLabel('本地日志预览').innerText(),/UI TEST ONLY/);
      assert.equal(await page.getByRole('button',{name:'开始分析',exact:true}).isDisabled(),true);
      await page.getByRole('button',{name:'清除日志',exact:true}).click();
      assert.equal(await page.getByLabel('本地日志预览').count(),0);
      await page.getByLabel('选择本地日志').setInputFiles({name:'too-large.log',mimeType:'text/plain',buffer:Buffer.alloc(2*1024*1024+1,65)});
      await page.getByRole('alert').waitFor();
      assert.match(await page.getByRole('alert').innerText(),/超过 2 MB/);
      assert.deepEqual(requests,[]);
      page.off('request',track); await page.keyboard.press('Escape');
    });
    await check('system reduced motion overrides animated dialogs',async()=>{
      await page.emulateMedia({reducedMotion:'reduce'});
      await page.goto(`${base}/?motion=on`);
      await page.getByRole('button',{name:'AI 诊断',exact:true}).click();
      assert.equal(await page.locator('dialog[open]').evaluate(el=>getComputedStyle(el).animationName),'none');
      await page.keyboard.press('Escape');
    });
    const {ctx:coreCtx,target:core}=await nativePage();
    await check('native fixture: only scanned IDs are presented',async()=>{
      await core.getByRole('heading',{name:'1.21.1',exact:true}).waitFor();
      assert.equal(await core.locator('.world-tile').count(),2);
      assert.equal(await core.locator('.launch-button').isEnabled(),true);
      await core.locator('.switch-version').click();
      await core.getByRole('dialog').getByRole('button',{name:'1.20.1',exact:true}).click();
      assert.equal(await core.locator('.dock-selection h2').innerText(),'1.20.1');
    });
    await check('native fixture: launch waits for result and is protected against duplicate clicks',async()=>{
      await core.locator('.launch-button').click();
      await core.waitForFunction(()=>Boolean(window.__forestTest.pendingLaunch));
      assert.equal(await core.locator('.launch-button').isDisabled(),true);
      assert.equal(await core.locator('.launch-feedback').getByText(/进程已创建/).count(),0);
      await core.evaluate(()=>{for(const callback of window.__forestTest.listeners) callback('1.20.1',{stage:'jvm',label:'UI TEST preparing JVM',done:0,total:0,bytes:0,totalBytes:0});});
      assert.match(await core.locator('.launch-feedback').innerText(),/preparing JVM/);
      await core.evaluate(()=>window.__forestTest.pendingLaunch.resolve(4242));
      await core.getByText(/进程已创建 · PID 4242/).waitFor();
      assert.match(await core.locator('.launch-feedback').innerText(),/后续运行状态尚未接入/);
      assert.equal(await core.evaluate(()=>window.__forestTest.calls.filter(call=>call.method==='launchVersion').length),1);
    });
    await check('native fixture: failed launch clears stale success and remains retryable',async()=>{
      await core.locator('.launch-button').click();
      await core.waitForFunction(()=>window.__forestTest.calls.filter(call=>call.method==='launchVersion').length===2);
      await core.evaluate(()=>window.__forestTest.pendingLaunch.reject(new Error('JAVA_MISSING: UI TEST')));
      await core.getByRole('alert').waitFor();
      assert.match(await core.getByRole('alert').innerText(),/JAVA_MISSING/);
      assert.equal(await core.getByText(/进程已创建 · PID/).count(),0);
      assert.equal(await core.locator('.launch-button').isEnabled(),true);
    });
    await check('native fixture: installation survives navigating away without premature success',async()=>{
      await nav('下载中心',core);
      const row=core.locator('li.row').filter({hasText:'1.21.4'});
      await row.getByRole('button',{name:'安装',exact:true}).click();
      await core.waitForFunction(()=>Boolean(window.__forestTest.pendingInstall));
      assert.equal(await row.getByText('已安装',{exact:true}).count(),0);
      await nav('设置',core); await nav('下载中心',core);
      assert.equal(await row.getByRole('button',{name:'安装中…',exact:true}).isDisabled(),true);
      assert.equal(await core.evaluate(()=>window.__forestTest.calls.filter(call=>call.method==='installVersion').length),1);
      await core.evaluate(()=>window.__forestTest.pendingInstall.resolve());
      await row.getByText('已安装',{exact:true}).waitFor();
    });
    await check('native fixture: settings save only reports success after resolution',async()=>{
      await nav('设置',core);
      await core.getByLabel('离线用户名',{exact:true}).fill('ForestTest');
      await core.getByRole('button',{name:'保存设置',exact:true}).click();
      await core.getByRole('status').filter({hasText:'已保存到本机'}).waitFor();
      assert.equal(await core.evaluate(()=>window.__forestTest.config.username),'ForestTest');
      await core.evaluate(()=>{window.__forestTest.failSave=true;});
      await core.getByLabel('离线用户名',{exact:true}).fill('UnsavedTest');
      await core.getByRole('button',{name:'保存设置',exact:true}).click();
      await core.getByRole('alert').waitFor();
      assert.match(await core.getByRole('alert').innerText(),/DISK_FULL/);
      assert.equal(await core.evaluate(()=>window.__forestTest.config.username),'ForestTest');
      assert.equal(await core.getByText('已保存到本机',{exact:true}).count(),0);
    });
    await nav('版本管理',core);
    await core.evaluate(()=>{const label=document.createElement('div');label.textContent='UI 测试桩 · 未调用真实核心';label.style.cssText='position:fixed;right:20px;bottom:42px;padding:8px 12px;background:#183b2b;color:#f4f7e6;z-index:9999;font:12px sans-serif;border:1px solid #b5c59b;border-radius:6px';document.body.append(label);});
    await core.screenshot({path:path.join(out,'versions-native-test-fixture.png')});
    await coreCtx.close();
    const {ctx:failureCtx,target:failure}=await nativePage({failConfig:true});
    await check('native fixture: read failures remain errors, never fake readiness',async()=>{
      await failure.getByRole('alert').waitFor();
      assert.match(await failure.getByRole('alert').innerText(),/PERMISSION/);
      assert.equal(await failure.locator('.launch-button').isDisabled(),true);
    });
    await failureCtx.close();
    await check('all scenarios: no uncaught runtime JavaScript errors',async()=>assert.deepEqual(errors,[]));
    const result={browser:await browser.version(),passed:cases.filter(item=>item.status==='passed').length,failed:cases.filter(item=>item.status==='failed').length,nativeTestBoundary:'Playwright substitutes bridge.ts; no real native calls, game processes, downloads, or user configuration writes.',cases};
    await fs.writeFile(path.join(__dirname,'test-results.json'),JSON.stringify(result,null,2)+'\n');
    console.log(JSON.stringify({browser:result.browser,passed:result.passed,failed:result.failed,failures:cases.filter(item=>item.status==='failed')},null,2));
    if(result.failed) process.exitCode=1;
  } finally { await context.close(); await browser.close(); }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
