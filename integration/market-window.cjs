// Deterministic provider/window races through the real SSR + hydrated MarketGrid.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const fs = require('node:fs');
const path = require('node:path');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const fixture = '/__test/shared-analyzer-data';

async function main() {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(30000);
  await page.setCookie({ name: 'HIDE_ADS', value: 'true', url: BASE });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    const url = message.location().url || '';
    if (message.type() === 'error' && !url.includes('/sale_stats/Cactuar?window=1')
      && !/favicon|ERR_BLOCKED_BY_CLIENT|net::ERR_ABORTED/.test(message.text())) errors.push(message.text());
  });
  const hits = new Map();
  const held = new Map();
  const hold = new Set();
  await page.setRequestInterception(true);
  page.on('request', request => {
    const url = new URL(request.url());
    if (!url.pathname.startsWith('/api/v1/sale_stats/')) return request.continue();
    const scope = decodeURIComponent(url.pathname.split('/').at(-1));
    const days = Number(url.searchParams.get('window'));
    const key = `${scope}/${days}`;
    hits.set(key, (hits.get(key) || 0) + 1);
    const multiplier = scope === 'Cactuar' ? 3 : 1;
    const prices = days === 7 ? [70, 140] : days === 30 ? [300, 150] : [900, 450];
    const body = { stats: prices.map((price, index) => ({ item_id: 42 + index, hq: false,
      min_price: price * multiplier - 10, median_price: price * multiplier,
      avg_price: price * multiplier + 10, num_sold: days * 2, units_sold: days * 4,
      vwap: price * multiplier, sales_per_day: 2, gil_volume: price * days,
      last_sold_unix: 1788900000, confidence: 'high',
    })) };
    const respond = () => request.respond({ status: days === 1 ? 503 : 200,
      contentType: 'application/json', body: JSON.stringify(body) }).catch(() => {});
    if (hold.has(key)) held.set(key, respond); else return respond();
  });
  await page.evaluateOnNewDocument(() => {
    window.addEventListener('ultros:hydrated', () => { window.__windowHydrated = true; });
  });
  const selector = column => `.virtual-grid-cell[data-column="${column}"]`;
  async function cell(column, text) {
    await page.waitForFunction((column, text) => [...document.querySelectorAll(`.virtual-grid-cell[data-column="${column}"]`)]
      .some(cell => cell.textContent.trim() === text), {}, column, text);
  }
  async function heading(column, text) {
    await page.waitForFunction((column, text) => document.querySelector(`.virtual-grid-heading[data-column="${column}"]`)?.textContent.includes(text), {}, column, text);
  }
  async function query(changes) {
    await page.evaluate(changes => {
      const url = new URL(location.href);
      for (const [key, value] of Object.entries(changes)) value === null ? url.searchParams.delete(key) : url.searchParams.set(key, value);
      const a = document.createElement('a'); a.href = url.href; document.querySelector('main').append(a); a.click(); a.remove();
    }, changes);
    await page.waitForFunction(changes => Object.entries(changes).every(([key, value]) => new URL(location.href).searchParams.get(key) === value), {}, changes);
  }
  async function rows(count) {
    await page.waitForFunction(count => Number(document.querySelector('.virtual-grid')?.getAttribute('aria-rowcount')) === count + 1, {}, count);
  }
  async function first(id) {
    await page.waitForFunction(id => document.querySelector('.virtual-grid-cell[data-column="item"] [data-window-item]')?.dataset.windowItem === String(id), {}, id);
  }
  async function release(key) {
    assert(held.has(key), `${key} request was held`);
    await held.get(key)(); held.delete(key); hold.delete(key);
  }
  try {
    await page.setViewport({ width: 1600, height: 900 });
    const params = new URLSearchParams({ 'market-window-test': '1', lang: 'en', v: '1',
      cols: 'market-sale-median,market-sale-median-7', sort: 'grid:market-sale-median', dir: 'asc' });
    await page.goto(`${BASE}${fixture}?${params}`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__windowHydrated);
    await cell('market-sale-median', '70');
    await heading('market-sale-median', '(7d)');
    await cell('price', '100');
    assert.equal(hits.get('Gilgamesh/7'), 1, 'default and follow/fixed columns share one request');
    assert.equal(hits.get('Gilgamesh/30'), undefined);
    await page.click('.virtual-grid-heading[data-column="item"]', { button: 'right' });
    for (const button of await page.$$('.grid-menu-panel button')) {
      if ((await button.evaluate(el => el.textContent.trim())).startsWith('Insert column after')) { await button.click(); break; }
    }
    await page.waitForSelector('[data-column-picker-group]');
    const groups = await page.$$eval('[data-column-picker-group]', labels => labels.map(el => el.textContent));
    assert(groups.includes('Sale history (selected window)'));
    assert(groups.includes('Sale history (7d)'));
    await page.keyboard.press('Escape');
    await page.select('select:has(option[value="sale-median"])', 'sale-median');
    await cell('price', '70');

    hold.add('Gilgamesh/30');
    await page.select('[data-market-window]', '30');
    await heading('market-sale-median', '(30d)');
    await heading('market-sale-median-7', '(7d)');
    await first(42); // pending bulk data must not apply a partial global sort
    await query({ gf: JSON.stringify({ price: { op: 'gte', value: '200' } }) });
    await rows(3); // listing fallbacks must not prematurely filter pending prices
    await page.select('[data-market-window]', '7');
    await heading('market-sale-median', '(7d)');
    await page.select('[data-market-window]', '30');
    await rows(3);
    await release('Gilgamesh/30');
    await rows(1);
    await cell('price', '300');
    await cell('market-sale-median-7', '70');
    assert.equal(hits.get('Gilgamesh/30'), 1, 'switching back while loading reuses the same slot');
    await page.click('[data-grid-query-summary] a');
    await rows(3);
    assert.equal(new URL(page.url()).searchParams.get('window'), '30', 'clear filters preserves view window');
    assert.equal(new URL(page.url()).searchParams.get('revenue'), 'sale-median');
    await first(43);

    // A hidden filter and hidden sort still request their separate bodies.
    await query({ cols: '', gf: JSON.stringify({ 'market-sale-median-90': { op: 'present' } }), sort: 'grid:market-sale-median' });
    await rows(2);
    assert.equal(hits.get('Gilgamesh/90'), 1);
    assert.equal(hits.get('Gilgamesh/30'), 1);
    await query({ gf: null, cols: 'market-sale-median,market-sale-median-7' });
    await rows(3);

    // Save/reload/restore retains the page window alongside basis and grid state.
    await page.click('[data-grid-saved-views] > button');
    await page.type('[data-grid-saved-views] form input', 'Thirty days');
    await page.click('[data-grid-saved-views] form button[type="submit"]');
    await page.waitForFunction(() => JSON.parse(localStorage.getItem('ultros.grid.window-fixture-grid.views') || '[]').some(view => view.name === 'Thirty days'));
    await page.keyboard.press('Escape');
    await page.select('[data-market-window]', '7');
    await page.click('[data-grid-saved-views] > button');
    await page.click('[data-grid-saved-views] a');
    await heading('market-sale-median', '(30d)');
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__windowHydrated);
    await cell('price', '300');
    assert.equal(await page.$eval('[data-market-window]', el => el.value), '30');
    const ssr = await browser.newPage();
    await ssr.setJavaScriptEnabled(false);
    await ssr.goto(page.url(), { waitUntil: 'domcontentloaded' });
    assert.match(await ssr.$eval('body', el => el.textContent), /Sale median \(30d\)/);
    assert.equal(await ssr.$eval(selector('price'), el => el.textContent.trim()), '100', 'SSR uses listing fallback before client data');
    await ssr.close();

    hold.add('Gilgamesh/90'); hold.add('Cactuar/90');
    await Promise.all([
      page.waitForRequest(request => request.url().includes('sale_stats/Gilgamesh?window=90')),
      page.select('[data-market-window]', '90'),
    ]);
    await Promise.all([page.waitForRequest(request => request.url().includes('sale_stats/Cactuar?window=90')), page.click('#window-scope')]);
    await release('Cactuar/90');
    await cell('market-sale-median', '2,700');
    await Promise.all([
      page.waitForResponse(response => response.url().includes('sale_stats/Gilgamesh?window=90')).then(response => response.json()),
      release('Gilgamesh/90'),
    ]);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await cell('price', '2700');
    await cell('market-sale-median-7', '210');
    await Promise.all([page.waitForResponse(response => response.url().includes('sale_stats/Cactuar?window=1')), page.select('[data-market-window]', '1')]);
    await cell('market-sale-median', '—');
    await query({ gf: JSON.stringify({ 'market-sale-median': { op: 'present' } }) });
    await rows(3); // unavailable is unknown, not an empty successful body
    await query({ gf: null });
    await cell('price', '100'); // failures retain pricing fallback without masquerading as no history
    await page.select('[data-market-window]', '30');
    await cell('market-sale-median', '900');
    await rows(3); // fixture row 44 has missing stats and keeps its listing
    // Trends shares the control while keeping its narrower choices and 30d default.
    await page.goto(`${BASE}/trends/Gilgamesh?v=1&lang=en`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__windowHydrated);
    assert.equal(await page.$eval('[data-market-window]', el => el.value), '30');
    assert.deepEqual(await page.$$eval('[data-market-window] option', options => options.map(el => el.value)), ['7', '30', '90']);
    await Promise.all([
      page.waitForRequest(request => {
        const url = new URL(request.url());
        return url.pathname === '/api/v1/trends/Gilgamesh' && url.searchParams.get('window') === '7';
      }),
      page.select('[data-market-window]', '7'),
    ]);
    await page.waitForFunction(() => new URL(location.href).searchParams.get('window') === '7');
    assert.deepEqual(errors, []);
    console.log('PASS market windows: defaults, pinned comparisons, prices, pending filters/sorts, hidden requirements, deduplication, saved URLs, SSR, scope/window races, failures and missing rows');
  } catch (error) {
    const artifacts = path.join(__dirname, 'artifacts', 'market-window');
    fs.mkdirSync(artifacts, { recursive: true });
    await page.screenshot({ path: path.join(artifacts, 'failure.png'), fullPage: true }).catch(() => {});
    console.error('Failure URL:', page.url(), 'Browser errors:', errors);
    throw error;
  } finally { await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
