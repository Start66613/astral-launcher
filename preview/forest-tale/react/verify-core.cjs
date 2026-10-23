// Isolated IPC fixtures only. The real bridge.ts and contract mergeTask run unchanged.
// No Rust process, user directory, real game, install, or download is touched.
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require(process.env.ASTRAL_PLAYWRIGHT_MODULE || 'playwright');
const base = process.env.ASTRAL_REACT_URL || 'http://127.0.0.1:5198';
const out = path.join(__dirname, 'screenshots', 'core-v3.2');
const checks = [], errors = [];
const stamp = '2026-10-23T14:00:00Z';
function instance(id, name, managed = true, health = {ok:true,reasons:[]}) {
  const dir = managed ? `C:/UI_TEST_ONLY/game/instances/${id}` : 'C:/UI_TEST_ONLY/game';
  const dirs = Object.fromEntries(['mods','config','saves','logs','resourcepacks','shaderpacks','screenshots'].map(key=>[key,`${dir}/${key}`]));
  return {id,name,gameVersion:'1.21.1',managed,dir,dirs,loader:null,modsCount:2,modsDisabled:1,cover:null,health,lastPlayed:null,totalPlaytimeMs:0,memoryMb:null,jvmArgs:[],javaPath:null,createdAt:stamp};
}
const pine = instance('inst_pine','月下松林');
const river = instance('inst_river','河畔营地'); // Same gameVersion: IDs must remain distinct.
const legacy = instance('ver_1.21.1','旧日旅途',false);
function launchTask(id, state = 'done') {
  return {taskId:id,revision:5,kind:'launch',instanceId:pine.id,gameVersion:pine.gameVersion,title:'启动 月下松林',stage:state==='done'?'done':'jvm',label:state==='done'?'完成':'准备 Java',done:0,total:0,bytes:0,totalBytes:0,state,error:null,result:state==='done'?{kind:'launch',sessionId:'sess_existing',pid:4242,startedAt:stamp}:null,startedAt:stamp,endedAt:state==='done'?stamp:null,retryOf:null};
}
const existingSession = {sessionId:'sess_existing',instanceId:pine.id,taskId:'task_existing',pid:4242,startedAt:stamp,state:'running',exitCode:null,endedAt:null,durationMs:null};

function installFixture(options) {
  let callbackSeq = 0, listenerSeq = 0, taskSeq = 0;
  const callbacks = new Map(), listeners = new Map();
  const clone = value => structuredClone(value);
  const state = window.__coreTest = {
    instances:options.instances, config:{gameDir:'C:/UI_TEST_ONLY/game',source:'official',username:'TestOffline',activeInstanceId:options.activeId},
    tasks:options.tasks || [], sessions:options.sessions || [], calls:[], pendingSessions:[],
    apiVersion:options.apiVersion ?? 3, features:options.features || ['instances','tasks','sessions','mods'],
    ...options,
  };
  state.emit = (event,payload) => {
    for (const [id,listener] of listeners) if (listener.event===event) callbacks.get(listener.handler)?.({event,id,payload:clone(payload)});
  };
  state.task = (taskId,patch) => {
    const old = state.tasks.find(item=>item.taskId===taskId);
    const next = {...old,...patch};
    state.tasks = state.tasks.filter(item=>item.taskId!==taskId).concat(next);
    state.emit('astral://task',next); return next;
  };
  state.releaseSessions = () => { state.holdSession=false; for (const pending of state.pendingSessions.splice(0)) pending.resolve(pending.snapshot); };
  state.finishLaunch = () => {
    const task = state.tasks.find(item=>item.taskId===state.lastLaunch);
    const session = {sessionId:`sess_${task.taskId}`,instanceId:task.instanceId,taskId:task.taskId,pid:4242,startedAt:task.startedAt,state:'running',exitCode:null,endedAt:null,durationMs:null};
    state.sessions.push(session);
    state.task(task.taskId,{revision:5,state:'done',stage:'done',label:'完成',endedAt:new Date().toISOString(),result:{kind:'launch',sessionId:session.sessionId,pid:session.pid,startedAt:session.startedAt}});
    return session;
  };
  state.exit = (sessionId,exitCode = 0) => {
    const session=state.sessions.find(item=>item.sessionId===sessionId);
    Object.assign(session,{state:'exited',exitCode,endedAt:new Date().toISOString(),durationMs:120000});
    state.emit('astral://game-exit',{sessionId,instanceId:session.instanceId,taskId:session.taskId,exitCode,crashed:exitCode!==0,durationMs:120000,endedAt:session.endedAt});
  };
  function newTask(kind,instanceId,gameVersion) {
    const task={taskId:`ui_task_${++taskSeq}`,revision:1,kind,instanceId,gameVersion,title:kind==='create'?'创建独立实例':'启动实例',stage:'queued',label:'排队中',done:0,total:0,bytes:0,totalBytes:0,state:'queued',error:null,result:null,startedAt:new Date().toISOString(),endedAt:null,retryOf:null};
    state.tasks.push(task); state.emit('astral://task',task); return clone(task);
  }
  window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(event,id){ listeners.delete(id); }};
  window.__TAURI_INTERNALS__={
    transformCallback(callback){ const id=++callbackSeq; callbacks.set(id,callback); return id; },
    unregisterCallback(id){ callbacks.delete(id); },
    async invoke(cmd,args={}) {
      state.calls.push({cmd,args:clone(args)});
      switch(cmd) {
        case 'plugin:event|listen': {
          if(state.failListen && args.event==='astral://game-exit') throw 'INTERNAL|UI TEST listener failure';
          const id=++listenerSeq; listeners.set(id,args); return id;
        }
        case 'plugin:event|unlisten': listeners.delete(args.eventId); return;
        case 'get_api_info': if(state.missingInfo) throw 'Command get_api_info not found'; if(state.failInfo) throw 'NETWORK|UI TEST capability read failed'; return {apiVersion:state.apiVersion,appVersion:'UI TEST CORE',features:state.features};
        case 'get_system_info': return {os:'UI TEST DEVICE',cpuCores:8,totalMemGB:16,freeMemGB:8,demo:false};
        case 'get_config': return clone(state.config);
        case 'set_config': Object.assign(state.config,args.config); return clone(state.config);
        case 'scan_versions': return [...new Set(state.instances.map(item=>item.gameVersion))];
        case 'list_instances': if(state.failList) throw 'PERMISSION|UI TEST cannot read instance directory'; return clone(state.instances);
        case 'get_active_instance': return clone(state.instances.find(item=>item.id===state.config.activeInstanceId) || null);
        case 'get_instance': return clone(state.instances.find(item=>item.id===args.id) || null);
        case 'set_active_instance':
          if(state.failSelect) throw 'PERMISSION|UI TEST cannot save selection';
          if(state.holdSelect) await new Promise(resolve=>{state.releaseSelect=resolve;});
          state.config.activeInstanceId=state.selectOverride ?? args.id; return clone(state.config);
        case 'get_tasks': return clone(state.tasks.filter(item=>(!args.kind||item.kind===args.kind)&&(!args.state||item.state===args.state)&&(!args.instanceId||item.instanceId===args.instanceId)));
        case 'get_task': return clone(state.tasks.find(item=>item.taskId===args.taskId) || null);
        case 'get_active_session': {
          if(state.failSession) throw 'PERMISSION|UI TEST session query failed';
          const snapshot=clone([...state.sessions].reverse().find(item=>item.instanceId===args.instanceId && (item.state!=='exited'||state.returnExited)) || null);
          if(state.holdSession) return new Promise(resolve=>state.pendingSessions.push({resolve,snapshot}));
          return snapshot;
        }
        case 'list_sessions': return clone([...state.sessions].reverse().filter(item=>!args.instanceId||item.instanceId===args.instanceId).slice(0,args.limit||50));
        case 'start_launch': {
          if(state.rejectLaunch) throw 'JAVA_MISSING|UI TEST Java unavailable';
          const inst=state.instances.find(item=>item.id===args.instanceId);
          const task=newTask('launch',args.instanceId,inst.gameVersion); state.lastLaunch=task.taskId;
          if(state.holdLaunchResponse) return new Promise(resolve=>{state.releaseLaunchResponse=()=>resolve(task);});
          return task;
        }
        case 'start_create_instance': state.createRequest=clone(args.request); const task=newTask('create',null,args.request.gameVersion); state.lastCreate=task.taskId; return task;
        case 'update_instance': {
          if(state.failEdit) throw 'PERMISSION|UI TEST cannot rename';
          const inst=state.instances.find(item=>item.id===args.id); Object.assign(inst,args.patch); state.emit('astral://instance-changed',{instanceId:inst.id,change:'updated'}); return clone(inst);
        }
        case 'delete_instance':
          if(state.busyDelete) throw 'INSTANCE_BUSY|UI TEST instance in use';
          state.instances=state.instances.filter(item=>item.id!==args.id); if(state.config.activeInstanceId===args.id) state.config.activeInstanceId=null;
          state.emit('astral://instance-changed',{instanceId:args.id,change:'deleted'}); return clone(state.config);
        default: throw `UNSUPPORTED|Unexpected UI test command ${cmd}`;
      }
    },
  };
}

async function main() {
  await fs.mkdir(out,{recursive:true});
  const browser=await chromium.launch({headless:true,...(process.env.ASTRAL_BROWSER_EXECUTABLE?{executablePath:process.env.ASTRAL_BROWSER_EXECUTABLE}:{})});
  async function fixture(options={},run) {
    const context=await browser.newContext({viewport:{width:1440,height:1000},deviceScaleFactor:1});
    await context.addInitScript(installFixture,{instances:[pine,river,legacy],activeId:pine.id,...options});
    await context.route('https://api.modrinth.com/**',route=>route.fulfill({contentType:'application/json',body:'{"hits":[]}'}));
    const page=await context.newPage(); page.on('pageerror',error=>errors.push(error.message));
    page.setDefaultTimeout(8000);
    try {
      await page.goto(`${base}/?theme=dark&motion=off`);
      if(options.apiVersion<3 || options.missingInfo || options.features && !options.features.includes('instances')) await page.waitForFunction(()=>window.__coreTest.calls.some(call=>call.cmd==='scan_versions'));
      else if(options.failInfo || options.failListen || options.failList) await page.getByRole('alert').waitFor();
      else await page.locator('.session-feedback:not([data-phase="recovering"])').waitFor();
      await run(page);
    } finally { await context.close(); }
  }
  async function check(name,run) {
    try { await run(); checks.push({name,status:'passed'}); }
    catch(error) { checks.push({name,status:'failed',message:error.message}); }
  }
  async function phase(page,value) { await page.locator(`.session-feedback[data-phase="${value}"]`).waitFor(); }
  async function nav(page,label) { await page.getByRole('navigation',{name:'主导航'}).getByRole('button',{name:label,exact:true}).click(); }
  async function shot(page,name) {
    await page.evaluate(()=>{
      document.querySelector('#ui-fixture-label')?.remove();
      const badge=document.createElement('div');badge.id='ui-fixture-label';badge.textContent='接口桩验收 · 未启动真实游戏 / 未操作真实文件';
      badge.style.cssText='position:fixed;top:8px;left:50%;transform:translateX(-50%);z-index:99999;background:#1b392d;color:#edf2dc;padding:3px 12px;border:1px solid #889e75;border-radius:5px;font:10px sans-serif;pointer-events:none;';
      document.body.append(badge);
    });
    await page.evaluate(()=>document.fonts.ready); await page.screenshot({path:path.join(out,`${name}.png`)});
  }
  try {
    await check('real bridge: browser calls reject UNSUPPORTED, with no fake data',async()=>{
      const context=await browser.newContext(); const page=await context.newPage();
      try {
        await page.goto(base);
        const codes=await page.evaluate(async()=>{const b=await import('/src/lib/bridge.ts'); const calls=[()=>b.getApiInfo(),()=>b.listInstances(),()=>b.getCoreConfig(),()=>b.startLaunch('test'),()=>b.onTask(()=>{}),()=>b.onGameExit(()=>{}),()=>b.onInstanceChanged(()=>{})];return Promise.all(calls.map(async call=>{try{await call();return 'DID_NOT_REJECT';}catch(error){return error.code;}}));});
        assert.deepEqual(codes,Array(7).fill('UNSUPPORTED'));
      } finally {await context.close();}
    });
    await check('real bridge: error detail pipes and contract mergeTask revision / sticky terminal',async()=>{
      const context=await browser.newContext();const page=await context.newPage();
      try {await page.goto(base);const result=await page.evaluate(async task=>{
        const b=await import('/src/lib/bridge.ts'); const error=b.parseApiError('NETWORK|失败|a|b');
        const done=b.mergeTask(new Map(),task); const stale=b.mergeTask(done,{...task,revision:4,state:'running'}); const resurrect=b.mergeTask(done,{...task,revision:6,state:'running'});
        return {code:error.code,detail:error.detail,stale:stale===done,sticky:resurrect===done};
      },launchTask('unit'));assert.deepEqual(result,{code:'NETWORK',detail:'a|b',stale:true,sticky:true});}finally{await context.close();}
    });
    await check('capability query precedes instance reads; listeners precede snapshots; IPC filters are flat',()=>fixture({},async page=>{
      const calls=await page.evaluate(()=>window.__coreTest.calls);
      assert.equal(calls[0].cmd,'get_api_info');
      const taskRead=calls.findIndex(call=>call.cmd==='get_tasks');
      for(const event of ['astral://task','astral://game-exit','astral://instance-changed']) assert(calls.findIndex(call=>call.cmd==='plugin:event|listen'&&call.args.event===event)<taskRead);
      assert(taskRead<calls.findIndex(call=>call.cmd==='get_active_instance'));
      assert(calls.findIndex(call=>call.cmd==='get_active_instance')<calls.findIndex(call=>call.cmd==='get_active_session'));
      assert(!calls.some(call=>call.cmd==='scan_versions'));
      await page.evaluate(async()=>{const b=await import('/src/lib/bridge.ts');await b.getTasks({kind:'launch',instanceId:'inst_pine'});});
      const args=await page.evaluate(()=>window.__coreTest.calls.filter(call=>call.cmd==='get_tasks').at(-1).args);
      assert.deepEqual(args,{kind:'launch',instanceId:'inst_pine',state:undefined});
    }));
    await check('same gameVersion remains two instances; selection awaits returned Config',()=>fixture({holdSelect:true,selectOverride:pine.id},async page=>{
      assert.equal(await page.locator('.world-tile').count(),3);
      await page.locator('.world-tile').filter({hasText:river.name}).click();
      await page.waitForFunction(()=>!!window.__coreTest.releaseSelect);
      assert.equal(await page.locator('.dock-selection h2').innerText(),pine.name);
      await page.evaluate(()=>window.__coreTest.releaseSelect());
      await page.waitForFunction(()=>window.__coreTest.calls.some(call=>call.cmd==='set_active_instance'));
      assert.equal(await page.locator('.dock-selection h2').innerText(),pine.name);
      assert.deepEqual(await page.evaluate(()=>window.__coreTest.calls.find(call=>call.cmd==='set_active_instance').args),{id:river.id});
    }));
    await check('selection failure preserves current instance and reports real error',()=>fixture({failSelect:true},async page=>{
      await page.locator('.world-tile').filter({hasText:river.name}).click();await page.getByRole('alert').waitFor();assert.equal(await page.locator('.dock-selection h2').innerText(),pine.name);
    }));
    await check('launch lifecycle, stage progress, stale revisions, done requires session confirmation, exit',()=>fixture({},async page=>{
      await page.locator('.launch-button').click(); await phase(page,'starting');
      await page.evaluate(()=>{const s=window.__coreTest;s.task(s.lastLaunch,{revision:3,state:'running',stage:'jvm',label:'正在准备 Java 运行环境'});});
      await page.getByText('jvm · 正在准备 Java 运行环境',{exact:true}).waitFor(); await shot(page,'starting');
      await page.evaluate(()=>{const s=window.__coreTest;s.task(s.lastLaunch,{revision:2,state:'running',stage:'old',label:'STALE_FRAME'});});
      assert(!((await page.locator('.session-feedback').innerText()).includes('STALE_FRAME')));
      assert.equal(await page.locator('.launch-button').isDisabled(),true);
      await page.evaluate(()=>{const s=window.__coreTest;s.holdSession=true;s.finishLaunch();});
      await phase(page,'confirming'); assert(!((await page.locator('.session-feedback').innerText()).includes('已由本地核心确认')));
      await page.evaluate(()=>window.__coreTest.releaseSessions()); await phase(page,'running'); await shot(page,'running');
      await page.evaluate(()=>{const s=window.__coreTest; s.task(s.lastLaunch,{revision:6,state:'running',stage:'bad',label:'DO_NOT_RESURRECT'});});
      await phase(page,'running');
      await page.evaluate(()=>{const s=window.__coreTest;s.exit(`sess_${s.lastLaunch}`,0);}); await phase(page,'exited');await shot(page,'exited');
      assert.equal(await page.evaluate(()=>window.__coreTest.calls.filter(call=>call.cmd==='start_launch').length),1);
      assert(!await page.evaluate(()=>window.__coreTest.calls.some(call=>call.cmd==='launch_version')));
    }));
    await check('exit before command response / done frame is buffered and never resurrected by a stale running query',()=>fixture({holdLaunchResponse:true},async page=>{
      await page.locator('.launch-button').click();await page.waitForFunction(()=>!!window.__coreTest.releaseLaunchResponse);
      await page.evaluate(()=>{
        const s=window.__coreTest,t=s.tasks.find(item=>item.taskId===s.lastLaunch),sid=`sess_${s.lastLaunch}`;
        s.emit('astral://game-exit',{sessionId:sid,taskId:t.taskId,instanceId:t.instanceId,exitCode:null,crashed:true,durationMs:1,endedAt:new Date().toISOString()});
        s.finishLaunch();s.releaseLaunchResponse();
      });
      await phase(page,'exited');assert.match(await page.locator('.session-feedback').innerText(),/退出码未知/);
    }));
    await check('foreign instance or stale session exit cannot clear a running session',()=>fixture({tasks:[launchTask('task_existing')],sessions:[existingSession]},async page=>{
      await phase(page,'running');await page.evaluate(()=>{const s=window.__coreTest;for(const [instanceId,sessionId] of [['inst_river','sess_existing'],['inst_pine','sess_old']])s.emit('astral://game-exit',{instanceId,sessionId,taskId:'old',exitCode:0,crashed:false,durationMs:1,endedAt:new Date().toISOString()});});await phase(page,'running');
    }));
    await check('refresh recovers running session without trusting localStorage PID',()=>fixture({tasks:[launchTask('task_existing')],sessions:[existingSession]},async page=>{
      await phase(page,'running');await page.evaluate(()=>localStorage.setItem('astral.pid','99999'));await page.reload();await phase(page,'running');
      const calls=await page.evaluate(()=>window.__coreTest.calls);assert(calls.findIndex(call=>call.cmd==='plugin:event|listen'&&call.args.event==='astral://game-exit')<calls.findIndex(call=>call.cmd==='get_tasks'));
    }));
    await check('null active session uses listSessions for a real exit record; no record remains idle',()=>fixture({sessions:[{...existingSession,state:'exited',exitCode:null,endedAt:stamp,durationMs:1}]},async page=>{
      await phase(page,'exited');assert.match(await page.locator('.session-feedback').innerText(),/退出码未知/);
      assert(await page.evaluate(()=>window.__coreTest.calls.some(call=>call.cmd==='list_sessions'&&call.args.instanceId==='inst_pine'&&call.args.limit===1)));
    }));
    await check('session read failure produces unknown, not PID-based running',()=>fixture({failSession:true,tasks:[launchTask('task_existing')],sessions:[existingSession]},async page=>{await phase(page,'unknown');assert.equal(await page.locator('.launch-button').isDisabled(),true);}));
    await check('done with no confirmed session stays unknown, not running or newly exited',()=>fixture({tasks:[launchTask('task_existing')]},async page=>{await phase(page,'unknown');assert.equal(await page.locator('.launch-button').isDisabled(),true);}));
    await check('direct exited query restores real exit state',()=>fixture({returnExited:true,sessions:[{...existingSession,state:'exited',exitCode:1,endedAt:stamp,durationMs:1}]},async page=>{await phase(page,'exited');assert.match(await page.locator('.session-feedback').innerText(),/退出码 1/);}));
    await check('late running query cannot resurrect a session after its matching exit',()=>fixture({tasks:[launchTask('task_existing')],sessions:[existingSession]},async page=>{
      await page.evaluate(()=>{window.__coreTest.holdSession=true;});await page.getByRole('button',{name:'刷新实例与会话',exact:true}).click();await page.waitForFunction(()=>window.__coreTest.pendingSessions.length>0);
      await page.evaluate(()=>{const s=window.__coreTest;s.exit('sess_existing',0);s.releaseSessions();});await phase(page,'exited');
    }));
    await check('old session exit cannot clear a newer launch of the same instance',()=>fixture({tasks:[launchTask('task_existing')],sessions:[{...existingSession,state:'exited',exitCode:0,endedAt:stamp,durationMs:1}]},async page=>{
      await phase(page,'exited');await page.locator('.launch-button').click();await page.waitForFunction(()=>!!window.__coreTest.lastLaunch);await page.evaluate(()=>window.__coreTest.finishLaunch());await phase(page,'running');await page.evaluate(()=>window.__coreTest.exit('sess_existing',0));await phase(page,'running');
    }));
    await check('launch failure remains retryable without fake success',()=>fixture({rejectLaunch:true},async page=>{await page.locator('.launch-button').click();await page.getByRole('alert').waitFor();assert.match(await page.getByRole('alert').innerText(),/Java unavailable/);assert.equal(await page.locator('.launch-button').isEnabled(),true);}));
    await check('empty instance view is honest and has no synthetic current selection',()=>fixture({instances:[],activeId:null},async page=>{assert.equal(await page.locator('.world-tile').count(),0);assert.equal(await page.locator('.launch-button').isDisabled(),true);await shot(page,'empty-instances');}));
    await check('compatibility card is explicitly shared and has no edit/delete actions',()=>fixture({instances:[legacy],activeId:legacy.id},async page=>{await nav(page,'版本管理');const card=page.locator('[data-instance-id]');await card.waitFor();assert.match(await card.innerText(),/兼容版本 · 共享游戏目录/);assert.equal(await card.getByRole('button',{name:'编辑',exact:true}).count(),0);assert.equal(await card.getByRole('button',{name:'删除',exact:true}).count(),0);await shot(page,'compatibility-instance');}));
    await check('incomplete files show first reason, expandable full reasons, and download guidance',()=>fixture({instances:[instance(pine.id,pine.name,true,{ok:false,reasons:['缺少客户端 1.21.1.jar','缺少 assets 资源目录']})]},async page=>{assert.match(await page.locator('.instance-health').innerText(),/缺少客户端/);await page.getByText('查看原因',{exact:true}).click();assert.match(await page.locator('.instance-health').innerText(),/缺少 assets/);await shot(page,'incomplete-files');await page.locator('.launch-button').click();assert.equal(await page.locator('h1:visible').innerText(),'下载中心');assert(!await page.evaluate(()=>window.__coreTest.calls.some(call=>call.cmd==='start_launch')));}));
    await check('edit only submits changed name and instance-changed refreshes cards',()=>fixture({},async page=>{
      await nav(page,'版本管理');const card=page.locator(`[data-instance-id="${pine.id}"]`);await card.getByRole('button',{name:'编辑',exact:true}).click();const modal=page.getByRole('dialog');await modal.getByLabel('实例名称',{exact:true}).fill('新的松林');await modal.getByRole('button',{name:'保存名称',exact:true}).click();await card.getByRole('heading',{name:'新的松林',exact:true}).waitFor();assert.deepEqual(await page.evaluate(()=>window.__coreTest.calls.find(call=>call.cmd==='update_instance').args),{id:pine.id,patch:{name:'新的松林'}});
      await page.evaluate(()=>{const s=window.__coreTest;s.instances[1].name='外部改名';s.emit('astral://instance-changed',{instanceId:s.instances[1].id,change:'updated'});});await page.getByRole('heading',{name:'外部改名',exact:true}).waitFor();
    }));
    await check('failed edit retains user input and original card name',()=>fixture({failEdit:true},async page=>{
      await nav(page,'版本管理');const card=page.locator(`[data-instance-id="${pine.id}"]`);await card.getByRole('button',{name:'编辑',exact:true}).click();const modal=page.getByRole('dialog');await modal.getByLabel('实例名称',{exact:true}).fill('未保存的名字');await modal.getByRole('button',{name:'保存名称',exact:true}).click();await modal.getByRole('alert').waitFor();assert.equal(await modal.getByLabel('实例名称',{exact:true}).inputValue(),'未保存的名字');assert.equal(await card.getByRole('heading',{name:pine.name,exact:true}).count(),1);
    }));
    await check('busy delete preserves card; confirmation required; successful current delete clears active Config',()=>fixture({busyDelete:true},async page=>{
      await nav(page,'版本管理');const card=page.locator(`[data-instance-id="${pine.id}"]`);await card.getByRole('button',{name:'删除',exact:true}).click();const modal=page.getByRole('dialog');assert.equal(await modal.getByRole('button',{name:'确认移入回收站',exact:true}).isDisabled(),true);assert(!await page.evaluate(()=>window.__coreTest.calls.some(call=>call.cmd==='delete_instance')));
      await modal.getByRole('checkbox').check();await modal.getByRole('button',{name:'确认移入回收站',exact:true}).click();await modal.getByRole('alert').waitFor();assert.match(await modal.getByRole('alert').innerText(),/退出游戏或等待任务/);assert.equal(await card.count(),1);
      await page.evaluate(()=>{window.__coreTest.busyDelete=false;});await modal.getByRole('button',{name:'确认移入回收站',exact:true}).click();await card.waitFor({state:'detached'});await nav(page,'首页');assert.equal(await page.locator('.launch-button').isDisabled(),true);assert.equal(await page.evaluate(()=>window.__coreTest.config.activeInstanceId),null);assert.deepEqual(await page.evaluate(()=>window.__coreTest.calls.filter(call=>call.cmd==='delete_instance').at(-1).args),{id:pine.id,opts:{trash:true}});
    }));
    await check('new instance waits for Task done, receives event progress, persists across navigation, and re-lists',()=>fixture({},async page=>{
      await nav(page,'版本管理');await page.getByRole('button',{name:'新建实例',exact:true}).click();const modal=page.getByRole('dialog');await modal.getByLabel('实例名称',{exact:true}).fill('雾中小屋');await modal.getByLabel('游戏版本 ID',{exact:true}).fill('1.21.1');await modal.getByRole('button',{name:'创建实例',exact:true}).click();await page.waitForFunction(()=>!!window.__coreTest.lastCreate);assert.equal(await modal.getByText('实例已创建',{exact:true}).count(),0);
      await page.evaluate(()=>{const s=window.__coreTest;s.task(s.lastCreate,{revision:2,state:'running',stage:'create',label:'正在创建独立目录'});});await modal.getByText('create · 正在创建独立目录',{exact:true}).waitFor();await page.keyboard.press('Escape');await nav(page,'首页');await nav(page,'版本管理');await page.getByText('create · 正在创建独立目录',{exact:true}).waitFor();
      await page.evaluate(newInstance=>{const s=window.__coreTest;s.instances.push(newInstance);s.emit('astral://instance-changed',{instanceId:newInstance.id,change:'created'});s.task(s.lastCreate,{revision:3,state:'done',stage:'done',label:'实例目录已创建',endedAt:new Date().toISOString(),result:{kind:'create',instance:newInstance}});},instance('inst_new','雾中小屋'));
      await page.getByRole('heading',{name:'雾中小屋',exact:true}).waitFor();assert.deepEqual(await page.evaluate(()=>window.__coreTest.createRequest),{name:'雾中小屋',gameVersion:'1.21.1'});
    }));
    await check('missing capabilities fall back quietly and never invoke v3 instance commands',()=>fixture({features:['tasks','sessions']},async page=>{await nav(page,'版本管理');assert.equal(await page.getByRole('button',{name:'新建实例',exact:true}).count(),0);assert(!await page.evaluate(()=>window.__coreTest.calls.some(call=>call.cmd==='list_instances')));}));
    await check('instances without sessions/tasks remain readable, but launch and creation are unavailable',()=>fixture({features:['instances']},async page=>{assert.equal(await page.locator('.launch-button').isDisabled(),true);await nav(page,'版本管理');assert.equal(await page.getByRole('button',{name:'新建实例',exact:true}).count(),0);assert(!await page.evaluate(()=>window.__coreTest.calls.some(call=>call.cmd==='get_tasks'||call.cmd==='get_active_session')));}));
    await check('old core without get_api_info falls back to legacy',()=>fixture({missingInfo:true},async page=>{assert.equal(await page.locator('.launch-button').isEnabled(),true);assert.equal(await page.getByRole('alert').count(),0);}));
    await check('capability transport failure is visible and not mistaken for old core',()=>fixture({failInfo:true},async page=>{assert.match(await page.getByRole('alert').innerText(),/capability read failed/);assert(!await page.evaluate(()=>window.__coreTest.calls.some(call=>call.cmd==='scan_versions')));}));
    await check('instance read failure is an error rather than a fabricated empty list',()=>fixture({failList:true},async page=>{assert.match(await page.locator('.first-world').innerText(),/不能确认本地实例数量/);assert(!((await page.locator('.first-world').innerText()).includes('暂未发现本地实例')));assert.equal(await page.locator('.launch-button').isDisabled(),true);}));
    await check('failed listener setup recovers by retrying subscriptions before queries',()=>fixture({failListen:true},async page=>{await page.evaluate(()=>{window.__coreTest.failListen=false;});await page.getByRole('button',{name:'重新读取',exact:true}).click();await page.waitForFunction(()=>document.querySelector('.launch-button')?.disabled===false);await phase(page,'idle');}));
    await check('unselected nonempty list stays unselected until confirmed by core',()=>fixture({activeId:null},async page=>{assert.equal(await page.locator('.launch-button').isDisabled(),true);assert.equal(await page.locator('.world-tile[aria-pressed="true"]').count(),0);assert(!await page.evaluate(()=>window.__coreTest.calls.some(call=>call.cmd==='set_active_instance')));}));
    await check('new core views fit dark/light and desktop/narrow widths',()=>fixture({},async page=>{
      for(const theme of ['dark','light']) {await page.evaluate(theme=>{document.documentElement.dataset.theme=theme;},theme);for(const label of ['首页','版本管理']){await nav(page,label);for(const width of [1440,960,390,320]){await page.setViewportSize({width,height:1000});const overflow=await page.evaluate(()=>[...document.querySelectorAll('.content,.forest-home,.dock-main,.version-card,.page')].filter(el=>el.getClientRects().length&&el.scrollWidth>el.clientWidth+2).map(el=>el.className));assert.deepEqual(overflow,[],`${theme}/${label}/${width}`);}}}
    }));
    await check('no uncaught runtime JS errors',async()=>assert.deepEqual(errors,[]));
    const result={browser:await browser.version(),passed:checks.filter(item=>item.status==='passed').length,failed:checks.filter(item=>item.status==='failed').length,boundary:'Real bridge over isolated Playwright Tauri IPC fixtures; no real Rust/game/filesystem operations. Every exported state screenshot is labeled.',checks};
    await fs.writeFile(path.join(out,'test-results.json'),JSON.stringify(result,null,2)+'\n');
    console.log(JSON.stringify({...result,checks:checks.filter(item=>item.status==='failed')},null,2));if(result.failed)process.exitCode=1;
  } finally {await browser.close();}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
