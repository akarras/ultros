// Material details remain usable outside the grid's fixed-height virtual rows.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD = process.env.WORLD || 'Goblin';

async function main() {
  const browser = await puppeteer.launch({headless:'new',args:['--no-sandbox']});
  try {
    for (const mobile of [false,true]) {
      const page = await browser.newPage();
      const errors=[];
      page.on('pageerror', e=>errors.push(e.message));
      page.on('console', m=>{if(m.type()==='error' && !/favicon|ERR_BLOCKED_BY_CLIENT|net::ERR_ABORTED/.test(m.text())) errors.push(m.text());});
      await page.setViewport({width:mobile?393:1280,height:800,isMobile:mobile,hasTouch:mobile});
      await page.setCookie({name:'HIDE_ADS',value:'true',url:BASE},{name:'HOME_WORLD',value:WORLD,url:BASE});
      await page.evaluateOnNewDocument(()=>window.addEventListener('ultros:hydrated',()=>window.__hydrated=true));
      await page.goto(`${BASE}/fc-crafting-analyzer/${encodeURIComponent(WORLD)}?min-sales=0`,{waitUntil:'domcontentloaded',timeout:90000});
      await page.waitForFunction(()=>window.__hydrated,{timeout:90000});
      const selector='.virtual-grid-cell button[aria-haspopup="dialog"]';
      await page.waitForSelector(selector,{timeout:90000});
      const url=page.url();
      for(let attempt=0;attempt<2;attempt++) {
        const button=await page.$(selector);
        await button.evaluate(e=>e.scrollIntoView({block:'center',inline:'nearest'}));
        if(mobile) await button.tap(); else await button.click();
        await page.waitForSelector('dialog[open]',{visible:true});
        assert(await page.$eval('dialog[open]',e=>e.contains(document.activeElement)),'modal owns keyboard focus');
        assert.equal(page.url(),url,'opening a breakdown must not follow the item link');
        const result=await page.$eval('dialog[open]',e=>{
          const r=e.getBoundingClientRect();
          return {insideGrid:!!e.closest('.virtual-grid'),text:e.textContent,height:r.height,left:r.left,right:r.right,viewport:innerWidth};
        });
        assert(!result.insideGrid,'details must escape the virtual row clip');
        assert(result.height>60 && result.left>=0 && result.right<=result.viewport+1);
        assert(/\d+\s*x\s+/.test(result.text),'material quantities must render every time the dialog opens');
        const dir=path.join(__dirname,'artifacts','fc-crafting');fs.mkdirSync(dir,{recursive:true});
        if(attempt===0) await page.screenshot({path:path.join(dir,`breakdown-${mobile?'mobile':'desktop'}.png`),fullPage:true});
        if(mobile) await page.tap('dialog[open] button'); else await page.keyboard.press('Escape');
        await page.waitForSelector('dialog[open]',{hidden:true});
        assert(await button.evaluate(e=>document.activeElement===e),'closing returns focus to the grid control');
      }
      assert.deepEqual(errors,[]);
      await page.close();
    }
    console.log('PASS: FC material details open, close and reopen on desktop and touch without clipping or navigation.');
  } finally {await browser.close();}
}
main().catch(e=>{console.error(e);process.exitCode=1;});
