"use strict";
const assert=require("node:assert/strict");
const fs=require("node:fs/promises");
const path=require("node:path");
const puppeteer=require("puppeteer");
const base=process.env.BASE_URL||"http://localhost:8080";
assert(["localhost","127.0.0.1","[::1]"].includes(new URL(base).hostname),"Use the isolated test-auth fixture");
async function main(){
  const browser=await puppeteer.launch({headless:true,args:["--no-sandbox"]});
  let page,stage="initialization";const errors=[];
  try{
    page=await browser.newPage();page.setDefaultTimeout(45000);page.setDefaultNavigationTimeout(120000);
    page.on("pageerror",error=>errors.push(error.message));
    await page.evaluateOnNewDocument(()=>{window.__guideHydrated=false;addEventListener("ultros:hydrated",()=>window.__guideHydrated=true);});
    await page.setRequestInterception(true);
    page.on("request",request=>{
      const url=new URL(request.url());
      if(["http:","https:"].includes(url.protocol)&&!["localhost","127.0.0.1","[::1]"].includes(url.hostname))request.abort();else request.continue();
    });
    const info=await (await fetch(new URL("/api/v1/plugin/browser/info",base))).json();
    const available=info.enabled&&(info.localhost||[info.latest_version,info.repository_url,info.download_url].every(value=>typeof value==="string"&&value.length>0));
    const folder=path.join(__dirname,"artifacts","dalamud");await fs.mkdir(folder,{recursive:true});
    for(const [name,width,height] of [["desktop",1440,1000],["mobile",390,844]]){
      await page.setViewport({width,height});
      await page.deleteCookie({name:"LABS_DALAMUD",url:new URL("/",base).href,path:"/"});
      stage=name+" default-off Labs gate";
      const hidden=await page.goto(new URL("/plugin/",base).href,{waitUntil:"networkidle0"});
      assert((await hidden.text()).includes("plugin-labs-required"),"The opt-in gate must be server-rendered");
      await page.waitForFunction(()=>window.__guideHydrated);
      assert(await page.$('[data-testid="plugin-labs-required"]'));
      assert.equal(await page.$('[data-testid="plugin-install-guide"]'),null);
      assert.equal(await page.$('.side-nav-info a[href="/plugin"]'),null);
      stage=name+" settings opt-in";
      await page.goto(new URL("/settings#labs",base).href,{waitUntil:"networkidle0"});
      await page.waitForFunction(()=>window.__guideHydrated);
      const toggle='[data-testid="dalamud-labs-setting"] input[role="switch"]';
      assert.equal(await page.$eval(toggle,element=>element.checked),false);
      await page.click('[data-testid="dalamud-labs-setting"] label');
      await page.waitForFunction(()=>document.cookie.split("; ").includes("LABS_DALAMUD=true"));
      await page.reload({waitUntil:"networkidle0"});
      await page.waitForFunction(()=>window.__guideHydrated);
      assert.equal(await page.$eval(toggle,element=>element.checked),true,"The Labs opt-in must survive reload");
      await page.screenshot({path:path.join(folder,`labs-${name}.png`),fullPage:true});
      stage=name+" navigation";
      const response=await page.goto(new URL("/plugin/",base).href,{waitUntil:"networkidle0"});
      assert.match(response.headers()["content-security-policy"],/frame-src 'none'/);
      assert.match(response.headers()["cache-control"],/no-store/);
      await page.waitForSelector(available?'[data-testid="plugin-install-guide"]':'[data-testid="plugin-unavailable"]');
      stage=name+" hydration";
      // Large development WASM can outlast the progress bar's 30s watchdog.
      // Assert the actual hydration event rather than the cosmetic indicator.
      await page.waitForFunction(()=>window.__guideHydrated);
      stage=name+" controls and layout";
      assert(await page.$('.side-nav-info a[href="/plugin"]'),"The plugin link should appear after opting in");
      if(!available){
        assert.equal(await page.$('[data-testid="plugin-install-guide"]'),null);
        assert.equal(await page.$('.plugin-page a[href="/plugin/verify"]'),null);
        assert.equal(await page.$('.plugin-page a[href="/login?next=/plugin"]'),null);
        assert.equal(await page.$('[data-testid="plugin-repository-url"]'),null);
        assert.equal(await page.$('[data-testid="plugin-manual-install"]'),null);
        assert((await page.$eval('.plugin-page',element=>element.textContent)).includes("Game connections are not available"));
        assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),"Unavailable notice must fit the viewport");
        await page.screenshot({path:path.join(folder,`setup-${name}.png`),fullPage:true});
        continue;
      }
      assert.equal(await page.$$eval('[data-testid="plugin-install-guide"] > li',rows=>rows.length),4);
      assert.equal(await page.$eval('[data-testid="plugin-repository-url"]',element=>element.textContent),info.repository_url);
      assert.equal(await page.$eval('a[href="/login?next=/plugin"]',element=>element.textContent),"Sign in with Discord");
      assert(await page.$('a[href="/plugin/verify"]'));assert(await page.$('a[href="/plugin/devices"]'));
      const text=await page.$eval('[data-testid="plugin-install-guide"]',element=>element.textContent);
      for(const phrase of ["/xlsettings","/xlplugins","/ultros","Register all observed retainers","Awaiting confirmation","five minutes"])assert(text.includes(phrase),phrase);
      if(info.download_url){
        await page.$eval('[data-testid="plugin-manual-install"]',element=>element.open=true);
        const download=await page.$eval('[data-testid="plugin-manual-install"] a',element=>element.getAttribute("href"));
        assert.equal(download,info.download_url);
        assert.equal((await fetch(new URL(download,base),{method:"HEAD"})).status,200);
        assert.equal(await page.$('[data-testid="plugin-release-pending"]'),null);
      }else assert(await page.$('[data-testid="plugin-release-pending"]'));
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),"Guide must fit the viewport");
      await page.screenshot({path:path.join(folder,`setup-${name}.png`),fullPage:true});
    }
    assert.deepEqual(errors,[]);
    stage="disable Labs again";
    await page.goto(new URL("/settings#labs",base).href,{waitUntil:"networkidle0"});
    await page.waitForFunction(()=>window.__guideHydrated);
    await page.click('[data-testid="dalamud-labs-setting"] label');
    await page.waitForFunction(()=>document.cookie.split("; ").includes("LABS_DALAMUD=false"));
    await page.goto(new URL("/plugin/verify",base).href,{waitUntil:"networkidle0"});
    await page.waitForFunction(()=>window.__guideHydrated);
    assert(await page.$('[data-testid="plugin-labs-required"]'));
    assert.equal(await page.$('input[name="user_code"]'),null);
    assert.equal(await page.$('.side-nav-info a[href="/plugin"]'),null);
    console.log("PASS: default-off Labs settings, persisted opt-in/out, gated setup/approval, available/local guides, desktop/mobile layout and sensitive-page policy.");
  }catch(error){
    console.error("Guide stage:",stage,"; page errors:",errors);
    if(page&&!page.isClosed())await page.screenshot({path:path.join(__dirname,"artifacts","dalamud","setup-failure.png"),fullPage:true});
    throw error;
  }finally{await browser.close();}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
