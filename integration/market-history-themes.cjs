// Theme regression for the real item page. Uses the same local data as market-history.cjs.
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const puppeteer = require('puppeteer');
(async () => {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  const base = process.env.BASE_URL || 'http://127.0.0.1:18180';
  const artifacts = path.join(__dirname, 'artifacts');
  fs.mkdirSync(artifacts, {recursive:true});
  try {
    await page.setViewport({width:1920,height:1450,deviceScaleFactor:1.5});
    await page.goto(base + '/item/Gilgamesh/46010?mode=candles&range=1mo', {waitUntil:'networkidle0',timeout:120000});
    await page.waitForSelector('.price-history-chart path[stroke="var(--mh-floor)"]', {timeout:60000});
    assert.equal(await page.$('.mh-heading'), null, 'no duplicate promotional header');
    assert(await page.$('#listings'), 'real listings section stays present');
    assert(await page.$('#history'), 'real recent-sales section stays present');
    assert(await page.$('#market-history .market-history'), 'chart lives in the item page card');
    const originalGeometry = await page.$eval('.price-history-chart path[stroke="var(--mh-floor)"]', p => p.getAttribute('d'));
    // Exercise every supported palette in each mode without rebuilding data.
    const palettes = ['ultros','maelstrom','twin-adder','ascian','ishgard','crystarium','sharlayan','tuliyollal','immortal-flames','uldah','limsa','garlemald'];
    for (const mode of ['dark','light']) for (const palette of palettes) {
      await page.evaluate(({mode,palette}) => {
        document.documentElement.dataset.theme = mode;
        document.documentElement.dataset.palette = palette;
      }, {mode,palette});
      // Palette switches animate the existing buttons. Measure the settled
      // colors, rather than an intermediate frame from the previous palette.
      await page.evaluate(async () => {
        const controls = [...document.querySelectorAll('.market-history button')];
        controls.forEach(el => void getComputedStyle(el).color);
        await Promise.all(controls.flatMap(el => el.getAnimations())
          .filter(a => a.effect?.getTiming().iterations !== Infinity)
          .map(a => a.finished.catch(() => {})));
      });
      const state = await page.evaluate(() => {
        const floor = document.querySelector('.price-history-chart path[stroke="var(--mh-floor)"]');
        const chart = document.querySelector('.price-history-chart');
        const text = chart.querySelector('text');
        const ctx = document.createElement('canvas').getContext('2d');
        function luminance(color) {
          ctx.clearRect(0,0,1,1); ctx.fillStyle=color; ctx.fillRect(0,0,1,1);
          const channels = [...ctx.getImageData(0,0,1,1).data].slice(0,3).map(v => {
            const c=v/255; return c<=0.04045 ? c/12.92 : ((c+0.055)/1.055)**2.4;
          });
          return channels[0]*0.2126+channels[1]*0.7152+channels[2]*0.0722;
        }
        const background = luminance(getComputedStyle(document.querySelector('#market-history')).backgroundColor);
        function contrast(color) {
          const value=luminance(color); return (Math.max(value,background)+0.05)/(Math.min(value,background)+0.05);
        }
        const controls = [...document.querySelectorAll('.market-history button[aria-pressed="true"]:not(.mh-layer)')];
        const controlContrast = Math.min(...controls.map(el => {
          const style=getComputedStyle(el), fg=luminance(style.color), bg=luminance(style.backgroundColor);
          return (Math.max(fg,bg)+0.05)/(Math.min(fg,bg)+0.05);
        }));
        return {controlContrast, floor:getComputedStyle(floor).stroke, textContrast:contrast(getComputedStyle(text).fill), floorContrast:contrast(getComputedStyle(floor).stroke), geometry:floor.getAttribute('d'), overflow:document.documentElement.scrollWidth-innerWidth};
      });
      assert.equal(state.geometry, originalGeometry, 'theme changes must preserve price geometry');
      assert.equal(state.floor, mode === 'light' ? 'rgb(8, 123, 104)' : 'rgb(77, 224, 193)');
      assert(state.controlContrast >= 4.5, `${mode}/${palette} selected control contrast ${state.controlContrast}`);
      assert(state.textContrast >= 4.5, `${mode}/${palette} axis text contrast ${state.textContrast}`);
      assert(state.floorContrast >= 3, `${mode}/${palette} listing stroke contrast ${state.floorContrast}`);
      assert(state.overflow <= 1, `${mode}/${palette} overflow`);
    }
    // Reload persisted themes too, covering SSR -> hydration for the screenshots.
    for (const [mode,palette] of [['dark','ultros'],['light','ultros'],['dark','ishgard'],['dark','maelstrom']]) {
      await page.evaluate(({mode,palette}) => {
        localStorage.setItem('theme.mode',mode); localStorage.setItem('theme.palette',palette);
        document.cookie=`theme_mode=${mode}; path=/`; document.cookie=`theme_palette=${palette}; path=/`;
      }, {mode,palette});
      await page.reload({waitUntil:'networkidle0',timeout:120000});
      await page.waitForSelector('.price-history-chart path[stroke="var(--mh-floor)"]', {timeout:60000});
      const actual = await page.evaluate(() => [document.documentElement.dataset.theme,document.documentElement.dataset.palette]);
      assert.deepEqual(actual,[mode,palette]);
      await page.$eval('#market-history', el => scrollTo(0,el.getBoundingClientRect().top + scrollY - 150));
      await page.screenshot({path:path.join(artifacts,`market-history-${palette}-${mode}-context.png`)});
      if (mode === 'dark' && palette === 'ultros') {
        await page.setViewport({width:2560,height:2050,deviceScaleFactor:1});
        await page.evaluate(() => scrollTo(0,0));
        await page.waitForNetworkIdle({idleTime:500});
        const pageHeight = await page.$eval('#market-history', el => Math.ceil(el.getBoundingClientRect().bottom + scrollY + 24));
        await page.setViewport({width:2560,height:pageHeight,deviceScaleFactor:1});
        await page.screenshot({path:path.join(artifacts,'market-history-item-page.png')});
        await page.setViewport({width:1920,height:1450,deviceScaleFactor:1.5});
      }
      if (mode === 'light') {
        await page.setViewport({width:390,height:1100,deviceScaleFactor:2});
        await page.reload({waitUntil:'networkidle0',timeout:120000});
        await page.waitForSelector('.price-history-chart path[stroke="var(--mh-floor)"]', {timeout:60000});
        await page.$eval('#market-history', el => el.scrollIntoView());
        assert(await page.evaluate(() => document.documentElement.scrollWidth-innerWidth <= 1));
        await page.screenshot({path:path.join(artifacts,'market-history-light-mobile.png')});
        await page.setViewport({width:1920,height:1450,deviceScaleFactor:1.5});
      }
    }
    console.log('PASS: 24 theme/palette combinations, geometry preservation, persisted theme hydration, real page integration, and mobile overflow');
  } finally { await browser.close(); }
})().catch(e => {console.error(e);process.exit(1);});
