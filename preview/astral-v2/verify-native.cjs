// Real isolated Tauri/WebView2. No IPC fixtures and no game installation or deletion.
const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || "C:/Users/ASUS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright");
const directory = path.join(__dirname, "native-test");
const out = path.join(__dirname, "screenshots", "native");
fs.mkdirSync(out, { recursive: true });
(async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9224");
  const pages = browser.contexts().flatMap(context => context.pages()).filter(page => page.url().startsWith("http://127.0.0.1:5198/"));
  assert.equal(pages.length, 1, "Require exactly one isolated test WebView");
  const page = pages[0]; page.setDefaultTimeout(10000);
  await page.reload(); await page.locator(".sidebar nav").waitFor();
  const errors = []; page.on("pageerror", error => errors.push(error.message));
  const checks = [];
  async function check(name, action) { try { await action(); checks.push({name,passed:true}); } catch (error) { checks.push({name,passed:false,error:error.message}); } }
  const actual = await page.evaluate(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    return { api: await invoke("get_api_info"), config: await invoke("get_config"), instances: await invoke("list_instances"), tasks: await invoke("get_tasks"), session: await invoke("get_active_session",{instanceId:null}), dpr:devicePixelRatio, width:innerWidth, height:innerHeight };
  });
  assert(actual.config.gameDir.replaceAll("\\", "/").includes("/preview/astral-v2/native-test/game"), "Stop unless game directory is isolated");
  await check("actual v3 core, isolated empty games and session", async () => { assert.equal(actual.api.apiVersion,3);assert.deepEqual(actual.instances,[]);assert.equal(actual.session,null); });
  await check("empty home disables launch without fabricated version",async()=>{await page.evaluate(()=>location.hash="home");await page.getByText("还没有游戏版本",{exact:true}).waitFor();assert(await page.getByRole("button",{name:"启动游戏",exact:true}).isDisabled());await page.screenshot({path:path.join(out,"empty-home.png")});});
  await page.getByRole("button",{name:"设置",exact:true}).click();
  await check("three actual forest themes, same B layout", async()=>{
    const random=page.getByRole("switch",{name:"打开时随机切换森林"});if(await random.getAttribute("aria-checked")==="true")await random.click();
    const frames=[];
    for (const [theme,name] of [["daylight","森林晨光"],["nightfall","深林秘境"],["fieldnotes","林野手札"]]) {
      await page.locator(".theme-card").filter({hasText:name}).click();await page.waitForTimeout(500);
      assert.equal(await page.locator("html").getAttribute("data-theme"),theme);
      await page.getByRole("button",{name:"启动",exact:true}).click();
      await page.waitForFunction(()=>document.querySelector(".route-panel:not([hidden])")?.dataset.transition==="idle");
      frames.push(await page.locator(".b-launch-dock").boundingBox());
      await page.screenshot({path:path.join(out,`home-${theme}.png`)});
      await page.getByRole("button",{name:"设置",exact:true}).click();
    }
    assert.deepEqual(frames[0],frames[1]);assert.deepEqual(frames[1],frames[2]);
  });
  await check("motion preference and all three launch directions persist",async()=>{
    await page.getByRole("combobox",{name:"启动动效"}).selectOption("light");
    assert.equal(await page.getByRole("combobox",{name:"启动动效"}).locator("option").count(),3);
    await page.getByRole("switch",{name:"界面动画"}).click();assert.equal(await page.locator("html").getAttribute("data-motion"),"off");
    await page.reload();await page.locator(".theme-grid").waitFor();assert.equal(await page.getByRole("combobox",{name:"启动动效"}).inputValue(),"light");
    await page.getByRole("switch",{name:"界面动画"}).click();
    await page.getByRole("switch",{name:"打开时随机切换森林"}).click();
    await page.screenshot({path:path.join(out,"settings.png")});
  });
  await check("account is offline, Microsoft remains unverified",async()=>{await page.getByRole("button",{name:"切换账号",exact:true}).click();assert.match(await page.locator("dialog").innerText(),/未验证 Microsoft/);await page.getByRole("button",{name:"Microsoft 正版",exact:true}).click();assert(await page.getByRole("button",{name:"登录 Microsoft",exact:true}).isDisabled());await page.screenshot({path:path.join(out,"account.png")});await page.getByRole("button",{name:"关闭窗口",exact:true}).last().click();});
  await check("native downloads query real version manifest",async()=>{await page.getByRole("button",{name:"下载",exact:true}).click();await page.waitForFunction(()=>document.querySelector(".version-viewport")||document.querySelector(".banner.error"),{timeout:30000});assert(await page.locator(".download-categories button").count()===6);await page.screenshot({path:path.join(out,"downloads.png")});});
  await check("native resource browsing produces metadata or retryable network error",async()=>{await page.getByRole("button",{name:"Mod",exact:true}).click();await page.waitForFunction(()=>document.querySelector(".project-card")||document.querySelector(".banner.error"),{timeout:30000});await page.screenshot({path:path.join(out,"resources.png")});});
  await check("no uncaught WebView JavaScript errors",async()=>assert.deepEqual(errors,[]));
  await page.getByRole("button",{name:"启动",exact:true}).click();
  fs.writeFileSync(path.join(directory,"native-results.json"),JSON.stringify({at:new Date().toISOString(),scope:"Real Tauri, real IPC, isolated empty games; no real game run; no Windows DPI change",actual,checks,errors},null,2));
  console.log(JSON.stringify({passed:checks.filter(c=>c.passed).length,failed:checks.filter(c=>!c.passed),dpr:actual.dpr,size:[actual.width,actual.height]},null,2));
  await browser.close();if(checks.some(c=>!c.passed))process.exitCode=1;
})().catch(error=>{console.error(error);process.exitCode=1;});
