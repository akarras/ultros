// #1500: one workspace and durable Make online, against a fresh test-auth build.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const base = new URL(process.env.BASE_URL || 'http://127.0.0.1:8080').origin;
const tid = id => `[data-testid="${id}"]`;
const needed = 'input[aria-label="Needed for Bronze Ingot"]';
const owned = 'input[aria-label="Owned for Bronze Ingot"]';
const owner = 990000001501, other = 990000001502;

async function main() {
  const browser = await puppeteer.launch({headless:true,args:['--disable-background-timer-throttling','--disable-renderer-backgrounding','--disable-backgrounding-occluded-windows']});
  const page = await browser.newPage();
  const created = new Set();
  const errors=[];
  let mode='normal', held=[], heldLogin=[], holdFinalLogin=false, promotionResponded=false, failedUploads=0;
  let failInvite=false, inviteRequests=0;
  async function configure(p) {
    p.setDefaultTimeout(60000);
    await p.setViewport({width:1280,height:900});
    p.on('pageerror',e=>errors.push(e.message));
    p.on('dialog',d=>d.type()==='beforeunload'?d.accept():d.dismiss());
    await p.evaluateOnNewDocument(()=>{window.__onlineHydrated=false;window.addEventListener('ultros:hydrated',()=>window.__onlineHydrated=true);});
    await p.setRequestInterception(true);
    p.on('request',async request=>{
      try {
        const url=new URL(request.url());
        if(url.origin===base && /\/api\/v1\/list\/\d+\/invite\/create$/.test(url.pathname) && request.method()==='POST') {
          inviteRequests++;
          if(failInvite) {await request.respond({status:503,contentType:'application/json',body:JSON.stringify('Invite service unavailable')});return;}
        }
        if(url.origin===base && url.pathname==='/api/v1/current_user' && holdFinalLogin && promotionResponded) {
          holdFinalLogin=false;heldLogin.push(request);return;
        }
        if(url.origin===base && url.pathname==='/api/v1/list/online' && request.method()==='POST') {
          if(mode==='drop-once') {
            mode='normal';
            const upstream=await fetch(request.url(),{method:'POST',headers:request.headers(),body:request.postData()});
            assert(upstream.ok,'server commits before the simulated response loss');
            created.add((await upstream.json()).list_id);
            await request.respond({status:503,contentType:'application/json',body:JSON.stringify('Response lost after commit')});return;
          }
          if(mode==='fail') {failedUploads++;await request.respond({status:503,contentType:'application/json',body:JSON.stringify('Offline test')});return;}
          if(mode==='hold') {held.push(request);return;}
        }
        if(/googlesyndication|doubleclick/.test(url.hostname)) await request.abort(); else await request.continue();
      } catch(e) {if(!/handled|closed|destroyed/.test(e.message))errors.push(e.message);}
    });
    p.on('response',async response=>{
      if(new URL(response.url()).pathname==='/api/v1/list/online' && response.ok()) {
        if(holdFinalLogin)promotionResponded=true;
        try{created.add((await response.json()).list_id);}catch{}
      }
    });
  }
  const load=async(p,route)=>{await p.bringToFront();await p.goto(new URL(route,base).href,{waitUntil:'domcontentloaded'});await p.waitForFunction(()=>window.__onlineHydrated);};
  let signedIn=false;
  const login=async(p,id,next='/list?labs=lists-sync')=>{signedIn=true;return load(p,`/test/login?user_id=${id}&username=Online${id}&redirect=${encodeURIComponent(next)}`);};
  const api=async(p,method,route,body)=>p.evaluate(async({method,route,body})=>{
    const r=await fetch(route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});
    const text=await r.text();let data;try{data=JSON.parse(text);}catch{data=text;}return {status:r.status,data};
  },{method,route,body});
  const replace=async(p,selector,value)=>{await p.bringToFront();await p.waitForSelector(selector,{visible:true});await p.focus(selector);await p.keyboard.down('Control');await p.keyboard.press('KeyA');await p.keyboard.up('Control');await p.keyboard.press('Backspace');await p.type(selector,String(value));};
  const waitValue=async(p,selector,value)=>{await p.bringToFront();await p.waitForFunction((selector,value)=>document.querySelector(selector)?.value===String(value),{polling:100},selector,value);};
  const saved=async p=>{await p.bringToFront();await p.waitForFunction(()=>document.querySelector('[data-testid="device-list-status"]')?.textContent==='Saved on this device');};
  const details=async p=>{await p.bringToFront();const sel='button[aria-label="Details for Bronze Ingot"]';await p.waitForSelector(sel);if(await p.$eval(sel,b=>b.getAttribute('aria-expanded'))!=='true')await p.click(sel);await p.waitForSelector(owned,{visible:true});};
  async function createDevice(name, clearSearch=true) {
    await load(page,'/list?labs=lists-sync');
    await page.waitForSelector(tid('list-new'));
    assert.equal(await page.$$eval('h1',nodes=>nodes.filter(n=>n.textContent.trim()==='Lists').length),1);
    assert.equal(await page.$$(tid('list-new')).then(a=>a.length),1);
    assert.equal(await page.$$(tid('device-lists-adoption')).then(a=>a.length),0);
    await page.click(tid('list-new'));await replace(page,tid('device-list-name'),name);
    // Signed-in sessions default to Online; this script exercises the local→online transition itself.
    if(signedIn){await page.waitForSelector(tid('device-list-storage-local'),{visible:true});await page.click(tid('device-list-storage-local'));await page.waitForFunction(sel=>document.querySelector(sel)?.getAttribute('aria-pressed')==='true',{},tid('device-list-storage-local'));}
    await page.click(tid('device-list-create'));
    await page.waitForFunction(()=>location.pathname.startsWith('/list/device/'));
    await replace(page,'input[aria-label="Quantity to add"]',3);
    await replace(page,'input[aria-label="Add an item"]','Bronze Ingot');
    await page.waitForSelector('button[aria-label="Add Bronze Ingot"]');await page.keyboard.press('Enter');if(clearSearch)await page.keyboard.press('Escape');
    await waitValue(page,needed,3);await saved(page);
    return new URL(page.url()).pathname;
  }
  const online=async p=>{await p.bringToFront();await p.waitForFunction(()=>/^\/list\/\d+$/.test(location.pathname));const id=Number(new URL(p.url()).pathname.split('/').at(-1));created.add(id);return id;};
  async function record(devicePath) {
    return page.evaluate(async id=>{
      const db=await new Promise((resolve,reject)=>{const r=indexedDB.open('ultros-device-lists-v1',1);r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);});
      const value=await new Promise((resolve,reject)=>{const r=db.transaction('lists').objectStore('lists').get(id);r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);});db.close();
      return {revision:value.revision,online:value.online,snapshot:[...value.snapshot]};
    },devicePath.split('/').at(-1));
  }
  const artifacts=path.join(__dirname,'artifacts/list-online');fs.mkdirSync(artifacts,{recursive:true});
  try {
    await configure(page);
    await page.setCookie(...[['LABS','lists-sync'],['HIDE_ADS','true'],['PRICE_ZONE','North-America'],['i18n_pref_locale','en']].map(([name,value])=>({name,value,url:base,path:'/'})));
    // Sign-in continuation uses exactly the return target produced by Make online.
    const first=await createDevice(`Online first ${Date.now()}`);
    const signIn=await page.$eval(tid('device-list-make-online-sign-in'),a=>a.href);
    const next=new URL(signIn).searchParams.get('next');
    assert(next.startsWith(first));assert(next.includes('make_online=1'));
    await login(page,owner,next);const firstId=await online(page);await waitValue(page,needed,3);
    const shares=await api(page,'GET',`/api/v1/list/${firstId}/shares`);
    assert.equal(shares.status,200);assert(shares.data.every(entries=>entries.length===0));
    await page.waitForSelector(tid('list-access-btn'));await page.click(tid('list-access-btn'));
    await page.waitForFunction(()=>document.body.textContent.includes('Only you have access until you invite someone.'));
    await page.screenshot({path:path.join(artifacts,'access-desktop.png'),fullPage:true});
    await page.setViewport({width:390,height:844});
    await page.waitForFunction(()=>document.querySelector('.side-nav').getBoundingClientRect().right<=1);
    await page.screenshot({path:path.join(artifacts,'access-mobile.png'),fullPage:true});
    if(!await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1))errors.push('Access dialog overflows mobile viewport');
    const inviteMethods='[role="group"][aria-label="Invite someone"]';
    await page.evaluate(selector=>[...document.querySelectorAll(`${selector} button`)].find(b=>b.textContent==='Discord user').click(),inviteMethods);
    await page.waitForFunction(()=>[...document.querySelectorAll('[role="dialog"] section')].filter(s=>s.checkVisibility()).length===2);
    const pressed=await page.$$eval(`${inviteMethods} button[aria-pressed="true"]`,buttons=>buttons.map(b=>b.textContent));
    if(JSON.stringify(pressed)!==JSON.stringify(['Discord user']))errors.push(`Expected accessible selected Discord user method, received ${JSON.stringify(pressed)}`);
    await page.evaluate(selector=>[...document.querySelectorAll(`${selector} button`)].find(b=>b.textContent==='Invite link').click(),inviteMethods);
    await page.waitForFunction(()=>document.querySelector('[role="dialog"] select[aria-label="Permission"]')?.checkVisibility());
    const inviteLimit=tid('list-invite-max-uses'), inviteCreate=tid('list-invite-create');
    const initialInvites=inviteRequests;
    for(const invalid of ['0','-1','1.5','abc','2147483648']) {
      await replace(page,inviteLimit,invalid);await page.click(inviteCreate);
      await page.waitForSelector('#list-invite-error',{visible:true});
      assert.equal(await page.$eval(inviteLimit,input=>input.getAttribute('aria-invalid')),'true');
      assert.equal(inviteRequests,initialInvites,'invalid limit cannot create an unlimited invite');
    }
    await page.select(tid('list-invite-permission'),'Write');
    failInvite=true;await replace(page,inviteLimit,2);await page.click(inviteCreate);
    await page.waitForFunction(()=>document.querySelector('#list-invite-error')?.textContent.includes('Try again'));
    assert.equal(await page.$eval(inviteLimit,input=>input.value),'2','failed invite retains the intended limit');
    assert.equal(inviteRequests,initialInvites+1);
    failInvite=false;await page.click(inviteCreate);
    await page.waitForFunction(()=>document.querySelector('[data-testid="list-invite-max-uses"]')?.value==='');
    const newInvites=(await api(page,'GET',`/api/v1/list/${firstId}/invites`)).data;
    assert.equal(newInvites.length,1);assert.equal(newInvites[0].max_uses,2);assert.equal(newInvites[0].permission,'Write');
    await page.waitForFunction(()=>document.querySelector('[data-testid="list-invite-permission"]')?.value==='Write');
    // The action refetch rebuilds this section; displayed permission must still
    // agree with the retained signal, and the next explicit Read grant.
    await page.select(tid('list-invite-permission'),'Read');await replace(page,inviteLimit,1);await page.click(inviteCreate);
    await page.waitForFunction(()=>document.querySelector('[data-testid="list-invite-max-uses"]')?.value==='');
    const allInvites=(await api(page,'GET',`/api/v1/list/${firstId}/invites`)).data;
    assert.equal(allInvites.length,2);assert(allInvites.some(invite=>invite.permission==='Read'&&invite.max_uses===1));
    assert.equal(await page.$eval(tid('list-invite-permission'),select=>select.value),'Read');
    for(const invite of allInvites)await api(page,'DELETE',`/api/v1/invite/${invite.id}`);
    console.log('[ok] invite limits validate, failed input survives, and visible permissions match grants after refetch');
    await page.setViewport({width:1280,height:900});
    await page.click('button[aria-label="Close modal"]');
    await replace(page,needed,5);await page.keyboard.press('Enter');await waitValue(page,needed,5);
    await page.click(tid('list-undo'));await waitValue(page,needed,3);
    await page.click(tid('list-redo'));await waitValue(page,needed,5);
    await page.waitForFunction(()=>document.querySelector('[data-testid="account-list-save-state"]')?.textContent==='Saved on this device');
    await load(page,first+'?labs=lists-sync');assert.equal(await online(page),firstId);await waitValue(page,needed,5);
    await load(page,'/list?labs=lists-sync');await page.waitForSelector(tid('lists-grid'));
    await page.waitForFunction(id=>document.querySelectorAll(`a[href="/list/${id}"]`).length>0,{},firstId);
    assert.equal((await api(page,'GET','/api/v1/list')).data.filter(l=>l.list.id===firstId).length,1);
    await page.screenshot({path:path.join(artifacts,'directory-desktop.png'),fullPage:true});
    await page.setViewport({width:390,height:844});
    await page.waitForFunction(()=>document.querySelector('.side-nav').getBoundingClientRect().right<=1);
    await page.screenshot({path:path.join(artifacts,'directory-mobile.png'),fullPage:true});
    if(!await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1))errors.push('Lists directory overflows mobile viewport');
    await page.setViewport({width:1280,height:900});
    console.log('[ok] one workspace, exact sign-in continuation, private online destination, continued editing and old URL reload');

    // Form failures remain visible inside the dialog, with the user's input.
    await page.click(tid('list-restore-open'));await replace(page,tid('device-list-backup'),'not a list backup');
    await page.click(tid('device-list-restore'));
    await page.waitForSelector('[role="dialog"] [role="alert"]',{visible:true});
    assert.equal(await page.$eval(tid('device-list-backup'),input=>input.value),'not a list backup');
    assert((await page.$eval('[role="dialog"] [role="alert"]',element=>element.textContent)).trim());
    await page.click('button[aria-label="Close modal"]');
    const revoked=(await api(page,'POST',`/api/v1/list/${firstId}/invite/create`,{permission:'Read',max_uses:1})).data;
    assert(revoked.id);assert.equal((await api(page,'DELETE',`/api/v1/invite/${revoked.id}`)).status,200);
    for(const code of ['invalid-online-invite',revoked.id]) {
      await page.click(tid('list-join-open'));await replace(page,tid('list-join-code'),code);await page.click(tid('list-join-submit'));
      await page.waitForSelector('[role="dialog"] [role="alert"]',{visible:true});
      assert.equal(await page.$eval(tid('list-join-code'),input=>input.value),code);
      assert((await page.$eval('[role="dialog"] [role="alert"]',element=>element.textContent)).trim());
      await page.waitForFunction(()=>!document.querySelector('[data-testid="list-join-submit"]')?.disabled);
      await page.click('button[aria-label="Close modal"]');
    }
    console.log('[ok] invalid restore and unavailable invites show visible dialog errors and retain input for retry');

    await createDevice(`Online used composer ${Date.now()}`,false);
    assert.equal(await page.$eval('input[aria-label="Add an item"]',input=>input.value),'Bronze Ingot');
    await page.click(tid('device-list-adopt'));await online(page);await waitValue(page,needed,3);
    console.log('[ok] successful add followed directly by Make online does not mistake retained search text for a draft');

    await createDevice(`Online Shop mode ${Date.now()}`);
    await page.click(tid('guest-shop-mode'));await page.click(tid('device-list-adopt'));await online(page);
    assert.equal(new URL(page.url()).searchParams.get('buy'),'true');
    await page.waitForFunction(()=>document.querySelector('[data-testid="guest-shop-mode"]')?.getAttribute('aria-pressed')==='true');
    console.log('[ok] Make online preserves the selected Shop mode');

    // An unavailable request retains bytes and retry identity.
    const failed=await createDevice(`Online retry ${Date.now()}`);const before=await record(failed);
    mode='fail';await page.click(tid('device-list-adopt'));
    await page.waitForFunction(()=>document.querySelector('[data-testid="list-online-controls"] [role="alert"]')?.textContent.includes('saved on this device'));
    const after=await record(failed);assert.deepEqual(after.snapshot,before.snapshot);assert.equal(after.online.owner,String(owner));
    await load(page,failed+'?labs=lists-sync');await page.waitForSelector(tid('device-list-adopt'));
    assert.deepEqual((await record(failed)).snapshot,before.snapshot,'reload during a failed transition preserves original bytes');
    mode='normal';
    if (new URL(page.url()).pathname.startsWith('/list/device/')) {
      try { await page.click(tid('device-list-adopt')); } catch (error) {
        if (!/Node is detached from document/.test(String(error)) || !/^\/list\/\d+$/.test(new URL(page.url()).pathname)) throw error;
      }
    }
    const retryId=await online(page);
    const retryBinding=await record(failed);
    assert.equal(retryBinding.online.list_id,retryId);
    assert.equal(retryBinding.online.acknowledged,retryBinding.revision);
    await load(page,failed+'?labs=lists-sync');assert.equal(await online(page),retryId);
    console.log('[ok] failed transfer preserves the source and retry continues into one destination');

    const lost=await createDevice(`Online lost response ${Date.now()}`);
    mode='drop-once';await page.click(tid('device-list-adopt'));
    // A queued save may retry automatically. Otherwise the direct action retries.
    await Promise.race([page.waitForFunction(()=>/^\/list\/\d+$/.test(location.pathname)),page.waitForFunction(()=>document.querySelector('[data-testid="list-online-controls"] [role="alert"]'))]);
    if(new URL(page.url()).pathname.startsWith('/list/device/')) {
      await page.waitForFunction(()=>!document.querySelector('[data-testid="device-list-adopt"]')?.disabled);
      await page.click(tid('device-list-adopt'));
    }
    const lostId=await online(page);
    await load(page,lost+'?labs=lists-sync');assert.equal(await online(page),lostId);
    const lostBinding=await record(lost);assert.equal(lostBinding.online.acknowledged,lostBinding.revision);
    console.log('[ok] a lost response after commit retries into the same destination');

    // Both tabs keep their changes while the first requests are in flight.
    const concurrent=await createDevice(`Online concurrent ${Date.now()}`);
    const second=await browser.newPage();await configure(second);await load(second,concurrent+'?labs=lists-sync');await details(second);
    console.log('[progress] both device tabs ready');
    await page.bringToFront();
    mode='hold';await page.click(tid('device-list-adopt'));
    await page.waitForFunction(()=>document.querySelector('[data-testid="device-list-adopt"]')?.disabled);
    await replace(page,needed,7);await page.keyboard.press('Enter');
    await replace(second,owned,2);await second.keyboard.press('Enter');
    console.log('[progress] concurrent edits made while uploads held');
    const deadline=Date.now()+60000;while(held.length<2&&Date.now()<deadline)await new Promise(r=>setTimeout(r,100));
    assert(held.length>=2,'both tabs have a pending request');
    mode='normal';await Promise.all(held.splice(0).map(r=>r.continue()));
    const concurrentId=await online(page);assert.equal(await online(second),concurrentId);
    await waitValue(page,needed,7);await details(page);await waitValue(page,owned,2);
    await second.close();
    const state=await record(concurrent);assert.equal(state.online.acknowledged,state.revision);
    console.log('[ok] concurrent tabs share a destination and preserve edits during transition');

    // Hold the final session check after acknowledgement. An edit made during
    // that await must keep the local editor visible until its own upload lands.
    const lateNavigation=await createDevice(`Online final handoff ${Date.now()}`);
    promotionResponded=false;holdFinalLogin=true;
    await page.click(tid('device-list-adopt'));
    const loginDeadline=Date.now()+60000;while(!heldLogin.length&&Date.now()<loginDeadline)await new Promise(r=>setTimeout(r,50));
    assert.equal(heldLogin.length,1,'final session check is held after initial promotion');
    const acknowledged=await record(lateNavigation);
    assert.equal(acknowledged.online.acknowledged,acknowledged.revision);
    const failedBefore=failedUploads;mode='fail';
    await replace(page,needed,9);await page.keyboard.press('Enter');
    const failDeadline=Date.now()+60000;while(failedUploads===failedBefore&&Date.now()<failDeadline)await new Promise(r=>setTimeout(r,50));
    assert(failedUploads>failedBefore,'later edit upload is deliberately unavailable');
    const latest=await record(lateNavigation);assert(latest.revision>latest.online.acknowledged);
    const loginReturned=page.waitForResponse(r=>new URL(r.url()).pathname==='/api/v1/current_user');
    await Promise.all(heldLogin.splice(0).map(r=>r.continue()));await loginReturned;
    await new Promise(r=>setTimeout(r,250));
    assert.equal(new URL(page.url()).pathname,lateNavigation,'stale acknowledgement cannot hide newer unsynced edits');
    await waitValue(page,needed,9);
    mode='normal';await page.waitForFunction(()=>!document.querySelector('[data-testid="device-list-adopt"]')?.disabled);
    await page.click(tid('device-list-adopt'));await online(page);await waitValue(page,needed,9);
    const settled=await record(lateNavigation);assert.equal(settled.online.acknowledged,settled.revision);
    console.log('[ok] edits during the final session check remain visible until the latest revision is acknowledged');

    for(const resolution of ['commit','discard','catalog','quantity','quality']) {
      const drafted=await createDevice(`Online draft ${resolution} ${Date.now()}`);
      promotionResponded=false;holdFinalLogin=true;await page.click(tid('device-list-adopt'));
      const deadline=Date.now()+60000;while(!heldLogin.length&&Date.now()<deadline)await new Promise(r=>setTimeout(r,50));
      assert.equal(heldLogin.length,1);
      const beforeDraft=await record(drafted);
      const draftSelector=resolution==='catalog'?'input[aria-label="Add an item"]':resolution==='quantity'?'input[aria-label="Quantity to add"]':resolution==='quality'?'select[aria-label="Quality to add"]':needed;
      const draftValue=resolution==='catalog'?'Iron Ingot':resolution==='quality'?'hq':9;
      if(resolution==='quality'){await page.focus(draftSelector);await page.select(draftSelector,draftValue);}
      else await replace(page,draftSelector,draftValue); // Deliberately no Enter or blur.
      assert.equal((await record(drafted)).revision,beforeDraft.revision);
      const returned=page.waitForResponse(r=>new URL(r.url()).pathname==='/api/v1/current_user');
      await Promise.all(heldLogin.splice(0).map(r=>r.continue()));await returned;
      await page.waitForFunction(()=>document.body.textContent.includes('Finish or cancel your edit to continue online.'));
      assert.equal(new URL(page.url()).pathname,drafted);await waitValue(page,draftSelector,draftValue);
      await page.keyboard.press(resolution==='commit'?'Enter':'Escape');
      await online(page);await waitValue(page,needed,resolution==='commit'?9:3);
      const afterDraft=await record(drafted);
      assert.equal(afterDraft.online.acknowledged,afterDraft.revision);
      if(resolution!=='commit')assert.deepEqual(afterDraft.snapshot,beforeDraft.snapshot,'discarded text never changes the document');
    }
    console.log('[ok] uncommitted drafts defer handoff until commit or Escape, preserving native text editing');

    // Legacy adoption remains explicit: no name-based merge or loss of backup.
    const legacy=await createDevice(`Online legacy ${Date.now()}`);const old=await record(legacy);
    const oldId=legacy.split('/').at(-1);
    const legacyRequest={expected_owner:owner,adoption_key:`legacy-${Date.now()}`,device_list_id:oldId,source_revision:String(old.revision),name:`Legacy destination ${Date.now()}`,wdr_filter:{Region:1},items:[{item_id:5056,hq:null,quantity:3,acquired:0,target_price:null}]};
    const receipt=(await api(page,'POST','/api/v1/list/adopt',legacyRequest)).data;created.add(receipt.list_id);
    await page.evaluate(({owner,id,receipt})=>localStorage.setItem(`ultros:device-adoption:v1:${owner}:${id}:receipt`,JSON.stringify(receipt)),{owner,id:oldId,receipt});
    await load(page,legacy+'?labs=lists-sync');await page.waitForSelector(tid('device-list-open-online'));
    promotionResponded=true;holdFinalLogin=true;
    await page.click(tid('device-list-open-online'));
    const legacyDeadline=Date.now()+60000;while(!heldLogin.length&&Date.now()<legacyDeadline)await new Promise(r=>setTimeout(r,50));
    assert.equal(heldLogin.length,1);await replace(page,needed,9);
    await Promise.all(heldLogin.splice(0).map(r=>r.continue()));
    await page.waitForFunction(()=>document.body.textContent.includes('Finish or cancel your edit to continue online.'));
    assert.equal(new URL(page.url()).pathname,legacy);await waitValue(page,needed,9);
    await page.keyboard.press('Escape');
    assert.equal(await online(page),receipt.list_id);assert.deepEqual((await record(legacy)).snapshot,old.snapshot);
    await load(page,legacy+'?labs=lists-sync&recovery=1');await page.waitForSelector(tid('device-list-export'));await page.click(tid('device-list-export'));
    await page.waitForFunction(()=>document.querySelector('[data-testid="device-list-backup"]')?.value.length>0);
    assert.equal(new URL(page.url()).pathname,legacy,'recovery URL stays on original bytes');
    console.log('[ok] legacy copy choice retains an accessible recovery backup');

    const separate=await createDevice(`Online separate ${Date.now()}`);const separateBefore=await record(separate);
    const separateDevice=separate.split('/').at(-1);
    const separateReceipt=(await api(page,'POST','/api/v1/list/adopt',{...legacyRequest,adoption_key:`separate-${Date.now()}`,device_list_id:separateDevice,source_revision:String(separateBefore.revision),name:`Independent legacy ${Date.now()}`})).data;
    created.add(separateReceipt.list_id);
    await page.evaluate(({owner,id,receipt})=>localStorage.setItem(`ultros:device-adoption:v1:${owner}:${id}:receipt`,JSON.stringify(receipt)),{owner,id:separateDevice,receipt:separateReceipt});
    await replace(page,needed,8);await page.keyboard.press('Enter');await saved(page);
    await load(page,separate+'?labs=lists-sync');await page.waitForSelector(tid('device-list-open-online'));
    // The separate upload is a secondary path behind the storage menu.
    await page.click(tid('device-list-storage-toggle'));await page.waitForSelector(tid('device-list-upload-separate'));await page.click(tid('device-list-upload-separate'));
    const separateId=await online(page);assert.notEqual(separateId,separateReceipt.list_id);await waitValue(page,needed,8);
    const untouched=(await api(page,'GET',`/api/v1/list/${separateReceipt.list_id}/listings`)).data;
    assert.equal(untouched[1][0][0].quantity,3,'independently edited legacy destination is not replaced');
    console.log('[ok] keeping a newer version separate preserves the existing independent destination');

    // A session switch during an already dispatched request never redirects the
    // user into the previous account or changes the bound destination account.
    const changing=await createDevice(`Online in-flight account ${Date.now()}`);
    mode='hold';await page.click(tid('device-list-adopt'));
    const holdDeadline=Date.now()+60000;while(!held.length&&Date.now()<holdDeadline)await new Promise(r=>setTimeout(r,100));
    assert(held.length);
    const accountTab=await browser.newPage();await configure(accountTab);await login(accountTab,other);
    mode='normal';await Promise.all(held.splice(0).map(r=>r.continue()));
    await page.waitForFunction(()=>!document.querySelector('[data-testid="device-list-adopt"]')?.disabled,{polling:100});
    assert.equal(new URL(page.url()).pathname,changing);
    assert.equal((await record(changing)).online.owner,String(owner));
    assert.equal((await api(accountTab,'GET','/api/v1/list')).data.length,0);
    await accountTab.close();await login(page,owner,changing+'?labs=lists-sync');await online(page);
    console.log('[ok] account switch during a request preserves the original binding and prevents wrong-account navigation');

    // Switching accounts cannot repurpose an explicitly bound failed transition.
    const switched=await createDevice(`Online account boundary ${Date.now()}`);mode='fail';await page.click(tid('device-list-adopt'));
    await page.waitForFunction(()=>document.querySelector('[data-testid="list-online-controls"] [role="alert"]'));
    mode='normal';await login(page,other,switched+'?labs=lists-sync');await page.waitForSelector(tid('list-online-account-required'));
    assert.equal((await page.$$(needed)).length,0,'a bound device editor is not exposed to another account');
    assert.equal((await record(switched)).online.owner,String(owner));
    assert.equal((await api(page,'GET','/api/v1/list')).data.length,0);
    const denied=await api(page,'GET',`/api/v1/list/${firstId}/listings`);assert([403,404].includes(denied.status));
    await login(page,owner,switched+'?labs=lists-sync');await online(page);
    await api(page,'POST',`/api/v1/list/${firstId}/share/user`,{user_id:other,permission:'Read'});
    await login(page,other,`/list/${firstId}?labs=lists-sync`);
    await page.waitForSelector(tid('list-settings-btn'));
    assert.equal((await page.$$(tid('list-access-btn'))).length,0,'viewers cannot manage access');
    await page.click(tid('list-settings-btn'));await page.waitForSelector(tid('list-settings-drawer'));
    assert.equal((await page.$$(tid('list-settings-sharing'))).length,0);
    assert.equal((await page.$$(tid('list-delete-btn'))).length,0);
    await login(page,owner,`/list/${firstId}?labs=lists-sync`);
    await api(page,'POST',`/api/v1/list/${firstId}/share/user`,{user_id:other,permission:'Write'});
    await login(page,other,`/list/${firstId}?labs=lists-sync`);await page.waitForSelector(needed);
    assert.equal(await page.$eval(needed,input=>input.disabled),false,'editors retain editing capability');
    assert.equal((await page.$$(tid('list-access-btn'))).length,0,'editors cannot grant access');
    await login(page,owner);
    console.log('[ok] access management is owner-only while viewers and editors retain their appropriate controls');
    assert.deepEqual(errors,[]);
    console.log('Make online regression passed.');
  } catch(error) {
    console.error('Scenario failed before cleanup:',error);
    for(const tab of await browser.pages()) {
      if(!tab.url().startsWith(base))continue;
      console.error('Failure page:',tab.url());
      try{console.error(await tab.evaluate(()=>({status:document.querySelector('[data-testid="device-list-status"]')?.textContent,online:document.querySelector('[data-testid="list-online-controls"]')?.textContent,drafts:[...document.querySelectorAll('[data-committed],[data-handoff-committed]')].map(input=>({label:input.getAttribute('aria-label'),value:input.value,committed:input.getAttribute('data-committed'),handoff:input.getAttribute('data-handoff-committed')}))})));}catch{}
    }
    throw error;
  } finally {
    mode='normal';holdFinalLogin=false;for(const request of [...held.splice(0),...heldLogin.splice(0)]){try{await request.abort();}catch{}}
    try{await login(page,owner);for(const id of created)if(Number.isInteger(id))await api(page,'DELETE',`/api/v1/list/${id}/delete`);}catch(e){console.error('Fixture cleanup failed:',e.message);}
    await browser.close();
  }
}
main().catch(e=>{console.error(e);process.exitCode=1;});
