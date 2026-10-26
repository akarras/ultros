"use strict";
const {clientFetch:fetch,enableLabs}=require("./dalamud-client.cjs");
const assert=require("node:assert/strict");
const {createHash}=require("node:crypto");
const fs=require("node:fs/promises");
const path=require("node:path");
const puppeteer=require("puppeteer");
const pairingRequest=require("./dalamud-pairing-request.cjs");
const base=process.env.BASE_URL||"http://localhost:8080";
const origin=new URL(base).origin;
let stage="initialization";
assert(["localhost","127.0.0.1","[::1]"].includes(new URL(base).hostname),"use a disposable loopback test server");
async function api(route,body,token){
  const response=route==="/api/v1/plugin/pairings"?await pairingRequest(new URL(route,base),JSON.stringify(body)):await fetch(new URL(route,base),{method:body===undefined?"GET":"POST",redirect:"manual",headers:{"Content-Type":"application/json",...(token?{Authorization:"Bearer "+token}:{})},body:body===undefined?undefined:JSON.stringify(body)});
  let data;try{data=await response.json();}catch{}
  return {status:response.status,data};
}
function linkToken(pairing){
  const url=new URL(pairing.verification_uri_complete);
  assert.equal(url.origin,origin);assert.equal(url.pathname,"/plugin/verify");assert.equal(url.search,"");
  assert.match(url.hash,/^#pair=[A-Za-z0-9_-]{43}$/);
  const token=url.hash.slice(6);assert(token!==pairing.device_code);
  const hash=createHash("sha256").update("ultros-plugin-link-code-v1/").update(token).digest();
  let bits=BigInt("0x"+hash.subarray(0,5).toString("hex"));
  let code="";for(let index=0;index<8;index++)code+="0123456789ABCDEFGHJKMNPQRSTVWXYZ"[Number((bits>>BigInt((7-index)*5))&31n)];
  assert.equal(pairing.user_code.replaceAll("-",""),code,"the phone code must fingerprint the link token");
  return token;
}
async function main(){
  const browser=await puppeteer.launch({headless:true,args:["--no-sandbox","--disable-setuid-sandbox"]});
  const credentials=[];
  let page;
  try {
    page=await browser.newPage();await page.setViewport({width:1440,height:1000});page.setDefaultTimeout(45000);page.setDefaultNavigationTimeout(120000);
    await enableLabs(page,base);
    async function click(selector){
      const element=await page.waitForSelector(selector,{visible:true});
      await element.evaluate(element=>element.scrollIntoView({block:"center",inline:"nearest"}));await element.dispose();
      await page.locator(selector).filter(element=>{const bounds=element.getBoundingClientRect();const hit=document.elementFromPoint(bounds.left+bounds.width/2,bounds.top+bounds.height/2);return hit!==null&&element.contains(hit);}).click();
    }
    const errors=[];const requestTargets=[];
    page.on("pageerror",error=>errors.push(error.message));
    await page.setRequestInterception(true);
    page.on("request",request=>{const url=new URL(request.url());requestTargets.push(url.pathname+url.search);if(["http:","https:"].includes(url.protocol)&&!["localhost","127.0.0.1","[::1]"].includes(url.hostname))request.abort();else request.continue();});
    const first=await api("/api/v1/plugin/pairings",{label:"Direct approval link",scopes:["lists:read"]});assert.equal(first.status,200);
    const token=linkToken(first.data);
    for(const forged of [undefined,"https://attacker.invalid"]){
      const response=await fetch(new URL("/api/v1/plugin/browser/link",base),{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded",...(forged?{Origin:forged}:{})},body:new URLSearchParams({token})});
      assert.equal(response.status,403,"link capture requires the exact website Origin");
    }
    assert.equal((await api("/api/v1/plugin/session",undefined,token)).status,401);
    assert.equal((await api("/api/v1/plugin/pairings/poll",{device_code:token})).status,401,"a browser link must not redeem game credentials");
    await page.goto(first.data.verification_uri_complete,{waitUntil:"domcontentloaded"});
    await page.waitForSelector('[data-testid="plugin-link-ready"]');
    assert.equal(new URL(page.url()).hash,"");
    const cookie=(await page.cookies(base)).find(cookie=>cookie.name==="ultros_game_link");
    assert(cookie&&cookie.httpOnly&&cookie.sameSite==="Lax"&&cookie.value!==token);
    assert(cookie.expires<=Date.now()/1000+305,"the pending link must expire within five minutes");
    assert(!(await page.evaluate(()=>document.cookie)).includes("ultros_game_link"));
    const cookies=(await page.cookies(base)).map(cookie=>cookie.name+"="+cookie.value).join("; ");
    const login=await fetch(new URL("/login?next=/plugin/verify",base),{redirect:"manual",headers:{Cookie:cookies}});
    assert.equal(login.status,303);assert(!login.headers.get("location").includes(token));
    const user=(900000000000000000n+BigInt(Date.now()%1000000)).toString();
    await page.goto(new URL("/test/login?user_id="+user+"&username=DirectLinkFixture&redirect=/plugin/verify",base).href,{waitUntil:"domcontentloaded"});
    await page.waitForSelector('[data-testid="plugin-review"]');
    assert.equal(await page.$('input[name="user_code"]'),null,"a direct link must skip code entry after login");
    const review=await page.$eval('[data-testid="plugin-review"]',element=>element.innerText);
    assert(review.includes("Direct approval link")&&review.includes(first.data.user_code.replaceAll("-","")));
    const stored=await page.evaluate(()=>JSON.stringify({local:Object.entries(localStorage),session:Object.entries(sessionStorage)}));
    assert(!stored.includes(token)&&!stored.includes(first.data.device_code));
    const history=await (await page.createCDPSession()).send("Page.getNavigationHistory");
    assert(history.entries.every(entry=>!entry.url.includes(token)),"the browser history must contain the cleaned URL");
    const artifact=path.join(__dirname,"artifacts","dalamud");await fs.mkdir(artifact,{recursive:true});
    await page.screenshot({path:path.join(artifact,"direct-link-consent.png"),fullPage:true});
    stage="approve after sign-in";
    await click('[data-testid="plugin-approve"]');await page.waitForSelector('[data-testid="plugin-decision"]');
    const tokens=await api("/api/v1/plugin/pairings/poll",{device_code:first.data.device_code});assert.equal(tokens.status,200);credentials.push(tokens.data.refresh_token);
    stage="consumed-link replay";
    await page.goto(first.data.verification_uri_complete,{waitUntil:"domcontentloaded"});
    await page.waitForFunction(()=>document.querySelector('[role="alert"]')?.textContent.includes("invalid or has expired"));
    assert.equal(await page.$('[data-testid="plugin-review"]'),null);assert.equal(new URL(page.url()).hash,"");
    const second=await api("/api/v1/plugin/pairings",{label:"Existing browser tab",scopes:["lists:read"]});assert.equal(second.status,200);
    const secondToken=linkToken(second.data);
    // Same-document hash navigation must also capture, clean and open review.
    const current=new URL(page.url());current.hash="pair="+secondToken;
    stage="existing-tab link";
    await page.goto(current.href,{waitUntil:"domcontentloaded"});await page.waitForSelector('[data-testid="plugin-review"]');
    assert.equal(new URL(page.url()).hash,"");
    assert((await page.$eval('[data-testid="plugin-review"]',element=>element.innerText)).includes("Existing browser tab"));
    stage="decline";
    await click('[data-testid="plugin-deny"]');await page.waitForSelector('[data-testid="plugin-decision"]');
    assert.equal((await api("/api/v1/plugin/pairings/poll",{device_code:second.data.device_code})).status,403);
    assert(requestTargets.every(target=>!target.includes(token)&&!target.includes(secondToken)),"link credentials must never enter HTTP request URLs");
    await page.setViewport({width:390,height:844,isMobile:true,hasTouch:true});
    const manual=await api("/api/v1/plugin/pairings",{label:"Phone code fallback",scopes:["lists:read"]});assert.equal(manual.status,200);linkToken(manual.data);
    stage="phone-code fallback";
    await page.goto(manual.data.verification_uri,{waitUntil:"domcontentloaded"});
    await page.waitForSelector('input[name="user_code"]');
    await page.locator('input[name="user_code"]').fill(manual.data.user_code);
    await click('[data-testid="plugin-select"]');await page.waitForSelector('[data-testid="plugin-review"]');
    assert((await page.$eval('[data-testid="plugin-review"]',element=>element.innerText)).includes("Phone code fallback"));
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),"the phone consent page must not overflow horizontally");
    await page.screenshot({path:path.join(artifact,"direct-link-phone-consent.png"),fullPage:true});
    await click('[data-testid="plugin-approve"]');await page.waitForSelector('[data-testid="plugin-decision"]');
    const phoneTokens=await api("/api/v1/plugin/pairings/poll",{device_code:manual.data.device_code});assert.equal(phoneTokens.status,200);credentials.push(phoneTokens.data.refresh_token);
    assert.deepEqual(errors,[],"the direct-link/login/replay paths must hydrate without exceptions");
    console.log("PASS: dedicated 256-bit link token, short hash fingerprint, fragment/history cleanup, HttpOnly pending cookie across login, automatic review, signed-in/hash navigation, phone-code fallback, consent/deny, replay rejection and polling-token separation.");
  } catch(error) {
    if(page){await page.screenshot({path:path.join(__dirname,"artifacts","dalamud","direct-link-failure.png"),fullPage:true});console.error("Visible alerts: "+JSON.stringify(await page.$$eval('[role="alert"]',elements=>elements.map(element=>element.textContent))));}
    throw error;
  } finally {
    for(const credential of credentials)await api("/api/v1/plugin/session/revoke",{refresh_token:credential});
    await browser.close();
  }
}
main().catch(error=>{console.error(stage+": "+error.name+": "+error.message);process.exitCode=1;});
