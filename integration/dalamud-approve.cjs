"use strict";
// Invoked by the managed HTTP smoke. Pairing data arrives on stdin, never argv.
const puppeteer=require("puppeteer");
const assert=require("node:assert/strict");
let input="";process.stdin.setEncoding("utf8");process.stdin.on("data",part=>input+=part);
process.stdin.on("end",async()=>{
  let browser;
  try {
    const request=JSON.parse(input);input="";
    const origin=new URL(request.origin);
    assert(["localhost","127.0.0.1","[::1]"].includes(origin.hostname));
    browser=await puppeteer.launch({headless:true,args:["--no-sandbox","--disable-setuid-sandbox"]});
    const page=await browser.newPage();
    page.setDefaultTimeout(120000);page.setDefaultNavigationTimeout(120000);
    await page.goto(new URL("/test/login?user_id=900000000000000112&username=ManagedClientFixture&redirect=/plugin",origin).href,{waitUntil:"domcontentloaded"});
    if(request.approval_url) {
      const approval=new URL(request.approval_url);
      assert(approval.origin===origin.origin&&approval.pathname==="/plugin/verify"&&approval.search===""&&/^#pair=[A-Za-z0-9_-]{43}$/.test(approval.hash),"invalid direct approval URL");
      await page.goto(approval.href,{waitUntil:"domcontentloaded"});
      await page.waitForSelector('[data-testid="plugin-review"]');
      assert.equal(new URL(page.url()).hash,"");
      assert.equal(await page.$("input[name=user_code]"),null,"a direct link must skip manual entry");
      assert((await page.$eval('[data-testid="plugin-review"]',element=>element.innerText)).includes(request.code.replaceAll("-","")));
    } else {
      await page.goto(new URL("/plugin/verify",origin).href,{waitUntil:"domcontentloaded"});
      await page.waitForSelector("input[name=user_code]");
      await page.type("input[name=user_code]",request.code);
      const [review]=await Promise.all([page.waitForResponse(response=>new URL(response.url()).pathname==="/api/v1/plugin/pairings/select"),page.click('[data-testid="plugin-select"]')]);
      assert.equal(review.status(),200,"code selection failed");
      await page.waitForSelector('[data-testid="plugin-review"]');
    }
    assert.match(await page.title(),/Review game connection/);
    await page.click('[data-testid="plugin-approve"]');
    await page.waitForSelector('[data-testid="plugin-decision"]');
    assert.match(await page.title(),/Game connection approved/);
    const cookies=(await page.cookies(origin.href)).map(c=>c.name+"="+c.value).join("; ");
    const name="Managed game cart "+Date.now();
    const response=await fetch(new URL("/api/v1/list/create",origin),{method:"POST",headers:{"Content-Type":"application/json",Cookie:cookies,Origin:origin.origin},body:JSON.stringify({name,wdr_filter:{World:63}})});
    assert.equal(response.status,200,"create a private cart using the browser account");
    const listed=await fetch(new URL("/api/v1/list",origin),{headers:{Cookie:cookies}});
    assert.equal(listed.status,200);
    const list=(await listed.json()).find(row=>row.list.name===name);
    assert(list);
    process.stdout.write(JSON.stringify({list_id:list.list.id})+"\n");
  }catch(error){console.error(error.name+": "+error.message);process.exitCode=1;}
  finally{if(browser)await browser.close();}
});
