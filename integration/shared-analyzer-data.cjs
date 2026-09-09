// Deterministic shared query behavior against the real SSR + hydrated QueryGrid.
// Requires a debug build of this worktree; no market history is required.
// CHECK_ANALYZER_ROUTES=1 also probes all seven tools with deterministic API data.
// ANALYZER_TOOLS=tool,tool narrows those probes; ANALYZER_MARKET_FIXTURE=0 uses live data.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');

const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const ROUTE = '/__test/shared-analyzer-data';
const artifacts = path.join(__dirname, 'artifacts', 'shared-analyzer-data');

function fixtureUrl(params = {}) {
  return `${BASE}${ROUTE}?${new URLSearchParams({ lang: 'en', ...params })}`;
}

async function main() {
  fs.mkdirSync(artifacts, { recursive: true });
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(30000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    if (message.type() === 'error' && !/favicon|ERR_BLOCKED_BY_CLIENT|net::ERR_ABORTED/.test(message.text())) {
      errors.push(message.text());
    }
  });
  await page.setCookie({ name: 'HIDE_ADS', value: 'true', url: BASE });
  await page.evaluateOnNewDocument(() => {
    window.addEventListener('ultros:hydrated', () => { window.__queryHydrated = true; });
  });
  async function open(params = {}) {
    const response = await page.goto(fixtureUrl(params), { waitUntil: 'domcontentloaded' });
    assert(response.ok(), `fixture requires a debug server: HTTP ${response.status()}`);
    await page.waitForFunction(() => window.__queryHydrated);
    await page.waitForSelector('#query-fixture-grid, .virtual-grid');
  }
  async function count(expected) {
    await page.waitForFunction(expected =>
      Number(document.querySelector('.virtual-grid')?.getAttribute('aria-rowcount')) - 1 === expected,
    {}, expected);
  }
  async function first(expected) {
    await page.$eval('.virtual-grid', element => { element.scrollTop = 0; });
    await page.waitForFunction(expected =>
      document.querySelector('.virtual-grid-cell[data-column="item"] [data-fixture-id]')?.getAttribute('data-fixture-id') === String(expected),
    {}, expected);
  }
  async function menu(column) {
    const selector = `.virtual-grid-heading[data-column="${column}"]`;
    await page.$eval(selector, element => element.scrollIntoView({ block: 'center', inline: 'nearest' }));
    await page.click(selector, {button:'right'});
    await page.waitForSelector('.grid-menu-panel');
  }
  async function filter(column, operator, value = '') {
    await menu(column);
    const selector = `[data-metric-filter="${column}"]`;
    await page.select(`${selector} select`, operator);
    if (!['missing', 'present'].includes(operator)) {
      await page.click(`${selector} input`, { count: 3 });
      await page.type(`${selector} input`, value);
    }
    await page.click(`${selector} button[type="submit"]`);
    await page.waitForFunction((column, operator) =>
      JSON.parse(new URL(location.href).searchParams.get('gf') || '{}')[column]?.op === operator,
    {}, column, operator);
    await page.keyboard.press('Escape');
  }
  async function menuAction(label) {
    for (const button of await page.$$('.grid-menu-panel button')) {
      if ((await button.evaluate(element => element.textContent.trim())) === label) {
        await button.click();
        return;
      }
    }
    throw new Error(`Missing grid action: ${label}`);
  }
  async function coverage(expected) {
    await page.waitForFunction(expected => {
      const text = document.querySelector('[data-grid-query-coverage]')?.textContent || '';
      return Number(text.match(/\d+/)?.[0]) === expected;
    }, {}, expected);
  }
  try {
    await page.setViewport({ width: 1280, height: 900 });
    await open();
    await count(250);
    assert(await page.$$eval('.virtual-grid-cell', cells => cells.length) < 1000);
    await page.$eval('.virtual-grid', element => { element.scrollTop = element.scrollHeight; });
    await page.waitForSelector('[data-fixture-id="249"]');

    // This match starts beyond both the former 100-row cap and the initial viewport.
    await first(0);
    await filter('amount', 'gte', '150');
    await count(99);
    await first(150);
    await menu('amount');
    await menuAction('Hide column');
    await page.waitForFunction(() => !document.querySelector('.virtual-grid-heading[data-column="amount"]'));
    await count(99);
    assert.match(await page.$eval('[data-grid-query-summary]', element => element.textContent), /Amount: At least 150/);
    const saved = page.url();
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__queryHydrated);
    await count(99);
    assert.equal(page.url(), saved, 'reload keeps hidden filter and layout state');
    assert.equal(await page.$('.virtual-grid-heading[data-column="amount"]'), null);
    await page.click('[data-grid-saved-views] > button');
    await page.type('[data-grid-saved-views] form input', 'Hidden amount');
    await page.click('[data-grid-saved-views] form button[type="submit"]');
    await page.waitForFunction(() =>
      JSON.parse(localStorage.getItem('ultros.grid.query-fixture-grid.views') || '[]').some(view => view.name === 'Hidden amount'));
    await page.keyboard.press('Escape');
    await page.click('[data-grid-query-summary] a');
    await count(250);
    await page.click('[data-grid-saved-views] > button');
    await page.click('[data-grid-saved-views] a');
    await count(99);
    assert.equal(await page.$('.virtual-grid-heading[data-column="amount"]'), null,
      'named view restores a hidden query column');
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__queryHydrated);
    await count(99);

    // Direct SSR and hydration agree on a filtered, sorted result.
    const params = { gf: JSON.stringify({ amount: { op: 'gte', value: '150' } }), sort: 'grid:amount', dir: 'desc' };
    const ssr = await browser.newPage();
    await ssr.setJavaScriptEnabled(false);
    await ssr.goto(fixtureUrl(params), { waitUntil: 'domcontentloaded' });
    assert.equal(await ssr.$eval('.virtual-grid', element => Number(element.getAttribute('aria-rowcount')) - 1), 99);
    assert.equal(await ssr.$eval('.virtual-grid-cell[data-column="item"] [data-fixture-id]', element => element.dataset.fixtureId), '248');
    await ssr.close();
    await open(params);
    await count(99);
    await first(248);

    // Missing prices remain at the end in both directions, rather than becoming zero.
    for (const dir of ['asc', 'desc']) {
      await open({ sort: 'grid:amount', dir });
      await first(dir === 'asc' ? 0 : 248);
      await page.$eval('.virtual-grid', element => { element.scrollTop = element.scrollHeight; });
      await page.waitForSelector('.virtual-grid-cell[data-column="item"][data-grid-row="250"] [data-fixture-id="249"]');
    }

    // Unknown rows stay eligible; loading either half removes only known failures.
    await open({ gf: JSON.stringify({ partial: { op: 'gte', value: '150' } }) });
    await count(250);
    await coverage(250);
    await page.click('#query-load-first');
    await count(125);
    await coverage(125);
    await first(125);
    await page.$eval('.virtual-grid', element => { element.scrollTop = element.scrollHeight; });
    await page.waitForSelector('[data-fixture-id="249"]');
    await first(125);
    assert.equal(await page.$('[data-fixture-id="0"]'), null, 'offscreen loaded failures remain excluded');
    await page.click('#query-load-all');
    await count(100);
    await coverage(3); // Failed, completed missing history and still-pending feeds remain disclosed.
    await first(150);
    await menu('partial');
    assert.equal(await page.$$eval('.grid-menu-panel a', links => links.filter(link => /sort=grid/.test(link.href)).length), 0,
      'partial enrichment must not advertise a global sort');
    await page.keyboard.press('Escape');

    // Set membership and missing-data queries use raw values, including on phones.
    await page.setViewport({ width: 393, height: 844, isMobile: true, hasTouch: true });
    await open({ cols: 'worlds' });
    await filter('worlds', 'eq', 'cactuar');
    await count(125);
    await first(0);
    await open();
    await filter('amount', 'missing');
    await count(1);
    await first(249);
    await page.screenshot({ path: path.join(artifacts, 'missing-mobile.png'), fullPage: true });
    // Shared registry: toolbar/menu/header edits are the same query, including
    // legacy bounds and inputs that run before metric evaluation.
    await page.setViewport({ width: 1200, height: 900 });
    await open({ 'registry-test': '1', 'min-amount': '10', 'max-amount': '20' });
    await count(11);
    assert.equal(await page.$$eval('[data-registered-filter="amount"]', chips => chips.length), 1);
    assert.equal(await page.$('[data-grid-query-summary]'), null, 'one filter surface');
    await page.click('[data-registered-filter="amount"] .filter-chip-value');
    await page.waitForSelector('[data-registered-editor]');
    assert.equal(await page.$eval('[data-registered-editor] select', el => el.value), 'between');
    assert.equal(await page.$eval('[data-registered-editor] input[aria-label="Upper bound"]', el => el.value), '20');
    await page.click('[data-registered-editor] input[aria-label="Upper bound"]', { count: 3 });
    await page.type('[data-registered-editor] input[aria-label="Upper bound"]', '25');
    await page.click('[data-registered-editor] button[type="submit"]');
    await count(16);
    assert.equal(new URL(page.url()).searchParams.has('min-amount'), false);
    assert.equal(new URL(page.url()).searchParams.has('max-amount'), false);
    await filter('amount', 'gte', '240');
    await count(9);
    assert.match(await page.$eval('[data-registered-filter="amount"]', el => el.textContent), /At least 240/);
    await page.click('[data-registered-filter="amount"] .filter-chip-x');
    await count(250);
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__queryHydrated);
    await count(250);
    const addMenu = async () => {
      await page.click('.registered-filter-bar [data-add-filter-menu]');
    };
    await addMenu();
    await page.click('[data-add-filter="amount"]');
    await page.waitForSelector('[data-registered-editor]');
    assert.equal(new URL(page.url()).searchParams.has('gf'), false, 'adding does not invent a threshold');
    await count(250);
    await page.click('[data-registered-editor] button[type="submit"]');
    assert.equal(new URL(page.url()).searchParams.has('gf'), false, 'blank numeric input cannot apply');
    await page.select('[data-registered-editor] select', 'between');
    await page.type('[data-registered-editor] input[aria-label="Value"]', '20');
    await page.type('[data-registered-editor] input[aria-label="Upper bound"]', '40');
    await page.click('[data-registered-editor] button[type="submit"]');
    await count(21);
    await addMenu();
    await page.click('[data-add-filter="multiplier"]');
    await page.type('[data-registered-editor] input', '2');
    await page.click('[data-registered-editor] button[type="submit"]');
    await count(11);
    await first(10);
    await menu('amount');
    await menuAction('Hide column');
    await count(11);
    await page.setViewport({ width: 375, height: 812 });
    // Let the existing desktop sidebar's mobile slide-out transition finish
    // before capturing the filter row beneath it.
    await page.waitForFunction(() => {
      const nav = document.querySelector('.app-shell > .side-nav');
      return !nav || nav.getBoundingClientRect().right <= 1;
    });
    await page.screenshot({ path: path.join(artifacts, 'registered-filters-mobile.png'), fullPage: true });
    assert(await page.$$eval('.registered-filter-bar .filter-chip', chips => chips.every(chip => {
      const rect = chip.getBoundingClientRect(); return rect.left >= 0 && rect.right <= innerWidth;
    })), 'registered chips wrap within mobile viewport');
    await page.click('[aria-label="Clear all filters"]');
    await count(250);
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__queryHydrated);
    await count(250);
    assert.equal(new URL(page.url()).searchParams.has('multiplier'), false);
    console.log('PASS shared registry: editable chips, menu/header equivalence, simultaneous bounds, legacy clear/reload, pre-calculation inputs and mobile wrapping');

    // The real MarketGrid wrapper, including a delayed complete statistics body.
    await page.setViewport({ width: 1280, height: 900 });
    let releaseStats;
    let holdStats = true;
    const sortFixture = request => {
      const url = new URL(request.url());
      if (!url.pathname.startsWith('/api/v1/sale_stats/')) return request.continue();
      const body = { stats: [42, 43].map((item_id, index) => ({ item_id, hq: false,
        min_price: 50 + index * 50, median_price: 100 + index * 100, avg_price: 110 + index * 100,
        num_sold: 14, units_sold: 28, vwap: 100 + index * 100, sales_per_day: 2,
        gil_volume: 2800, last_sold_unix: 1788900000, confidence: 'high',
      })) };
      const respond = () => request.respond({ status: 200, contentType: 'application/json', body: JSON.stringify(body) });
      if (url.searchParams.get('window') === '30' && holdStats) releaseStats = respond;
      else return respond();
    };
    await page.setRequestInterception(true);
    page.on('request', sortFixture);
    const median = 'market-sale-median-30';
    const heading = column => `.virtual-grid-heading[data-column="${column}"]`;
    const sortLink = column => `[data-metric-sort="${column}"]`;
    async function sorted(column, dir) {
      await page.waitForFunction((column, dir) => {
        const q = new URL(location.href).searchParams;
        return q.get('sort') === `grid:${column}` && q.get('dir') === dir
          && document.querySelector(`.virtual-grid-heading[data-column="${column}"]`)?.getAttribute('aria-sort') === (dir === 'asc' ? 'ascending' : 'descending');
      }, {}, column, dir);
      assert.equal(await page.$eval(`${sortLink(column)} [aria-hidden]`, el => el.textContent), dir === 'asc' ? '↑' : '↓');
    }
    async function marketFirst(id) {
      await page.waitForFunction(id => document.querySelector('.virtual-grid-cell[data-column="item"] [data-window-item]')?.dataset.windowItem === String(id), {}, id);
    }
    const preserved = { 'market-window-test': '1', window: '30', revenue: 'sale-median',
      scope: 'Gilgamesh', cols: `${median},market-sale-min-7,market-trend-7,market-drift-7`,
      gf: JSON.stringify({ 'market-quality': { op: 'eq', value: 'NQ' } }),
      l: '2~~market-sale-median-30.5k', custom: 'a & b', lang: 'en' };
    await open({ ...preserved, sort: 'price', dir: 'asc' });
    await count(3);
    assert.equal(await page.$eval(heading('item'), el => el.getAttribute('aria-sort')), 'ascending');
    await page.waitForFunction(() => [...document.querySelectorAll('.virtual-grid-cell')].some(el => /Loading/.test(el.textContent)));
    await page.click(sortLink(median));
    await sorted(median, 'desc');
    assert.equal(await page.$eval(heading('item'), el => el.getAttribute('aria-sort')), 'none', 'unregistered native sort clears');
    await count(3);
    await marketFirst(42); // No premature ranking while a complete body is pending.
    await page.waitForFunction(() => [...document.querySelectorAll('[role="status"]')].some(el => /sort/i.test(el.textContent)));
    for (const [key, value] of Object.entries(preserved)) assert.equal(new URL(page.url()).searchParams.get(key), value, `${key} survives sorting`);
    assert(releaseStats, 'complete 30-day body was held');
    holdStats = false;
    await releaseStats();
    await marketFirst(43);
    await page.click(sortLink(median));
    await sorted(median, 'asc');
    await marketFirst(42);
    assert.equal(await page.$eval('.virtual-grid-cell[data-column="item"][data-grid-row="3"] [data-window-item]', el => el.dataset.windowItem), '44', 'missing history remains last');
    // Follow the grid keyboard model: focus grid, choose header, Enter, Tab past
    // its move handle, then Enter on the actual sort link.
    await page.click(heading(median), { button: 'right' });
    await page.keyboard.press('Escape');
    await page.focus('.virtual-grid');
    await page.keyboard.press('Enter');
    await page.keyboard.press('Tab');
    assert.equal(await page.evaluate(() => document.activeElement?.dataset.metricSort), median);
    await page.keyboard.press('Enter');
    await sorted(median, 'desc');
    await marketFirst(43);
    await page.keyboard.press('Escape');
    const reloadUrl = page.url();
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__queryHydrated);
    await sorted(median, 'desc');
    await marketFirst(43);
    assert.equal(page.url(), reloadUrl);
    const marketSsr = await browser.newPage();
    await marketSsr.setJavaScriptEnabled(false);
    await marketSsr.goto(reloadUrl, { waitUntil: 'domcontentloaded' });
    assert.equal(await marketSsr.$eval(heading(median), el => el.getAttribute('aria-sort')), 'descending');
    assert.equal(await marketSsr.$eval(sortLink(median), el => new URL(el.href).searchParams.get('dir')), 'asc');
    assert.equal(await marketSsr.$eval('.virtual-grid-cell[data-column="item"] [data-window-item]', el => el.dataset.windowItem), '42', 'SSR keeps original order before statistics arrive');
    await marketSsr.close();
    for (const partial of ['market-trend-7', 'market-drift-7']) {
      await page.$eval(heading(partial), el => el.scrollIntoView({ block: 'nearest', inline: 'center' }));
      assert.equal(await page.$(`${heading(partial)} .grid-heading-content a, ${heading(partial)} .grid-heading-content button`), null);
      assert.equal(await page.$eval(heading(partial), el => el.getAttribute('aria-sort')), 'none');
      await menu(partial);
      assert.equal(await page.$$eval('.grid-menu-panel a', links => links.filter(link => /sort=grid/.test(link.href)).length), 0);
      await page.keyboard.press('Escape');
    }
    await page.setViewport({ width: 393, height: 844, isMobile: true, hasTouch: true });
    await page.waitForFunction(() => window.__queryHydrated);
    await page.$eval(heading(median), el => el.scrollIntoView({ block: 'nearest', inline: 'center' }));
    await page.tap(sortLink(median));
    await sorted(median, 'asc');
    await marketFirst(42);
    await menu(median);
    await page.click('.grid-menu-panel a[href*="dir=desc"]');
    await sorted(median, 'desc');
    await marketFirst(43);
    assert.equal(new URL(page.url()).searchParams.getAll('sort').length, 1, 'menu and header share replacement semantics');
    assert.equal(new URL(page.url()).searchParams.getAll('dir').length, 1);
    await page.keyboard.press('Escape');
    await page.screenshot({ path: path.join(artifacts, 'shared-sort-mobile.png'), fullPage: true });
    page.off('request', sortFixture);
    await page.setRequestInterception(false);
    console.log('PASS shared headers: delayed body, click/toggle, native replacement, URL preservation/reload, aria-sort, keyboard, touch, missing last, partial exclusion');
    if (process.env.CHECK_ANALYZER_ROUTES === '1') {
      const fixture = process.env.ANALYZER_MARKET_FIXTURE === '0' ? null
        : require('./shared-analyzer-market-fixture.cjs').marketFixture();
      if (fixture) {
        await page.setRequestInterception(true);
        page.on('request', request => {
          const response = fixture.reply(request);
          return response ? request.respond(response) : request.continue();
        });
        console.log('Analyzer route probes use deterministic market API fixtures with real game definitions and WASM adapters');
      }
      const world = process.env.WORLD || 'Gilgamesh';
      const routes = [
        ['flip-finder', `/flip-finder/${world}`],
        ['recipe-analyzer', '/recipe-analyzer'],
        ['venture-analyzer', '/venture-analyzer'],
        ['leve-analyzer', '/leve-analyzer'],
        ['fc-crafting-analyzer', `/fc-crafting-analyzer/${world}`],
        ['vendor-resale', `/vendor-resale/${world}`],
        ['scrip-sources', '/scrip-sources'],
      ];
      const shared = ['market-sale-median-7', 'market-sale-min-7', 'market-sale-avg-7',
        'market-sale-median-30', 'market-sale-median', 'market-gil-7',
        'market-world', 'market-datacenter', 'market-sales-per-day-7', 'market-cadence-7', 'market-trend-7'];
      await page.setViewport({ width: 1600, height: 1000 });
      await page.setCookie({ name: 'HOME_WORLD', value: world, url: BASE });
      for (const [tool, route] of routes) {
        if (process.env.ANALYZER_TOOLS && !process.env.ANALYZER_TOOLS.split(',').includes(tool)) continue;
        // Recipe preserves its existing saved column IDs and presents cadence
        // through its daily-sales column; the other adapters use market-* IDs.
        const required = tool === 'recipe-analyzer'
          ? ['rev-sale-median', 'rev-sale-min', 'rev-sale-avg', 'listing-world', 'listing-dc', 'daily-sales', 'trend']
          : shared;
        const medianColumn = required[0];
        const query = new URLSearchParams({ v: '1', lang: 'en', world, 'min-sales': '0',
          profit: '-1000000000', roi: '-1000000000', 'next-sale': '1M', sort: 'grid:item', dir: 'asc',
          cols: ['profit', 'cost', ...required].join(',') });
        if (tool === 'flip-finder' || tool === 'vendor-resale') query.delete('world');
        const target = `${BASE}${route}?${query}`;
        console.log(`CHECK ${tool}: navigating`);
        if (fixture) {
          // SSR resources read the server database, outside browser interception.
          // Navigate in-app so client resource requests exercise the wire fixtures.
          await open();
          await page.evaluate(href => {
            const link = document.createElement('a');
            link.id = 'fixture-navigate'; link.href = href; link.textContent = 'Open analyzer';
            document.querySelector('main').prepend(link);
          }, target);
          await page.$eval('#fixture-navigate', link => link.click());
          await page.waitForFunction(pathname => location.pathname === pathname, {}, route);
        } else {
          const response = await page.goto(target, { waitUntil: 'domcontentloaded', timeout: 90000 });
          assert(response.ok(), `${tool}: HTTP ${response.status()}`);
        }
        await page.waitForFunction(() => window.__queryHydrated, { timeout: 90000 });
        await page.waitForSelector('.virtual-grid', { timeout: 90000 });
        if (fixture) await page.waitForFunction(() => Number(document.querySelector('.virtual-grid')?.getAttribute('aria-rowcount')) > 1, { timeout: 90000 });
        console.log(`CHECK ${tool}: grid ready`);
        const rowCount = await page.$eval('.virtual-grid', element => Number(element.getAttribute('aria-rowcount')) - 1);
        if (rowCount === 0) console.log(`EMPTY DATA ${tool}: validating column/query controls; market result-value assertions skipped`);
        // Virtualized columns mount only as their portion of the grid becomes visible.
        const observed = new Set();
        let medianPosition = 0;
        const width = await page.$eval('.virtual-grid', element => element.scrollWidth);
        for (let left = 0; left <= width; left += 500) {
          await page.$eval('.virtual-grid', (element, left) => { element.scrollLeft = left; }, left);
          await new Promise(resolve => setTimeout(resolve, 100));
          for (const id of await page.$$eval('.virtual-grid-heading', headings => headings.map(element => element.dataset.column))) {
            observed.add(id);
            if (id === medianColumn) medianPosition = left;
          }
        }
        for (const column of required) assert(observed.has(column), `${tool} registers ${column}`);
        await page.$eval('.virtual-grid', (element, left) => { element.scrollLeft = left; }, medianPosition);
        await page.waitForSelector(`.virtual-grid-heading[data-column="${medianColumn}"]`);
        if (fixture) await page.waitForFunction(column => [...document.querySelectorAll(`.virtual-grid-cell[data-column="${column}"]`)]
          .some(cell => /900|1[,. ]?500/.test(cell.textContent)), { timeout: 90000 }, medianColumn);
        if (fixture) {
          await page.$eval('.virtual-grid', element => { element.scrollLeft = 0; });
          const calculated = `.virtual-grid-cell[data-column="${tool === 'scrip-sources' ? 'cost' : 'profit'}"]`;
          await page.waitForSelector(calculated);
          const before = await page.$eval(calculated, cell => cell.textContent);
          const basisKey = ['leve-analyzer', 'fc-crafting-analyzer', 'scrip-sources'].includes(tool) ? 'cost-basis' : 'revenue';
          if (tool !== 'recipe-analyzer') {
            await page.click(`[data-registered-filter="${basisKey}"] .filter-chip-value`);
            await page.waitForSelector('[data-registered-editor]');
          }
          const controls = await page.$$('select');
          let basis;
          for (const select of controls) {
            if (await select.evaluate(element => !!element.querySelector('option[value="sale-median"]') && !!element.getClientRects().length)) {
              basis = select; break;
            }
          }
          assert(basis, `${tool} exposes selectable median pricing`);
          await basis.select('sale-median');
          if (tool !== 'recipe-analyzer') await page.click('[data-registered-editor] button[type="submit"]');
          await page.waitForFunction(() => [...new URL(location.href).searchParams.values()].includes('sale-median'));
          await page.waitForFunction((selector, before) => {
            const cell = document.querySelector(selector);
            return cell && cell.textContent !== before;
          }, { timeout: 90000 }, calculated, before);
          if (tool !== 'recipe-analyzer') {
            const sevenDayPrice = await page.$eval(calculated, cell => cell.textContent);
            await page.select('[data-market-window]', '30');
            await page.waitForFunction((selector, before) => document.querySelector(selector)?.textContent !== before,
              {}, calculated, sevenDayPrice);
            assert.equal(new URL(page.url()).searchParams.get('window'), '30');
            await page.click(`[data-registered-filter="${basisKey}"] .filter-chip-value`);
            await page.waitForSelector('[data-registered-editor]');
            assert.match(await page.$eval('[data-registered-editor] option[value="sale-median"]', el => el.textContent), /30d/);
            await page.keyboard.press('Escape');
          }
          await page.$eval('.virtual-grid', (element, left) => { element.scrollLeft = left; }, medianPosition);
          await page.waitForSelector(`.virtual-grid-heading[data-column="${medianColumn}"]`);
        }
        if (tool !== 'recipe-analyzer') {
          await page.$eval('.virtual-grid', element => { element.scrollLeft = 0; });
          const native = tool === 'scrip-sources' ? 'cost' : 'profit';
          await page.waitForSelector(`${heading(native)} a`);
          await page.click(`${heading(native)} a`);
          await page.waitForFunction(() => !new URL(location.href).searchParams.get('sort')?.startsWith('grid:'));
          await page.$eval('.virtual-grid', (element, left) => { element.scrollLeft = left; }, medianPosition);
          await page.waitForSelector(sortLink(medianColumn));
          const beforeSort = new URL(page.url()).searchParams;
          await page.click(sortLink(medianColumn));
          await sorted(medianColumn, 'desc');
          for (const [key, value] of beforeSort) if (!['sort', 'dir'].includes(key)) {
            assert.equal(new URL(page.url()).searchParams.get(key), value, `${tool}: ${key} survives native-to-shared sort`);
          }
          assert.equal(await page.$$eval('.virtual-grid-heading[aria-sort]', headings => headings.filter(el => el.getAttribute('aria-sort') !== 'none').length), 1);
          await page.click(sortLink(medianColumn));
          await sorted(medianColumn, 'asc');
          await page.$eval('.virtual-grid', element => { element.scrollLeft = 0; });
          await page.waitForSelector(`${heading(native)} a`);
          assert.equal(await page.$('.virtual-grid-heading a[aria-current="true"]'), null, `${tool}: native sort arrow is inactive`);
          await page.click(`${heading(native)} a`);
          await page.waitForFunction(() => !new URL(location.href).searchParams.get('sort')?.startsWith('grid:'));
          await page.$eval('.virtual-grid', (element, left) => { element.scrollLeft = left; }, medianPosition);
          await page.waitForSelector(sortLink(medianColumn));
          assert.equal(await page.$eval(heading(medianColumn), el => el.getAttribute('aria-sort')), 'none');
          assert.equal(await page.$(`${sortLink(medianColumn)} [aria-hidden]`), null);
        }
        await filter(medianColumn, 'present');
        await menu(medianColumn);
        await menuAction('Hide column');
        await page.waitForFunction(column => !new URL(location.href).searchParams.get('cols')?.split(',').includes(column), {}, medianColumn);
        assert(JSON.parse(new URL(page.url()).searchParams.get('gf'))[medianColumn], `${tool}: hidden filter survives`);
        await page.reload({ waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => window.__queryHydrated, { timeout: 90000 });
        await page.waitForSelector(tool === 'recipe-analyzer'
          ? '[data-grid-query-summary]' : `[data-registered-filter="${medianColumn}"]`);
        if (tool !== 'recipe-analyzer') assert.equal(await page.$('[data-grid-query-summary]'), null, `${tool}: shared bar owns the only filter summary`);
        assert(JSON.parse(new URL(page.url()).searchParams.get('gf'))[medianColumn], `${tool}: filter reload survives`);
        if (fixture) assert([...new URL(page.url()).searchParams.values()].includes('sale-median'), `${tool}: selected pricing basis reload survives`);
        if (fixture && tool !== 'recipe-analyzer') {
          await page.click('[aria-label="Clear all filters"]');
          await page.waitForFunction(() => !new URL(location.href).searchParams.has('gf'));
          assert.equal(new URL(page.url()).searchParams.get('window'), '30', `${tool}: Clear all preserves window`);
          assert([...new URL(page.url()).searchParams.values()].includes('sale-median'), `${tool}: Clear all preserves price basis`);
        }
        console.log(`PASS ${tool}: shared market columns, median calculation, filter, hide and reload (${rowCount} initial rows)`);
        if (tool === 'recipe-analyzer') {
          // Bookmarks carrying the retired experiment flag keep the same columns.
          const legacy = new URL(page.url());
          legacy.searchParams.set('labs', 'analyzer-recipe');
          legacy.searchParams.set('cols', [...required, 'rev-sale-median'].join(','));
          await page.goto(legacy.href, { waitUntil: 'domcontentloaded', timeout: 90000 });
          await page.waitForFunction(() => window.__queryHydrated, { timeout: 90000 });
          await page.waitForSelector('.virtual-grid', { timeout: 90000 });
          const legacyColumns = new Set();
          const legacyWidth = await page.$eval('.virtual-grid', element => element.scrollWidth);
          for (let left = 0; left <= legacyWidth; left += 500) {
            await page.$eval('.virtual-grid', (element, left) => { element.scrollLeft = left; }, left);
            await new Promise(resolve => setTimeout(resolve, 100));
            for (const id of await page.$$eval('.virtual-grid-heading', headings => headings.map(element => element.dataset.column))) {
              legacyColumns.add(id);
            }
          }
          for (const column of ['item', 'profit', 'daily-sales', 'listing-world', 'listing-dc']) {
            assert(legacyColumns.has(column), `default Recipe retains ${column}`);
          }
          for (const column of required) {
            assert(legacyColumns.has(column), `Recipe retains ${column} in an old Labs bookmark`);
          }
          console.log('PASS recipe-analyzer: promoted columns work by default and in old Labs bookmarks');
        }
      }
      if (fixture) {
        for (const source of ['cheapest', 'recentSales', 'sale_stats']) assert(fixture.hits.get(source) > 0, `${source} fixture was consumed`);
      }
    }
    assert.deepEqual(errors, [], 'no browser or hydration errors');
    console.log('PASS shared queries: all rows, hidden filters, reload, SSR/hydration, missing values, partial coverage, offscreen retention, set membership, mobile');
  } catch (error) {
    console.error('Failure URL:', page.url());
    console.error('Browser errors:', errors);
    await page.screenshot({ path: path.join(artifacts, 'failure.png'), fullPage: true }).catch(() => {});
    throw error;
  } finally {
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
