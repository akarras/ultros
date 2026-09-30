// Local-only browser regression: all list writes are intercepted.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const { marketFixture, itemIds } = require('./shared-analyzer-market-fixture.cjs');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
assert(['localhost', '127.0.0.1', '[::1]'].includes(new URL(BASE).hostname));
const out = path.join(__dirname, 'artifacts', 'recipe-preview-inline');
async function main() {
  fs.mkdirSync(out, { recursive: true });
  const browser = await puppeteer.launch({headless:true,args:['--no-sandbox']});
  const page = await browser.newPage();
  page.setDefaultTimeout(90000);
  const errors = [], writes = [];
  let listMode = 'ok', submitMode = 'error', finishSubmit;
  page.on('pageerror', e => errors.push(e.message));
  await page.evaluateOnNewDocument(() => window.addEventListener('ultros:hydrated', () => window.__hydrated = true));
  await page.setCookie({name:'HOME_WORLD',value:'Gilgamesh',url:BASE},{name:'HIDE_ADS',value:'true',url:BASE});
  const fixture = marketFixture(itemIds);
  await page.setRequestInterception(true);
  page.on('request', async r => {
    if (r.isInterceptResolutionHandled()) return;
    const u = new URL(r.url());
    if (u.origin !== new URL(BASE).origin && !['data:','blob:'].includes(u.protocol)) return r.abort();
    const json = (body, status=200) => r.respond({status,contentType:'application/json',body:JSON.stringify(body)});
    if (u.pathname === '/api/v1/list') {
      if (listMode === 'signed-out') return json({ApiError:'NotAuthenticated'},401);
      if (listMode === 'error') return json({ApiError:{Message:'List fixture failure'}},500);
      return json([{list:{id:901,owner:1,name:'Preview fixture A',wdr_filter:{World:74}},permission:'Owner'},
                   {list:{id:902,owner:1,name:'Preview fixture B',wdr_filter:{World:74}},permission:'Owner'}]);
    }
    if (/\/api\/v1\/list\/\d+\/add\/items$/.test(u.pathname)) {
      writes.push({url:u.pathname,items:JSON.parse(r.postData())});
      await new Promise(resolve => finishSubmit = resolve);
      return submitMode === 'success' ? json(null) : json({ApiError:{Message:'Submit fixture failure'}},500);
    }
    // No actual mutating list request can reach the local server.
    if (u.pathname.startsWith('/api/v1/list') && r.method() !== 'GET') return r.abort();
    const response = fixture.reply(r);
    return response ? r.respond(response) : r.continue();
  });
  const form = '[data-recipe-list-form]';
  const craft = form+' input[id^="craft-qty"]', qty = form+' input[id^="ingredient-qty"]:not(:disabled)';
  const select = form+' select', submit = form+' button[type="submit"]';
  async function input(selector,value) {
    await page.$eval(selector,(el,v) => {el.value=String(v);el.dispatchEvent(new Event('input',{bubbles:true}));},value);
  }
  async function open() {
    await page.click('[data-recipe-breakdown-toggle]');
    await page.waitForSelector(form);
    await page.waitForSelector(select);
  }
  try {
    await page.goto(BASE+'/__test/shared-analyzer-data?lang=en',{waitUntil:'domcontentloaded'});
    await page.waitForFunction(() => window.__hydrated);
    await page.evaluate(href => {const a=document.createElement('a');a.href=href;document.querySelector('main').prepend(a);a.click();}, BASE+'/recipe-analyzer/Gilgamesh?lang=en');
    await page.waitForSelector('[data-recipe-breakdown-toggle]');
    for (const width of [1178,390]) {
      await page.setViewport({width,height:900});
      await open();
      assert.equal(await page.$$eval('[role="dialog"]', els=>els.length),0,'No nested dialog');
      assert.equal(await page.$eval(submit,el=>el.disabled),true,'Choose destination before submitting');
      await page.select(select,'902');
      await input(craft,3);
      await input(qty,17);
      await input(craft,4);
      assert.equal(await page.$eval(qty,el=>el.value),'17','Craft count preserves manual override');
      // Positive crystal label remains stable while the underlying switch changes.
      const labels = await page.$$eval(form+' label:has([role="switch"])',els=>els.map(el=>el.textContent.trim()));
      assert(labels[1].includes('Include'),'Positive include crystals label');
      await page.$eval(form+' [role="switch"]',el=>{el.checked=true;el.dispatchEvent(new Event('change',{bubbles:true}));});
      await page.$$eval(form+' label:has([role="switch"])',els=>els[1].click());
      const after = await page.$$eval(form+' label:has([role="switch"])',els=>els.map(el=>el.textContent.trim()));
      assert.equal(after[1],labels[1]);
      await page.click(submit);
      await page.waitForFunction(s=>document.querySelector(s).disabled,{},submit);
      await page.waitForFunction(s=>document.querySelector(s).textContent.includes('Adding'),{},submit);
      assert.equal(writes.at(-1).url,'/api/v1/list/902/add/items');
      assert(writes.at(-1).items.every(i=>i.list_id===902 && i.quantity>0));
      finishSubmit();
      await page.waitForFunction(s=>document.querySelector(s).textContent.includes('Submit fixture failure'),{},form);
      assert.equal(await page.$eval(select,el=>el.value),'902','Error preserves destination');
      assert.equal(await page.$eval(qty,el=>el.value),'17','Error preserves override');
      const fits = await page.$$eval('.recipe-breakdown-table tr',rows=>rows.every(row=>{
        const cells=[...row.cells]; return cells.every((c,i)=>!i || c.getBoundingClientRect().left>=cells[i-1].getBoundingClientRect().right-1 && c.scrollWidth <= c.clientWidth);
      }));
      assert(fits,'Price columns must not overlap');
      await page.screenshot({path:path.join(out,width+'.png')});
      submitMode='success';
      await page.click(submit);
      await page.waitForFunction(s=>document.querySelector(s).disabled,{},submit);
      finishSubmit();
      await page.waitForSelector(form,{hidden:true});
      await open();
      assert.equal(await page.$eval(craft,el=>el.value),'1','Reopen starts a fresh draft');
      await page.focus(craft);
      await page.keyboard.press('Escape');
      await page.waitForSelector(form,{hidden:true});
      assert(await page.$eval('[data-recipe-breakdown-toggle]',el=>el===document.activeElement),'Escape restores toggle focus');
      submitMode='error';
    }
    for (listMode of ['signed-out','error']) {
      await page.click('[data-recipe-breakdown-toggle]');
      await page.waitForSelector(form);
      if (listMode==='signed-out') await page.waitForSelector(form+' a[href^="/login"]');
      else await page.waitForFunction(s=>document.querySelector(s).textContent.includes('load'),{},form);
      await page.click('.recipe-breakdown-close');
    }
    assert.deepEqual(errors,[]);
    console.log('Recipe preview inline browser regression passed; list writes intercepted:',writes.length);
  } finally { await browser.close(); }
}
main().catch(e=>{console.error(e);process.exitCode=1;});
