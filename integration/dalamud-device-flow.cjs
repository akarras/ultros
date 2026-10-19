#!/usr/bin/env node
"use strict";
// This harness uses the repository's test-auth workflow. Never point it at production.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const puppeteer = require("puppeteer");
const pairingRequest=require("./dalamud-pairing-request.cjs");
const base = process.env.BASE_URL || "http://localhost:8080";
const origin = new URL(base).origin;
assert(["localhost", "127.0.0.1", "[::1]"].includes(new URL(base).hostname), "use a disposable loopback test server");
const scopes = ["notifications:read","notifications:ack","notifications:mark-read","lists:read","lists:write","retainers:read","retainers:register","retainers:upload"];
async function api(method, route, body, token, extra = {}) {
  const response = route==="/api/v1/plugin/pairings"?await pairingRequest(new URL(route,base),JSON.stringify(body)):await fetch(new URL(route, base), {method, redirect:"manual", headers:{"Content-Type":"application/json",...(token ? {Authorization:"Bearer "+token}:{}),...extra}, body:body===undefined?undefined:JSON.stringify(body)});
  let data;try{data=await response.json();}catch{}
  return {status:response.status,data,headers:response.headers};
}
async function localPage(context) {
  const page=await context.newPage();
  page.setDefaultTimeout(45000);
  page.setDefaultNavigationTimeout(120000);
  await page.evaluateOnNewDocument(()=>{window.__pluginPolicyViolations=[];document.addEventListener("securitypolicyviolation",event=>window.__pluginPolicyViolations.push(event.violatedDirective+": "+event.blockedURI));});
  await page.setRequestInterception(true);
  page.on("request",request=>{
    const url=new URL(request.url());
    if(["http:","https:"].includes(url.protocol)&&!["localhost","127.0.0.1","[::1]"].includes(url.hostname))request.abort();
    else request.continue();
  });
  return page;
}
async function main() {
  assert.equal((await api("GET","/api/v1/plugin/capabilities")).status,200,"enable plugin connections in the local server");
  assert.equal((await api("GET","/api/v1/plugin/notifications/stream")).status,401,"notification streams require a bearer grant");
  assert.equal((await api("GET","/api/v1/plugin/notifications/stream?access_token=forbidden")).status,403,"notification credentials must not enter a URL");
  const browser=await puppeteer.launch({headless:true,args:["--no-sandbox","--disable-setuid-sandbox"]});
  let credential;
  try {
    const page=await localPage(browser);
    const pageErrors=[];
    page.on("pageerror",error=>pageErrors.push(error.message));
    const artifact=path.join(__dirname,"artifacts","dalamud");await fs.mkdir(artifact,{recursive:true});
    await page.goto(new URL("/plugin/",base).href,{waitUntil:"networkidle0"});
    await page.waitForFunction(()=>!document.getElementById("boot-progress"));
    const user=(900000000000000000n+BigInt(Date.now()%1000000)).toString();
    const bootstrap=page.waitForResponse(response=>new URL(response.url()).pathname==="/api/v1/plugin/browser/approval");
    await page.goto(new URL("/test/login?user_id="+user+"&username=GameFlowFixture&redirect=/plugin/verify",base).href,{waitUntil:"domcontentloaded"});
    await page.waitForSelector("input[name=user_code]");
    assert.match(await page.title(),/Connect your game/);
    const pairing=await api("POST","/api/v1/plugin/pairings",{label:"My game <script>escaped</script>",scopes});
    assert.equal(pairing.status,200);
    assert.equal(pairing.data.verification_uri,new URL("/plugin/verify",base).href);
    const cookies=(await page.cookies(base)).map(c=>c.name+"="+c.value).join("; ");
    const csrf=(await (await bootstrap).json()).csrf;
    const form=new URLSearchParams({csrf,user_code:pairing.data.user_code});
    for(const forgedOrigin of [undefined,"https://attacker.invalid"]) {
      const result=await fetch(new URL("/api/v1/plugin/pairings/select",base),{method:"POST",redirect:"manual",headers:{"Content-Type":"application/x-www-form-urlencoded",Cookie:cookies,...(forgedOrigin?{Origin:forgedOrigin}:{})},body:form});
      assert.equal(result.status,403,"code selection requires an exact browser Origin");
    }
    assert.equal((await api("GET","/api/v1/plugin/session",undefined,undefined,{Cookie:cookies})).status,401,"cookie auth must never grant game access");
    assert.equal((await api("GET","/api/v1/plugin/browser/approval",undefined,"not-a-session",{Cookie:cookies})).status,403,"browser JSON must reject bearer auth even with cookies");
    assert.equal((await api("GET","/plugin/verify",undefined,"not-a-session")).status,403,"browser approval must reject bearer auth");
    assert.equal((await api("GET","/plugin/verify?code=must-not-be-in-url")).status,403);
    await page.type("input[name=user_code]","not-a-code");
    await page.click('[data-testid="plugin-select"]');
    await page.waitForFunction(()=>document.querySelector('[role="alert"]')?.textContent.includes("incorrect or has expired"));
    await page.$eval("input[name=user_code]",element=>{element.value="";element.dispatchEvent(new Event("input",{bubbles:true}));});
    await page.type("input[name=user_code]",pairing.data.user_code);
    const [review]=await Promise.all([page.waitForResponse(response=>new URL(response.url()).pathname==="/api/v1/plugin/pairings/select"),page.click('[data-testid="plugin-select"]')]);
    assert.equal(review.status(),200,"code selection must succeed with the protected website document Origin");
    await page.waitForSelector('[data-testid="plugin-review"]');
    assert.match(await page.title(),/Review game connection/);
    assert.equal(await page.$eval('[data-testid="plugin-review"]',element=>element.querySelector("script")),null,"client labels must be text, not executable markup");
    assert.match(await page.$eval("body",e=>e.textContent),/My game <script>escaped<\/script>/);
    assert([...new URL(page.url()).searchParams.keys()].every(key=>key==="lang"),"connection secrets must not enter the URL");
    assert(!page.url().includes(pairing.data.user_code)&&!page.url().includes(pairing.data.device_code));
    // Reading is required by editing; toggling the parent must keep checkboxes consistent.
    await page.click('input[name=scope][value="lists:read"]');
    assert.equal(await page.$eval('input[value="lists:write"]',element=>element.checked),false);
    await page.click('input[name=scope][value="lists:write"]');
    assert.equal(await page.$eval('input[value="lists:read"]',element=>element.checked),true);
    await page.screenshot({path:path.join(artifact,"consent.png"),fullPage:true});
    const [approved]=await Promise.all([page.waitForResponse(response=>new URL(response.url()).pathname==="/api/v1/plugin/pairings/approve"),page.click('[data-testid="plugin-approve"]')]);
    assert.equal(approved.status(),200);
    assert.equal(approved.headers()["referrer-policy"],"same-origin");
    await page.waitForSelector('[data-testid="plugin-decision"]');
    assert.match(await page.title(),/Game connection approved/);
    assert.deepEqual(await page.evaluate(()=>window.__pluginPolicyViolations),[],"consent must not trigger external-content violations");
    const poll=await api("POST","/api/v1/plugin/pairings/poll",{device_code:pairing.data.device_code});
    assert.equal(poll.status,200);
    credential=poll.data.refresh_token;
    const access=poll.data.access_token;
    assert.equal((await api("GET","/api/v1/plugin/session",undefined,credential)).status,401);
    const session=await api("GET","/api/v1/plugin/session",undefined,access);
    assert.equal(session.status,200);assert.equal(session.data.account_id,user);
    const duplicate=await api("POST","/api/v1/plugin/pairings/poll",{device_code:pairing.data.device_code});
    assert.equal(duplicate.status,400,"a consumed pairing must not mint a second family");
    const operation=crypto.randomUUID().replaceAll("-","");
    assert.equal((await api("POST","/api/v1/plugin/notifications/test",{operation_id:operation},access)).status,202);
    assert.equal((await api("POST","/api/v1/plugin/notifications/test",{operation_id:operation},access)).status,202);
    const batch=await api("GET","/api/v1/plugin/notifications",undefined,access);
    assert.equal(batch.status,200);assert.equal(batch.data.deliveries.length,1);
    const ids=batch.data.deliveries.map(d=>d.id);
    assert.equal((await api("POST","/api/v1/plugin/notifications/ack",{ids},access)).status,204);
    assert.equal((await api("GET","/api/v1/plugin/notifications",undefined,access)).data.deliveries.length,0);
    const legacyEndpoints=await api("GET","/api/v1/endpoints",undefined,undefined,{Cookie:cookies});
    assert.equal(legacyEndpoints.status,200);
    assert(!legacyEndpoints.data.some(endpoint=>endpoint.method==="InGame"),"cached clients must not receive an unknown endpoint variant");
    const endpoints=await api("GET","/api/v1/endpoints",undefined,undefined,{Cookie:cookies,"X-Ultros-Capabilities":"in-game-endpoints-v1"});
    assert.equal(endpoints.status,200);
    assert(endpoints.data.some(endpoint=>endpoint.id===session.data.endpoint_id&&endpoint.method==="InGame"&&endpoint.device_id===session.data.id));
    const websiteTest=await api("POST","/api/v1/endpoints/"+session.data.endpoint_id+"/test",undefined,undefined,{Cookie:cookies,Origin:origin});
    assert.equal(websiteTest.status,200);assert.equal(websiteTest.data.queued,true);assert.equal(websiteTest.data.delivered,false);
    const fromWebsite=await api("GET","/api/v1/plugin/notifications",undefined,access);
    assert.equal(fromWebsite.data.deliveries.length,1,"the website must queue a delivery for this connected client");
    assert.equal((await api("POST","/api/v1/plugin/notifications/ack",{ids:fromWebsite.data.deliveries.map(delivery=>delivery.id)},access)).status,204);
    const observe={content_id:"123456789",name:"Game Fixture",home_world:63,retainers:Array.from({length:9},(_,i)=>({content_id:String(987654321+i),name:"GameFixtureRetainer"+(i?i+1:""),city:1}))};
    const registration=await api("POST","/api/v1/plugin/retainers/register",{operation_id:crypto.randomUUID().replaceAll("-",""),character:observe},access);
    assert.equal(registration.status,200);const character=registration.data.filter(c=>c.name===observe.name&&c.home_world===observe.home_world).sort((a,b)=>Date.parse(b.observed_at)-Date.parse(a.observed_at))[0];assert(character&&!character.verified&&character.registered);
    const websiteRoster=await api("GET","/api/v1/user/retainer/in-game",undefined,undefined,{Cookie:cookies});
    assert.equal(websiteRoster.status,200);
    assert.match(websiteRoster.headers.get("cache-control"),/no-store/);
    const websiteCharacter=websiteRoster.data.find(c=>c.id===character.id);
    assert(websiteCharacter&&websiteCharacter.retainers.some(r=>r.name===observe.retainers[0].name),"game registration must appear on the website account");
    assert.equal(websiteCharacter.retainers.length,9,"the website must retain the full saved roster");
    assert.equal((await api("GET","/api/v1/user/retainer/in-game",undefined,access)).status,401,"a game token alone cannot authenticate the website roster");
    const anonymous=await browser.createBrowserContext();
    try {
      const stranger=await localPage(anonymous);
      await stranger.goto(new URL("/test/login?user_id=900000000000000112&username=OtherRosterFixture&redirect=/retainers",base).href,{waitUntil:"networkidle0"});
      assert(!(await stranger.$eval("body",e=>e.innerText)).includes("GameFixtureRetainer"),"another account must not see this game roster");
    } finally { await anonymous.close(); }
    for(const route of ["/retainers","/retainers/edit","/retainers/listings"]) {
      const response=await page.goto(new URL(route,base).href,{waitUntil:"networkidle0"});
      const html=await response.text();
      assert(html.includes("GameFixtureRetainer"),"the normal retainer view must render during SSR at "+route);
      await page.waitForFunction(()=>!document.getElementById("boot-progress"));
      assert((await page.$eval("body",e=>e.innerText)).includes("GameFixtureRetainer9"),"the entire roster must survive hydration at "+route);
      assert.equal(await page.$('[data-testid="saved-game-retainers"]'),null,"observed characters must use normal retainer views");
    }
    await page.screenshot({path:path.join(artifact,"saved-retainers.png"),fullPage:true});
    assert.deepEqual(pageErrors,[],"the saved roster routes must hydrate without browser exceptions");
    const consent=await api("POST","/api/v1/plugin/characters/"+character.id+"/uploads",{enabled:true},access);
    assert.equal(consent.status,200);const current=consent.data.find(c=>c.id===character.id);const retainer=current.retainers[0];
    assert(Number.isInteger(retainer.public_retainer_id),"registration must create a real retainer record before any Universalis listing");
    await page.goto(new URL("/retainers/listings",base).href,{waitUntil:"networkidle0"});
    const capture={capture_id:crypto.randomUUID().replaceAll("-",""),base_revision:retainer.revision,capture_epoch:current.capture_epoch,observed_at:new Date().toISOString(),complete:true,listings:[{slot:0,item_id:8,hq:false,quantity:5,unit_price:100},{slot:1,item_id:8,hq:false,quantity:5,unit_price:100}]};
    assert.equal((await api("POST","/api/v1/plugin/retainers/"+retainer.id+"/listings",capture,access)).status,200);
    await page.waitForSelector('[data-listing-source="in-game"]',{timeout:15000});
    const ownListings=await api("GET","/api/v1/user/retainer/listings",undefined,undefined,{Cookie:cookies});
    assert.match(ownListings.headers.get("cache-control"),/private.*no-store/);
    const gameGroup=ownListings.data.retainers.find(([group])=>group?.kind==="unverified"&&group.character.id===character.id);
    assert(gameGroup,"observed characters must be grouped in the normal API");
    const pendingRows=gameGroup[1].find(([r])=>r.id===retainer.public_retainer_id)[1];
    assert.equal(pendingRows.length,2);assert(pendingRows.every(row=>row.id.source==="in_game"));
    const publicRows=await api("GET","/api/v1/retainer/listings/"+retainer.public_retainer_id);
    assert.equal(publicRows.data.listings.length,0,"private uploads must not be injected into public listings");
    for(const route of ["/retainers/listings","/retainers/listings/"+retainer.public_retainer_id]){
      const response=await page.goto(new URL(route,base).href,{waitUntil:"networkidle0"});
      assert.match(response.headers()["cache-control"],/no-store/);
      assert((await response.text()).includes("Awaiting confirmation"),"private rows must render during SSR");
      await page.waitForSelector('[data-listing-source="in-game"]');
      assert.equal(await page.$$eval('[data-listing-source="in-game"]',rows=>rows.length),2,"identical stacks must stay separate after hydration");
    }
    await page.screenshot({path:path.join(artifact,"private-retainer-listings.png"),fullPage:true});
    await page.goto(new URL("/retainers/undercuts",base).href,{waitUntil:"networkidle0"});
    assert((await page.$eval("body",e=>e.innerText)).includes("GameFixtureRetainer"),"unverified characters must participate in Undercuts");
    assert.deepEqual(pageErrors,[],"private listing routes must hydrate without browser exceptions");
    assert.equal((await api("POST","/api/v1/plugin/characters/"+character.id+"/uploads",{enabled:false,delete_snapshots:true},access)).status,200);
    const deleted=await api("GET","/api/v1/user/retainer/listings",undefined,undefined,{Cookie:cookies});
    assert(deleted.data.retainers.flatMap(([,retainers])=>retainers.flatMap(([,rows])=>rows)).every(row=>row.id.source!=="in_game"),"deleting private captures must remove their website rows");
    if(process.env.DALAMUD_RETAINER_ONLY==="1"){
      console.log("PASS: integrated onboarding and consent, real retainer registration, normal unverified-character grouping, immediate private listings in SSR/hydrated All Listings and individual views, separate identical stacks, Undercuts, account isolation, public-table separation and deletion.");
      return;
    }
    const listings=await api("GET","/api/v1/plugin/lists",undefined,access);
    assert.equal(listings.status,200);
    const rotated=await api("POST","/api/v1/plugin/session/refresh",{refresh_token:credential});
    assert.equal(rotated.status,200);
    assert.equal((await api("POST","/api/v1/plugin/session/refresh",{refresh_token:credential})).status,401);
    assert.equal((await api("GET","/api/v1/plugin/session",undefined,rotated.data.access_token)).status,401,"old refresh reuse must revoke its successor");
    // The same permission-protected management UI supports empty, connected and revoked states.
    await page.goto(new URL("/plugin/devices",base).href,{waitUntil:"domcontentloaded"});
    await page.waitForSelector('[data-testid="plugin-devices"]');
    assert.match(await page.$eval('[data-testid="plugin-devices"]',element=>element.innerText),/No game clients are connected/);
    const disconnectPairing=await api("POST","/api/v1/plugin/pairings",{label:"Disconnect fixture",scopes:["lists:read"]});
    assert.equal(disconnectPairing.status,200);
    await page.click('a[href="/plugin/verify"].btn-primary');
    await page.waitForSelector("input[name=user_code]");
    await page.type("input[name=user_code]",disconnectPairing.data.user_code);
    await page.click('[data-testid="plugin-select"]');
    await page.waitForSelector('[data-testid="plugin-review"]');
    await page.click('[data-testid="plugin-approve"]');
    await page.waitForSelector('[data-testid="plugin-decision"]');
    const disconnectTokens=await api("POST","/api/v1/plugin/pairings/poll",{device_code:disconnectPairing.data.device_code});
    assert.equal(disconnectTokens.status,200);
    credential=disconnectTokens.data.refresh_token;
    await page.click('[data-testid="plugin-decision"] a[href="/plugin/devices"]');
    await page.waitForSelector('[data-testid="plugin-revoke"]');
    await page.screenshot({path:path.join(artifact,"devices.png"),fullPage:true});
    await page.click('[data-testid="plugin-revoke"]');
    await page.waitForFunction(()=>document.querySelector('[data-testid="plugin-devices"]')?.textContent.includes("No game clients are connected"));
    assert.equal((await api("GET","/api/v1/plugin/session",undefined,disconnectTokens.data.access_token)).status,401,"website disconnect must immediately revoke the bearer grant");
    const denied=await api("POST","/api/v1/plugin/pairings",{label:"Decline fixture",scopes:["lists:read"]});
    assert.equal(denied.status,200);
    await page.click('a[href="/plugin/verify"].btn-primary');
    await page.waitForSelector("input[name=user_code]");
    await page.type("input[name=user_code]",denied.data.user_code);
    await page.click('[data-testid="plugin-select"]');
    await page.waitForSelector('[data-testid="plugin-review"]');
    await page.click('[data-testid="plugin-deny"]');
    await page.waitForSelector('[data-testid="plugin-decision"]');
    assert.match(await page.title(),/Game connection declined/);
    const deniedPoll=await api("POST","/api/v1/plugin/pairings/poll",{device_code:denied.data.device_code});
    assert.equal(deniedPoll.status,403);
    // An ordinary SPA document must reload before it can fetch consent data.
    await page.goto(new URL("/list",base).href,{waitUntil:"networkidle0"});
    assert.equal(await page.$("#ultros-plugin-document"),null);
    await Promise.all([page.waitForNavigation({waitUntil:"networkidle0"}),page.click('.side-nav-info a[href="/plugin"]')]);
    assert(await page.$("#ultros-plugin-document"));
    assert(await page.$('[data-testid="plugin-account"]'));
    assert.deepEqual(pageErrors,[],"the integrated routes must hydrate and navigate without exceptions");
    console.log("PASS: integrated Leptos setup/install/download, desktop/mobile layout, consent/decline/CSRF/CSP, escaped labels, permission dependencies, protected document navigation, device disconnect, bearer-cookie isolation, single redemption, website retainer registration, private uploads, game notifications, refresh reuse revocation.");
  } finally {
    if(credential)await api("POST","/api/v1/plugin/session/revoke",{refresh_token:credential});
    await browser.close();
  }
}
main().catch(error=>{console.error(error.name+": "+error.message);process.exitCode=1;});
