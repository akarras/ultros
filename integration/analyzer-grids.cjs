// Shared analyzer grid, header-filter and automatic last-view regression.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const BASE=process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD=process.env.WORLD || 'Gilgamesh';
const routes=[
  ['flip-finder',`/flip-finder/${WORLD}`,'profit','profit'],
  ['recipe-analyzer','/recipe-analyzer','profit','profit'],
  ['leve-analyzer','/leve-analyzer','profit','profit'],
  ['venture-analyzer','/venture-analyzer','profit','profit'],
  ['vendor-resale',`/vendor-resale/${WORLD}`,'profit','profit'],
  ['scrip-sources','/scrip-sources','scrip-type','scrip'],
  ['fc-crafting-analyzer',`/fc-crafting-analyzer/${WORLD}`,'profit','profit'],
];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function main(){
  const dir=path.join(__dirname,'artifacts','analyzer-grids');fs.mkdirSync(dir,{recursive:true});
  const profile=fs.mkdtempSync(path.join(dir,'profile-'));
  const launch=()=>puppeteer.launch({headless:'new',args:['--no-sandbox'],userDataDir:profile});
  let browser=await launch();
  const errors=[];
  async function newPage(mobile=false){
    const page=await browser.newPage();page.setDefaultTimeout(90000);
    await page.setViewport({width:mobile?393:1280,height:850,isMobile:mobile,hasTouch:mobile});
    await page.evaluateOnNewDocument(()=>window.addEventListener('ultros:hydrated',()=>window.__hydrated=true));
    page.on('pageerror',e=>errors.push(e.message));
    page.on('console',m=>{if(m.type()==='error'&&!/favicon|ERR_BLOCKED_BY_CLIENT|net::ERR_ABORTED/.test(m.text()))errors.push(m.text());});
    await page.setCookie({name:'HOME_WORLD',value:WORLD,url:BASE},{name:'HIDE_ADS',value:'true',url:BASE});
    return page;
  }
  async function goto(page,url){
    console.log(`Opening ${url}`);
    const response=await page.goto(new URL(url,BASE).href,{waitUntil:'domcontentloaded',timeout:90000});
    assert(response.ok(),`${url}: HTTP ${response.status()}`);
    try {
      await page.waitForFunction(()=>window.__hydrated);
      await page.waitForSelector('.virtual-grid');
    }
    catch (error) {
      await page.screenshot({path:path.join(dir,'failure.png'),fullPage:true});
      console.error('Page state:',page.url(),await page.$eval('body',e=>e.innerText.slice(0,4000)),errors);
      throw error;
    }
    await sleep(250);
    return response;
  }
  async function aligned(page){
    const results=await page.$eval('.virtual-grid',g=>{
      const headings=[...g.querySelectorAll('.virtual-grid-heading')];
      const rows=[...g.querySelectorAll('.virtual-grid-cell')];
      return {
        errors:rows.flatMap(cell=>{
          const h=headings.find(h=>h.dataset.column===cell.dataset.column);if(!h)return [];
          const a=cell.getBoundingClientRect(),b=h.getBoundingClientRect();
          return Math.abs(a.left-b.left)>1||Math.abs(a.width-b.width)>1?[cell.dataset.column]:[];
        }),
        cells:rows.length,
        nested:[...g.querySelectorAll('*')].filter(e=>e.scrollWidth>e.clientWidth+1&&/auto|scroll/.test(getComputedStyle(e).overflowX)).length,
      };
    });
    assert.deepEqual(results.errors,[],'all mounted cells line up with their headings');
    assert(results.cells<1000,'both axes keep the mounted DOM bounded');
    assert.equal(results.nested,0,'only the grid owns horizontal scrolling');
  }
  try{
    for(const [tool,route,column,filter] of routes){
      const page=await newPage();
      const world=tool==='flip-finder'?'':`&world=${WORLD}`;
      await goto(page,`${route}?v=1&min-sales=0&l=2~~item.8c${world}`);
      await aligned(page);
      assert(Math.abs(await page.$eval('.virtual-grid-heading[data-column="item"]',e=>e.getBoundingClientRect().width)-300)<1);
      const menu=`.virtual-grid-heading[data-column="${column}"] .grid-column-menu`;
      await page.$eval(menu,e=>e.scrollIntoView({block:'center',inline:'nearest'}));
      await page.click(menu);
      const form=`.grid-column-filter[data-filter="${filter}"]`;
      await page.waitForSelector(form);
      const selected=filter==='scrip'?'OrangeCrafters':'100';
      if(filter==='scrip')await page.select(`${form} select`,selected);
      else {await page.click(`${form} input`,{clickCount:3});await page.type(`${form} input`,selected);}
      const display=filter==='scrip'?await page.$eval(form+' select',e=>e.selectedOptions[0].textContent):selected;
      await page.click(`${form} button[type="submit"]`);
      await page.waitForFunction((filter,value)=>new URL(location.href).searchParams.get(filter)===value,{},filter,selected);
      await page.waitForSelector(`.virtual-grid-heading[data-column="${column}"].grid-filter-active`);
      await page.waitForFunction(display=>[...document.querySelectorAll('.filter-chip')].some(e=>e.textContent.includes(display)),{},display);
      await page.keyboard.press('Escape');
       await page.waitForFunction(tool=>localStorage.getItem(`ultros.last-view.${tool}`)?.includes('l=2'),{},tool);
       assert.equal(await page.evaluate(tool=>new URLSearchParams(localStorage.getItem(`ultros.last-view.${tool}`)).getAll('v').length,tool),1,'repeated saves do not accumulate version parameters');
      const cookies=await page.cookies();const cookie=cookies.find(c=>c.name===`ultros_last_${tool}`);
      assert(cookie && cookie.path===`/${tool}` && cookie.expires>Date.now()/1000,'view has a persistent, analyzer-scoped cookie');
      assert(cookie.value.length<3500);
      // A bare return is restored by HTTP before the SSR document is generated.
      const response=await goto(page,`${route}?lang=ja${world}`);
      assert(response.request().redirectChain().some(r=>r.response()?.status()===307),'bare entry is restored before SSR');
      assert.equal(new URL(page.url()).searchParams.get(filter),selected);
      assert.equal(new URL(page.url()).searchParams.get('lang'),'ja');
      assert.equal(new URL(page.url()).searchParams.get('l'),'2~~item.8c');
      // Explicit shared links override this device's last view.
      const explicit=filter==='scrip'?'PurpleCrafters':'200';
      await goto(page,`${route}?v=1&${filter}=${explicit}&lang=en${world}`);
      assert.equal(new URL(page.url()).searchParams.get(filter),explicit);
      assert(!new URL(page.url()).searchParams.has('l'));
       await page.setViewport({width:393,height:850,isMobile:true,hasTouch:true});
       await page.waitForFunction(()=>window.__hydrated);
       await page.waitForSelector('.virtual-grid');
       await page.$eval('.virtual-grid',e=>e.scrollIntoView({block:'start'}));
       assert(await page.evaluate(()=>document.querySelector('.sticky-bar').getBoundingClientRect().bottom<=document.querySelector('.virtual-grid').getBoundingClientRect().top+1),'the page toolbar must not cover the grid headings');
      const grid=await page.$('.virtual-grid');const bounds=await grid.boundingBox();
      const cdp=await page.createCDPSession();
      const x=bounds.x+Math.min(bounds.width-30,300),y=Math.min(650,bounds.y+180);
      await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x,y}]});
      for(let i=1;i<=12;i++){await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:x-i*15,y:y-i*4}]});await sleep(16);}
      await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await sleep(350);
      await aligned(page);
      await page.screenshot({path:path.join(dir,`${tool}-mobile.png`),fullPage:true});
      await page.close();
      console.log(`PASS ${tool}: header filter, cookie restore, explicit URL, mobile alignment`);
    }
    // Closing and reopening the browser keeps the last customization without Save view.
    await browser.close();browser=await launch();
    const page=await newPage();
    await goto(page,`/recipe-analyzer?world=${WORLD}`);
    assert.equal(new URL(page.url()).searchParams.get('profit'),'200');
    // localStorage remains the fallback when a cookie is absent (e.g. an oversized view).
    await page.deleteCookie({name:'ultros_last_recipe-analyzer',url:BASE+'/recipe-analyzer',path:'/recipe-analyzer'});
    await goto(page,`/recipe-analyzer?world=${WORLD}`);
    await page.waitForFunction(()=>new URL(location.href).searchParams.get('profit')==='200');
    await page.goto(BASE,{waitUntil:'domcontentloaded'});
    await page.waitForFunction(()=>window.__hydrated);
    await page.click('.side-nav a[href*="recipe-analyzer"]');
    await page.waitForFunction(()=>location.pathname==='/recipe-analyzer' && new URL(location.href).searchParams.get('profit')==='200');
    // An explicitly cleared view must stay cleared on the next visit.
    await goto(page,`/recipe-analyzer?v=1&world=${WORLD}`);
    await page.waitForFunction(()=>localStorage.getItem('ultros.last-view.recipe-analyzer')==='?v=1');
    await goto(page,`/recipe-analyzer?world=${WORLD}`);
    assert.equal(new URL(page.url()).searchParams.get('v'),'1');
    assert(!new URL(page.url()).searchParams.has('profit'));
    assert(!new URL(page.url()).searchParams.has('min-sales'));
    assert.deepEqual(errors,[]);
    console.log('PASS cross-session and localStorage fallback restoration');
  } finally {await browser.close();}
}
main().catch(e=>{console.error(e);process.exitCode=1;});
