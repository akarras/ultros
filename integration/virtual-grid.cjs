// Tests the real Leptos component through its debug-only deterministic route.
// Run against a fresh build of this worktree; no live market data is needed.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function main() {
  const browser = await puppeteer.launch({headless: 'new', args: ['--no-sandbox']});
  const page = await browser.newPage();
  await page.setCookie({name:'HIDE_ADS',value:'true',url:BASE});
  await page.evaluateOnNewDocument(()=>{
    window.__gridHydrated=false;
    window.addEventListener('ultros:hydrated',()=>{window.__gridHydrated=true;});
  });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', msg => { if (msg.type() === 'error') errors.push(msg.text()); });
  const dir = path.join(__dirname, 'artifacts', 'virtual-grid');
  fs.mkdirSync(dir, {recursive: true});
  async function menu(id) {
    await page.click(`.virtual-grid-heading[data-column="${id}"] .grid-column-menu`);
    await page.waitForSelector('.grid-menu-panel');
  }
  async function action(label) {
    await page.evaluate(label => {
      const button = [...document.querySelectorAll('.grid-menu-panel button')].find(b => b.textContent.trim() === label);
      if (!button) throw Error(`Missing column action: ${label}`);
      button.click();
    }, label);
    await sleep(120);
  }
  async function width(id) {
    return page.$eval(`.virtual-grid-heading[data-column="${id}"]`, e => e.getBoundingClientRect().width);
  }
  async function scroll(x, y) {
    await page.$eval('.virtual-grid', (el, x, y) => { el.scrollLeft=x; el.scrollTop=y; }, x, y);
    await sleep(180);
  }
  async function verifyAlignment() {
    const measurement = await page.evaluate(() => {
      const port = document.querySelector('.virtual-grid');
      const bounds = port.getBoundingClientRect();
      const headers = [...port.querySelectorAll('[role=columnheader]')];
      const failures = [];
      for (const header of headers) {
        const h = header.getBoundingClientRect();
        if (h.right <= bounds.left || h.left >= bounds.right) continue;
        const cell = port.querySelector(`.virtual-grid-cell[data-column="${header.dataset.column}"]`);
        if (!cell) {failures.push(`Missing cells for ${header.dataset.column}`);continue;}
        const c = cell.getBoundingClientRect();
        if (Math.abs(h.left-c.left)>1 || Math.abs(h.width-c.width)>1) failures.push(`Misaligned ${header.dataset.column}`);
        if (Math.abs(h.top-bounds.top-1)>1) failures.push(`Header not frozen: ${h.top-bounds.top}`);
      }
      const extraPorts = [...port.querySelectorAll('*')].filter(e => ['auto','scroll'].includes(getComputedStyle(e).overflowX) && e.scrollWidth > e.clientWidth);
      return {failures, extraPorts:extraPorts.length, cells:port.querySelectorAll('[role=gridcell]').length};
    });
    assert.deepEqual(measurement.failures, []);
    assert.equal(measurement.extraPorts, 0, 'header and body must share one horizontal scrollport');
    assert(measurement.cells < 1000, `both axes must be virtualized: ${measurement.cells} cells`);
  }
  try {
    await page.setViewport({width:1280,height:900});
    await page.goto(`${BASE}/__test/virtual-grid`, {waitUntil:'networkidle0',timeout:120000});
    await page.waitForSelector('.virtual-grid-cell');
    await page.waitForFunction(()=>window.__gridHydrated,{timeout:120000});
    await page.click('#fixture-update');
    await page.waitForFunction(() => document.querySelector('.virtual-grid-cell')?.textContent.includes('/ 1'));
    await verifyAlignment();
    await scroll(2400,40000);
    await page.waitForSelector('[data-grid-row="1001"][data-column="c20"]');
    await verifyAlignment();
    await page.click('#fixture-update');
    await page.waitForFunction(() => document.querySelector('[data-grid-row="1001"][data-column="c20"]')?.textContent.includes('/ 2'));
    assert.equal(await page.$eval('.virtual-grid',e=>e.scrollTop),40000,'live updates retain scroll');
    await scroll(1e6,1e6);
    await page.waitForSelector('[data-grid-row="10000"][data-column="c59"]');
    await verifyAlignment();
    await scroll(0,0);
    const border = await page.$('.virtual-grid-heading[data-column="c00"] .grid-resize-handle');
    await border.click({clickCount:2});
    await page.waitForFunction(() => document.querySelector('[role=columnheader][data-column=c00]').getBoundingClientRect().width > 350);
    const fitted=await width('c00');
    assert(fitted<800,'auto-fit stays bounded');
    assert(new URL(page.url()).searchParams.has('layout'));
    const handle=await border.boundingBox();
    await page.mouse.move(handle.x+3,handle.y+20);
    await page.mouse.down();
    await page.mouse.move(handle.x+63,handle.y+20,{steps:8});
    await page.mouse.up();
    await sleep(150);
    assert(Math.abs(await width('c00')-fitted-60)<2,'dragging resizes the column');
    await verifyAlignment();
    const beforeCancel=await width('c00');
    const cancelHandle=await border.boundingBox();
    await page.mouse.move(cancelHandle.x+3,cancelHandle.y+20);await page.mouse.down();
    await page.mouse.move(cancelHandle.x+40,cancelHandle.y+20,{steps:4});
    await page.keyboard.press('Escape');await page.mouse.up();await sleep(150);
    assert.equal(await width('c00'),beforeCancel,'Escape restores resize');
    await menu('c00');await action('Insert column after…');await action('Column 60');
    await page.waitForSelector('[role=columnheader][data-column=c60]');
    assert(new URL(page.url()).searchParams.get('cols').split(',').includes('c60'));
    let layout=JSON.parse(new URL(page.url()).searchParams.get('layout'));
    assert.equal(layout.order[layout.order.indexOf('c00')+1],'c60');
    await menu('c60');await action('Move left');
    layout=JSON.parse(new URL(page.url()).searchParams.get('layout'));
    assert.equal(layout.order[0],'c60');
    await verifyAlignment();
    const grip=await page.$('.virtual-grid-heading[data-column=c60] .grid-drag-handle');
    const source=await grip.boundingBox();
    const destination=await page.$eval('.virtual-grid-heading[data-column=c00]', e=>{
      const r=e.getBoundingClientRect();return {x:r.right-15,y:r.top+25};
    });
    await page.mouse.move(source.x+source.width/2,source.y+source.height/2);
    await page.mouse.down();await page.mouse.move(destination.x,destination.y,{steps:12});await page.mouse.up();await sleep(180);
    layout=JSON.parse(new URL(page.url()).searchParams.get('layout'));
    assert.equal(layout.order[layout.order.indexOf('c00')+1],'c60','grip drag commits the insertion target');
    await verifyAlignment();
    await page.goBack({waitUntil:'networkidle0'});
    await page.waitForFunction(()=>JSON.parse(new URL(location.href).searchParams.get('layout')).order[0]==='c60');
    assert.equal(await page.$eval('[role=columnheader][data-column=c60]',e=>e.getAttribute('aria-colindex')),'1','history restores rendered order');
    await page.goForward({waitUntil:'networkidle0'});
    await page.waitForFunction(()=>document.querySelector('[role=columnheader][data-column=c60]')?.getAttribute('aria-colindex')==='2');
    await scroll(0,10000);
    await menu('c60');await action('Reset column width');
    assert.equal(await page.$eval('.virtual-grid',e=>e.scrollTop),10000,'layout commits retain vertical scroll');
    await menu('c60');await action('Hide column');
    await page.waitForFunction(() => !document.querySelector('[role=columnheader][data-column=c60]'));
    assert.equal(await page.$eval('#fixture-sorts',e=>e.textContent),'0','column operations never sort');
    await page.reload({waitUntil:'networkidle0'});
    assert(Math.abs(await width('c00')-beforeCancel)<2,'reload restores saved URL width');
    await scroll(0,0);
    await page.focus('.virtual-grid');await page.keyboard.press('F2');await page.keyboard.press('Tab');await page.keyboard.press('Enter');
    await page.waitForFunction(()=>document.querySelector('#fixture-sorts').textContent==='1');
    await page.keyboard.press('Escape');
    assert.equal(await page.evaluate(()=>document.activeElement.className),'virtual-grid','Escape leaves cell interaction mode');
    await page.focus('.virtual-grid');await page.keyboard.down('Control');await page.keyboard.press('End');await page.keyboard.up('Control');
    await page.waitForFunction(() => {
      const g=document.querySelector('.virtual-grid');return g.getAttribute('aria-activedescendant')?.includes('-r10000-') && document.getElementById(g.getAttribute('aria-activedescendant'));
    });
    await verifyAlignment();
    await page.keyboard.down('Shift');await page.keyboard.press('F10');await page.keyboard.up('Shift');
    await page.waitForSelector('.grid-menu-panel');await page.keyboard.press('Escape');
    assert.equal(await page.evaluate(()=>document.activeElement.className),'virtual-grid');
    for (const viewport of [{width:620,height:800},{width:393,height:844}]) {
      await page.setViewport(viewport);await scroll(5000,200000);await verifyAlignment();
      await page.screenshot({path:path.join(dir,`grid-${viewport.width}.png`),fullPage:true});
    }
    await page.click('#fixture-empty');await page.waitForFunction(()=>!document.querySelector('[role=gridcell]'));
    await page.click('#fixture-restore');await page.waitForSelector('[role=gridcell]');
    assert.deepEqual(errors,[],'no hydration or browser errors');
    console.log('VirtualGrid: both axes, alignment, live updates, auto-fit, resize, cancel, insertion, ordering, persistence, keyboard and narrow screens passed');
  } catch(error) {
    console.error('Browser errors:',errors);
    console.error('Grid state:',await page.evaluate(()=>({
      url:location.href,cells:document.querySelectorAll('[role=gridcell]').length,
      firstCell:document.querySelector('[role=gridcell]')?.textContent,
      headings:document.querySelectorAll('[role=columnheader]').length,
      sorts:document.querySelector('#fixture-sorts')?.textContent,
      scripts:[...document.scripts].map(s=>s.src).filter(Boolean),
    })).catch(()=>null));
    await page.screenshot({path:path.join(dir,'failure.png'),fullPage:true}).catch(()=>{});
    throw error;
  } finally {await browser.close();}
}
main().catch(e=>{console.error(e);process.exitCode=1;});
