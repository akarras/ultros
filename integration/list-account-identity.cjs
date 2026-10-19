const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
// Requires an isolated local server built with test-auth.
// A real Discord snowflake exceeds JS's exact integer range. Keep the fixture
// ID as text and inspect raw WebSocket frames so the test itself cannot round it.
const base = process.env.BASE_URL || 'http://127.0.0.1:8080';
const userId = '9007199254740993';
async function main() {
  assert(['127.0.0.1','localhost'].includes(new URL(base).hostname));
  const browser = await puppeteer.launch({headless:true,args:['--no-sandbox']});
  const page = await browser.newPage();
  let listId;
  let fixtureUser = false;
  const sent=[],received=[],errors=[];
  const cdp=await page.createCDPSession();
  await cdp.send('Network.enable');
  cdp.on('Network.webSocketFrameSent',event=>{if(event.response.payloadData.includes('SubscribeListDoc'))sent.push(event.response.payloadData);});
  cdp.on('Network.webSocketFrameReceived',event=>{if(event.response.payloadData.includes('ListDocSubscribed'))received.push(event.response.payloadData);});
  page.on('pageerror',error=>errors.push(error.message));
  await page.setCookie({name:'HIDE_ADS',value:'true',url:base});
  const api=(method,route,body)=>page.evaluate(async({method,route,body})=>{
    const response=await fetch(route,{method,headers:body===undefined?{}:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});
    const text=await response.text();
    return {status:response.status,body:text?JSON.parse(text):null};
  },{method,route,body});
  try {
    const login = await page.goto(`${base}/test/login?user_id=${userId}&username=SnowflakeIdentityQA&redirect=/list`,{waitUntil:'domcontentloaded'});
    assert(login?.ok(),'test-auth login succeeds');
    fixtureUser = true;
    assert.equal((await api('GET','/api/v1/current_user')).body.id,userId);
    assert.equal(await page.evaluate(()=>window.__ULTROS_BOOTSTRAP__.current_user.id),userId);
    const name=`Large account ID QA ${Date.now()}`;
    assert.equal((await api('POST','/api/v1/list/create',{name,wdr_filter:{World:63}})).status,200);
    const lists=await api('GET','/api/v1/list');
    listId=lists.body.find(entry=>entry.list.name===name)?.list.id;
    assert(listId);
    assert.equal((await api('POST',`/api/v1/list/${listId}/add/item`,{id:0,list_id:listId,item_id:5364,hq:false,quantity:2,acquired:0,target_price:null})).status,200);
    await page.goto(`${base}/list/${listId}?lang=en`,{waitUntil:'domcontentloaded'});
    await page.waitForFunction(()=>document.querySelector('[data-testid="realtime-status-indicator"]')?.dataset.status==='live',{timeout:30000});
    assert(sent.some(frame=>frame.includes(`"expected_user_id":${userId}`)),'browser sends the exact account guard without JS-number rounding');
    assert(received.length,'server accepts the document subscription');
    assert.equal((await api('GET',`/api/v1/list/${listId}/listings?viewer=${userId}`)).status,200);
    assert.deepEqual(errors,[]);
    console.log('PASS: 9007199254740993 stays exact through bootstrap, current-user JSON, account-bound REST and the accepted browser document WebSocket handshake.');
  } finally {
    try {
      if(listId) assert.equal((await api('DELETE',`/api/v1/list/${listId}/delete`)).status,200,'delete owned fixture list');
      if(fixtureUser) assert.equal((await api('DELETE','/api/v1/current_user')).status,200,'delete owned fixture account');
    } finally {
      await browser.close();
    }
  }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
