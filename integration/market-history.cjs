// Full chart regression: use a local market with sales and listing observations.
// BASE_URL=http://127.0.0.1:18180 node integration/market-history.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');

(async () => {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  const errors = [], requests = [];
  let floorDelay = 0, saleDelay = 0, floorFailure = false;
  await page.setRequestInterception(true);
  page.on('request', async request => {
    if (request.isInterceptResolutionHandled()) return;
    if (request.url().includes('/api/v1/floor_history/') && floorFailure) {
      return request.respond({status: 503, contentType: 'application/json', body: '{"error":"fixture unavailable"}'});
    }
    const delay = request.url().includes('/api/v1/floor_history/') ? floorDelay
      : request.url().includes('/api/v1/price_series/') ? saleDelay : 0;
    if (delay) await new Promise(resolve => setTimeout(resolve, delay));
    if (!request.isInterceptResolutionHandled()) await request.continue();
  });
  page.on('pageerror', e => {
    const text = e.stack || e.message;
    // Same vendor-only exception as runner.cjs: AdSense throws in headless Chrome.
    if (!text.includes('https://pagead2.googlesyndication.com/')) errors.push(text);
  });
  page.on('request', r => {
    if (/\/api\/v1\/(price_series|floor_history|price_density)\//.test(r.url())) requests.push(new URL(r.url()));
  });
  const base = process.env.BASE_URL || 'http://127.0.0.1:8080';
  const route = `/item/${process.env.WORLD || 'Gilgamesh'}/${process.env.ITEM_ID || '46010'}`;
  const artifacts = path.join(__dirname, 'artifacts');
  fs.mkdirSync(artifacts, { recursive: true });
  const modeButton = name => `.market-history button[aria-label="${name}"]`;
  const floorVisible = () => [...document.querySelectorAll('.price-history-chart path')]
    .some(p => getComputedStyle(p).stroke === 'rgb(77, 224, 193)');
  async function chooseMode(label, value) {
    await page.click(modeButton(label));
    await page.waitForFunction((label, value) =>
      document.querySelector(`.market-history button[aria-label="${label}"]`).getAttribute('aria-pressed') === 'true'
      && new URL(location.href).searchParams.get('mode') === value, {}, label, value);
  }
  async function clickText(text) {
    await page.evaluate(text => {
      const el = [...document.querySelectorAll('.market-history button')].find(b => b.textContent.trim() === text);
      if (!el) throw new Error(`Missing chart control ${text}`);
      el.click();
    }, text);
  }
  async function screenshot(name) {
    const panel = await page.$('.market-history');
    await panel.scrollIntoView();
    await panel.screenshot({ path: path.join(artifacts, name) });
  }
  try {
    await page.setViewport({ width: 1440, height: 1700, deviceScaleFactor: 2 });
    await page.goto(base + route + '?range=1mo', { waitUntil: 'networkidle0', timeout: 120000 });
    await page.waitForFunction(floorVisible, { timeout: 60000 });
    for (const label of ['Price line', 'Candlesticks', 'Price range', 'Sale density', 'Overlay', 'Grid']) {
      assert(await page.$(modeButton(label)), `${label} stays directly available`);
    }
    assert.equal(await page.$('.market-history + details'), null, 'no second chart hidden in a disclosure');
    const units = await page.$eval('.mh-stat:last-child strong', el => el.textContent);
    await chooseMode('Candlesticks', 'candles');
    await page.waitForFunction(floorVisible);
    assert(await page.$('.price-history-chart path[fill]:not([fill="none"])'), 'real candle bodies render');
    assert.equal(await page.$eval('.mh-stat:last-child strong', el => el.textContent), units);
    await screenshot('market-history-candles-desktop.png');

    await page.click('.mh-ask');
    await page.waitForFunction(() => new URL(location.href).searchParams.get('floor') === 'false');
    assert.equal(await page.evaluate(floorVisible), false, 'floor toggle removes the listing path');
    assert(await page.$('.price-history-chart path[fill]:not([fill="none"])'), 'hiding floor preserves candles');
    await page.click('.mh-ask');
    await page.waitForFunction(floorVisible);
    await page.focus('.price-history-chart');
    await page.keyboard.press('ArrowRight');
    await page.waitForSelector('.market-chart-tooltip');
    assert.match(await page.$eval('.market-chart-tooltip', el => el.textContent), /Lowest listing/);
    await page.keyboard.press('Escape');
    await page.waitForFunction(() => !document.querySelector('.market-chart-tooltip'));

    await chooseMode('Price range', 'range');
    await page.waitForFunction(floorVisible);
    await page.click(modeButton('Grid'));
    await page.waitForFunction(() => new URL(location.href).searchParams.get('view') === 'grid');
    assert(await page.$eval('.mh-ask', el => el.disabled));
    assert.match(await page.$eval('.mh-floor-reason', el => el.textContent), /whole selected market/);
    await page.click(modeButton('Overlay'));
    await page.waitForFunction(floorVisible);
    await chooseMode('Sale density', 'density');
    await page.waitForNetworkIdle({ idleTime: 600 });
    assert(requests.some(u => u.pathname.includes('/price_density/')), 'density still fetches its own data');
    assert(await page.$eval('.mh-ask', el => el.disabled));
    await chooseMode('Price line', 'price');
    await page.waitForFunction(floorVisible);
    // Exercise the existing overlays popover and percent index.
    await page.evaluate(() => [...document.querySelectorAll('.market-chart-toolbar button')]
      .find(b => b.textContent.includes('Overlays')).click());
    await page.evaluate(() => [...document.querySelectorAll('.market-chart-toolbar label')]
      .find(l => l.textContent.includes('Index to % change')).querySelector('input').click());
    await page.waitForFunction(() => document.querySelector('.mh-ask').disabled);
    assert.match(await page.$eval('.mh-floor-reason', el => el.textContent), /% change/);
    await page.evaluate(() => [...document.querySelectorAll('.market-chart-toolbar label')]
      .find(l => l.textContent.includes('Index to % change')).querySelector('input').click());
    await page.click('.mh-stats');
    await page.waitForFunction(floorVisible);

    floorDelay = 1200; // Sales finish first: stale listing bounds must not reset the preset.
    await clickText('7d');
    await page.waitForFunction(() => new URL(location.href).searchParams.get('range') === '7d');
    await page.waitForNetworkIdle({ idleTime: 600 });
    assert.equal(new URL(page.url()).searchParams.get('range'), '7d', 'loading either data source must preserve the chosen range');
    const sale = requests.findLast(u => u.pathname.includes('/price_series/'));
    const floor = requests.findLast(u => u.pathname.includes('/floor_history/'));
    assert(sale && floor);
    for (const key of ['from', 'to', 'hq']) assert.equal(sale.searchParams.get(key), floor.searchParams.get(key));
    floorDelay = 0;
    saleDelay = 1200; // Reverse the response order during a manual slice.
    const handle = await page.$('button[aria-label="Adjust slice start"]');
    await handle.scrollIntoView();
    const start = await handle.boundingBox();
    const trackWidth = await handle.evaluate(el => el.parentElement.getBoundingClientRect().width);
    await page.mouse.move(start.x + start.width / 2, start.y + start.height / 2);
    await page.mouse.down();
    await page.mouse.move(start.x + trackWidth * 0.15, start.y + start.height / 2, { steps: 5 });
    await page.mouse.up();
    await page.waitForFunction(() => {
      const q = new URL(location.href).searchParams;
      return q.has('from') && q.has('to') && !q.has('range');
    });
    await page.waitForNetworkIdle({ idleTime: 600 });
    const draggedSales = requests.findLast(u => u.pathname.includes('/price_series/'));
    const draggedFloor = requests.findLast(u => u.pathname.includes('/floor_history/'));
    for (const key of ['from', 'to']) assert.equal(draggedSales.searchParams.get(key), draggedFloor.searchParams.get(key));
    saleDelay = 0;
    const draggedBounds = ['from', 'to'].map(k => new URL(page.url()).searchParams.get(k));
    assert(draggedBounds.every(v => v !== null), 'custom bounds survive both resource responses');
    await chooseMode('Candlesticks', 'candles');
    // Shared presentation state must survive a direct load.
    await page.reload({ waitUntil: 'networkidle0', timeout: 120000 });
    await page.waitForFunction(floorVisible);
    assert.equal(await page.$eval(modeButton('Candlesticks'), el => el.getAttribute('aria-pressed')), 'true');
    assert.deepEqual(['from', 'to'].map(k => new URL(page.url()).searchParams.get(k)), draggedBounds, 'shared custom window survives hydration');

    await page.setViewport({ width: 390, height: 1700, deviceScaleFactor: 2, hasTouch: true });
    await page.waitForFunction(floorVisible, { timeout: 60000 });
    await page.waitForFunction(() => document.querySelector('.price-history-chart svg').viewBox.baseVal.width < 500);
    assert.deepEqual(['from', 'to'].map(k => new URL(page.url()).searchParams.get(k)), draggedBounds, 'custom window survives mobile hydration too');
    const overflow = await page.$eval('.market-history', el => el.scrollWidth - el.clientWidth);
    assert(overflow <= 1, `market chart overflows mobile by ${overflow}px`);
    await screenshot('market-history-candles-mobile.png');
    const plot = await page.$('.price-history-chart');
    await plot.scrollIntoView();
    const box = await plot.boundingBox();
    await page.touchscreen.tap(box.x + box.width * 0.5, box.y + box.height * 0.4);
    await page.waitForSelector('.market-chart-tooltip');
    await new Promise(resolve => setTimeout(resolve, 150));
    assert(await page.$('.market-chart-tooltip'), 'touch cursor survives finger lift');
    await page.touchscreen.tap(5, 5);
    await page.waitForFunction(() => !document.querySelector('.market-chart-tooltip'));
    floorFailure = true;
    await clickText('1y');
    await page.waitForFunction(() => document.querySelector('.mh-data-note').textContent.includes('temporarily unavailable'));
    assert(await page.$('.price-history-chart svg'), 'sale chart survives listing-history failure');
    assert.deepEqual(errors, [], 'browser runtime errors');
    console.log('Market history passed: all four modes, real candles + listing floor, grid, percent overlays, ranges, shared URLs, keyboard/touch, mobile layout, and failure fallback.');
  } finally { await browser.close(); }
})().catch(e => { console.error(e); process.exitCode = 1; });
