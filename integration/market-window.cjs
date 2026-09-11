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
  const expectedFailures = new Set();
  const isExpectedFailure = value => {
    const url = new URL(value, BASE);
    return url.origin === new URL(BASE).origin && (
      url.pathname === '/api/v1/listing_stats/Cactuar' && !url.search
      || ['/api/v1/sale_stats/Cactuar', '/api/v1/sale_stats/Gilgamesh'].includes(url.pathname)
        && url.search === '?window=1');
  };
  page.on('response', response => {
    if (response.status() >= 500) {
      if (response.status() === 503 && isExpectedFailure(response.url())) expectedFailures.add(response.url());
      else errors.push(`Unexpected HTTP ${response.status()}: ${response.url()}`);
    }
  });
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    const url = message.location().url || '';
    if (message.type() === 'error' && !(isExpectedFailure(url) && /Failed to load resource:.*503/.test(message.text()))
      && !/favicon|ERR_BLOCKED_BY_CLIENT|net::ERR_ABORTED/.test(message.text())) errors.push(message.text());
  });
  const hits = new Map();
  const listingHits = new Map();
  let holdListings = false;
  const heldListings = [];
  let listingWindowed = false;
  const held = new Map();
  const hold = new Set();
  await page.setRequestInterception(true);
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.pathname.startsWith('/api/v1/listing_stats/')) {
      // The alive set is one window-free body per scope. Row 42 has three
      // listings, row 43 an empty board, and row 44 is absent; Cactuar fails.
      const scope = decodeURIComponent(url.pathname.split('/').at(-1));
      listingHits.set(scope, (listingHits.get(scope) || 0) + 1);
      if (url.searchParams.has('window')) listingWindowed = true;
      const now = Math.floor(Date.now() / 1000);
      const body = { stats: [
        { item_id: 42, hq: false, alive_count: 3, alive_units: 30, distinct_retainers: 2,
          oldest_reviewed_unix: now - 90000, median_age_secs: 3600, floor_alive: 100 },
        { item_id: 43, hq: false, alive_count: 0, alive_units: 0, distinct_retainers: 0,
          oldest_reviewed_unix: 0, median_age_secs: 0, floor_alive: 0 },
      ] };
      if (holdListings) {
        heldListings.push({ scope, respond: count => {
          body.stats[0].alive_count = count;
          return request.respond({ status: 200, contentType: 'application/json', body: JSON.stringify(body) });
        } });
        return;
      }
      return request.respond(scope === 'Cactuar'
        ? { status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'Listing statistics temporarily unavailable' }) }
        : { status: 200, contentType: 'application/json', body: JSON.stringify(body) }).catch(() => {});
    }
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
      contentType: 'application/json', body: JSON.stringify(days === 1 ? { error: 'Sale statistics temporarily unavailable' } : body) }).catch(() => {});
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
    // Follow-window columns request their own body even with listing pricing.
    hold.add('Gilgamesh/30');
    await Promise.all([
      page.waitForRequest(request => request.url().includes('sale_stats/Gilgamesh?window=30')),
      page.select('[data-market-window]', '30'),
    ]);
    await heading('market-sale-median', '(30d)');
    await cell('price', '100');
    await page.select('[data-market-window]', '7');
    await heading('market-sale-median', '(7d)');
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
    // The shared bar owns the only filter surface (#1351); Clear all is its button.
    assert.equal(await page.$('[data-grid-query-summary]'), null, 'the grid renders no filter strip of its own');
    await page.click('.registered-filter-bar button[aria-label="Clear all filters"]');
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
    // Current-listing columns fetch one window-free body, only once wanted.
    assert.equal(listingHits.get('Gilgamesh'), undefined, 'no listing body before a listing column is wanted');
    await query({ cols: 'market-sale-median,market-alive,market-oldest-listing' });
    await cell('market-alive', '3');
    await cell('market-alive', '0'); // row 43: a known empty board is zero, not missing
    await cell('market-oldest-listing', '1d 1h');
    await cell('market-oldest-listing', '—'); // rows 43/44: unknown review times stay missing
    assert.equal(listingHits.get('Gilgamesh'), 1, 'both listing columns share one request');
    assert.equal(listingWindowed, false, 'current-listing columns need no page window');
    await heading('market-alive', 'Active listings');
    assert.match(await page.$eval('[data-metric-sort="market-oldest-listing"]', el => el.parentElement.title), /retainer last touched/);
    assert.equal(await page.$eval('[data-metric-sort="market-alive"]', el => el.parentElement.title), '', 'counts carry no age caveat');
    await page.click('[data-metric-sort="market-alive"]');
    await page.waitForFunction(() => new URL(location.href).searchParams.get('sort') === 'grid:market-alive');
    await first(42); // desc: 3, 0, missing last
    await page.select('[data-market-window]', '7');
    await heading('market-sale-median', '(7d)');
    assert.equal(listingHits.get('Gilgamesh'), 1, 'window changes do not refetch the alive set');
    await page.select('[data-market-window]', '30');
    await heading('market-sale-median', '(30d)');
    // A hidden filter reuses the slot rather than requesting again.
    await query({ cols: 'market-sale-median', sort: 'grid:market-sale-median', dir: 'asc', gf: JSON.stringify({ 'market-sellers': { op: 'gte', value: '2' } }) });
    await rows(1);
    assert.equal(listingHits.get('Gilgamesh'), 1, 'hidden listing filters reuse the alive-set body');
    await query({ gf: null, cols: 'market-sale-median,market-sale-median-7' });
    await rows(3);

    // Save/reload/restore retains the page window alongside basis and grid state.
    await page.click('[data-grid-saved-views] > button');
    await page.type('[data-grid-saved-views] form input', 'Thirty days');
    await page.click('[data-grid-saved-views] form button[type="submit"]');
    await page.waitForFunction(() => JSON.parse(localStorage.getItem('ultros.grid.window-fixture-grid.views') || '[]').some(view => view.name === 'Thirty days'));
    // Saving disables the cleared form's submit button, which releases focus.
    // Dismiss by clicking outside instead of sending Escape to the page body.
    await page.click('h1');
    await page.waitForSelector('[data-grid-saved-views] form', { hidden: true });
    await page.select('[data-market-window]', '7');
    await heading('market-sale-median', '(7d)');
    await page.click('[data-grid-saved-views] > button');
    await page.waitForSelector('[data-grid-saved-views] a', { visible: true });
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
    assert.equal(await page.$eval(`${selector('price')} [data-window-item="44"]`, el => el.textContent.trim()), '100');
    // A failed listing body is unknown for every row, never an empty board.
    await query({ cols: 'market-sale-median,market-alive' });
    await cell('market-alive', '—');
    assert.equal(listingHits.get('Cactuar'), 1);
    assert.equal(listingHits.get('Gilgamesh'), 1, 'the old scope is not refetched');
    await query({ gf: JSON.stringify({ 'market-alive': { op: 'present' } }) });
    await rows(3); // unavailable is unknown, not an empty successful body
    await query({ gf: null, cols: 'market-sale-median,market-sale-median-7' });
    await rows(3);
    // Run this race after the lazy-load/cache-count checks so its requests
    // cannot prime the earlier current-listing fixture.
    // A -> B -> A with all bodies held: neither a different scope nor the
    // earlier request for the same scope may overwrite the newest response.
    holdListings = true;
    for (const [index, scope] of ['Gilgamesh', 'Cactuar', 'Gilgamesh'].entries()) {
      await Promise.all([
        page.waitForRequest(request => request.url().includes(`/listing_stats/${scope}`)),
        query(index === 0 ? { scope, cols: 'market-alive' } : { scope }),
      ]);
    }
    assert.deepEqual(heldListings.map(entry => entry.scope), ['Gilgamesh', 'Cactuar', 'Gilgamesh']);
    await heldListings[2].respond(9);
    await cell('market-alive', '9');
    await heldListings[1].respond(15);
    await heldListings[0].respond(3);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await cell('market-alive', '9');
    assert(!(await page.$$eval(selector('market-alive'), cells => cells.map(cell => cell.textContent.trim()))).includes('15'));
    holdListings = false;

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
    for (const endpoint of ['/api/v1/listing_stats/Cactuar', '/api/v1/sale_stats/Cactuar?window=1', '/api/v1/sale_stats/Gilgamesh?window=1']) {
      assert(expectedFailures.has(BASE + endpoint), `expected failing fixture was exercised: ${endpoint}`);
    }
    assert.deepEqual(errors, []);
    console.log('PASS market windows: defaults, pinned comparisons, prices, pending filters/sorts, hidden requirements, deduplication, saved URLs, SSR, scope/window races, failures, missing rows and current-listing columns');
  } catch (error) {
    const artifacts = path.join(__dirname, 'artifacts', 'market-window');
    fs.mkdirSync(artifacts, { recursive: true });
    await page.screenshot({ path: path.join(artifacts, 'failure.png'), fullPage: true }).catch(() => {});
    console.error('Failure URL:', page.url(), 'Browser errors:', errors);
    throw error;
  } finally { await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
