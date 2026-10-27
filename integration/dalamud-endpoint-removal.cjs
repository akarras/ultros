"use strict";
const assert=require("node:assert/strict");
const fs=require("node:fs/promises");
const path=require("node:path");
const puppeteer=require("puppeteer");
const {clientFetch:fetch,enableLabs}=require("./dalamud-client.cjs");
const pairingRequest=require("./dalamud-pairing-request.cjs");
const base=process.env.BASE_URL||"http://127.0.0.1:8080";
const origin=new URL(base).origin;
assert(["localhost","127.0.0.1","[::1]"].includes(new URL(base).hostname),"use a disposable loopback test server");
async function api(method,route,body,token,headers={}) {
  const response=route==="/api/v1/plugin/pairings"
    ?await pairingRequest(new URL(route,base),JSON.stringify(body))
    :await fetch(new URL(route,base),{method,headers:{"Content-Type":"application/json",...(token?{Authorization:"Bearer "+token}:{}),...headers},body:body===undefined?undefined:JSON.stringify(body),redirect:"manual"});
  let data;try{data=await response.json();}catch{}
  return {status:response.status,data};
}
async function main() {
  const browser=await puppeteer.launch({headless:true,args:["--no-sandbox","--disable-setuid-sandbox"]});
  const credentials=[];
  try {
    const page=await browser.newPage();await enableLabs(page,base);
    page.setDefaultTimeout(45000);page.setDefaultNavigationTimeout(120000);
    await page.setViewport({width:1280,height:1000});
    const errors=[];page.on("pageerror",error=>errors.push(error.message));
    await page.setRequestInterception(true);
    page.on("request",request=>{
      const url=new URL(request.url());
      if(["http:","https:"].includes(url.protocol)&&!["localhost","127.0.0.1","[::1]"].includes(url.hostname))request.abort();else request.continue();
    });
    const user=(901000000000000000n+BigInt(Date.now()%1000000)).toString();
    await page.goto(new URL(`/test/login?user_id=${user}&username=EndpointFixture&redirect=/plugin/verify`,base).href,{waitUntil:"networkidle0"});
    const cookies=(await page.cookies(base)).map(c=>c.name+"="+c.value).join("; ");
    const headers={Cookie:cookies,Origin:origin,"X-Ultros-Capabilities":"in-game-endpoints-v1"};
    async function connect() {
      const pairing=await api("POST","/api/v1/plugin/pairings",{label:"Repeated authorization",scopes:["notifications:read","notifications:ack","notifications:mark-read"]});
      assert.equal(pairing.status,200);
      await page.goto(pairing.data.verification_uri_complete,{waitUntil:"domcontentloaded"});
      await page.waitForSelector('[data-testid="plugin-review"]');
      await page.click('[data-testid="plugin-approve"]');
      await page.waitForSelector('[data-testid="plugin-decision"]');
      const result=await api("POST","/api/v1/plugin/pairings/poll",{device_code:pairing.data.device_code});
      assert.equal(result.status,200);credentials.push(result.data.refresh_token);
      const session=await api("GET","/api/v1/plugin/session",undefined,result.data.access_token);
      assert.equal(session.status,200);return {...result.data,...session.data};
    }
    const stale=await connect();const active=await connect();
    assert.equal((await api("POST","/api/v1/plugin/session/revoke",{refresh_token:stale.refresh_token})).status,204);
    const listed=await api("GET","/api/v1/endpoints",undefined,undefined,headers);
    assert.equal(listed.status,200);
    const inbox=listed.data.find(e=>e.method==="InApp");assert(inbox);
    assert.equal((await api("DELETE",`/api/v1/endpoints/${inbox.id}`,undefined,undefined,headers)).status,400,"the persistent inbox cannot be deleted");
    assert.equal((await api("DELETE",`/api/v1/endpoints/${active.endpoint_id}`,undefined,undefined,{Cookie:cookies})).status,403,"destination removal requires the browser Origin");
    const foreign=await browser.createBrowserContext();
    try {
      const other=await foreign.newPage();
      await other.goto(new URL(`/test/login?user_id=${BigInt(user)+1n}&username=OtherEndpointFixture&redirect=/`,base).href,{waitUntil:"domcontentloaded"});
      const otherCookies=(await other.cookies(base)).map(c=>c.name+"="+c.value).join("; ");
      assert.notEqual((await api("DELETE",`/api/v1/endpoints/${active.endpoint_id}`,undefined,undefined,{Cookie:otherCookies,Origin:origin})).status,200,"another account cannot remove the client");
    }finally{await foreign.close();}
    assert.equal((await api("GET","/api/v1/plugin/session",undefined,active.access_token)).status,200);
    await page.goto(new URL("/alerts?lang=en",base).href,{waitUntil:"networkidle0"});
    const row=id=>`[data-endpoint-id="${id}"]`;
    await page.waitForSelector(row(stale.endpoint_id));
    assert.match(await page.$eval(row(stale.endpoint_id),e=>e.innerText),/disconnected/);
    assert(!(await page.$eval(row(stale.endpoint_id),e=>e.innerText)).includes("Test"),"a revoked client cannot be re-enabled with Test");
    const artifact=path.join(__dirname,"artifacts","dalamud");await fs.mkdir(artifact,{recursive:true});
    for(const width of [1280,390]) {
      await page.setViewport({width,height:1000});
      await page.screenshot({path:path.join(artifact,`endpoint-removal-${width}.png`),fullPage:true});
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth+1),"endpoint rows overflow on mobile");
    }
    // The first click is reviewable and cancellation sends no delete request.
    await page.click(`${row(active.endpoint_id)} [data-action="remove-endpoint"]`);
    await page.waitForSelector(`${row(active.endpoint_id)} [role=group]`);
    await page.click(`${row(active.endpoint_id)} [role=group] button:last-child`);
    assert.equal((await api("GET","/api/v1/plugin/session",undefined,active.access_token)).status,200);
    await page.click(`${row(stale.endpoint_id)} [data-action="remove-endpoint"]`);
    await page.click(`${row(stale.endpoint_id)} [role=group] button:first-child`);
    await page.waitForSelector(row(stale.endpoint_id),{hidden:true});
    assert(await page.$(row(active.endpoint_id)),"removing a duplicate label removed the wrong client");
    assert.equal((await api("POST","/api/v1/plugin/notifications/test",{operation_id:crypto.randomUUID().replaceAll("-","")},active.access_token)).status,202);
    await page.click(`${row(active.endpoint_id)} [data-action="remove-endpoint"]`);
    await page.click(`${row(active.endpoint_id)} [role=group] button:first-child`);
    await page.waitForSelector(row(active.endpoint_id),{hidden:true});
    assert.equal((await api("GET","/api/v1/plugin/session",undefined,active.access_token)).status,401,"removal must revoke the active grant");
    assert.equal((await api("POST","/api/v1/plugin/session/refresh",{refresh_token:active.refresh_token})).status,401,"removal must revoke refresh credentials");
    await page.reload({waitUntil:"networkidle0"});
    const remaining=await api("GET","/api/v1/endpoints",undefined,undefined,headers);
    assert(!remaining.data.some(e=>e.id===stale.endpoint_id||e.id===active.endpoint_id),"removed clients reappeared after reload");
    assert(remaining.data.some(e=>e.id===inbox.id),"cleanup deleted the account inbox");
    assert.deepEqual(errors,[],"the endpoints view must hydrate without browser exceptions");
    console.log("PASS: stale and active game-client removal, duplicate labels, confirmation/cancel, mobile layout, ownership/Origin/inbox protection and token revocation.");
  }finally{
    for(const refresh_token of credentials)await api("POST","/api/v1/plugin/session/revoke",{refresh_token});
    await browser.close();
  }
}
main().catch(error=>{console.error(error.stack);process.exitCode=1;});
