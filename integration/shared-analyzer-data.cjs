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
    const selector = `.virtual-grid-heading[data-column="${column}"] .grid-column-menu`;
    await page.$eval(selector, element => element.scrollIntoView({ block: 'center', inline: 'nearest' }));
    await page.click(selector);
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
          const controls = await page.$$('select');
          let basis;
          for (const select of controls) {
            if (await select.evaluate(element => !!element.querySelector('option[value="sale-median"]') && !!element.getClientRects().length)) {
              basis = select; break;
            }
          }
          assert(basis, `${tool} exposes selectable median pricing`);
          await basis.select('sale-median');
          await page.waitForFunction(() => [...new URL(location.href).searchParams.values()].includes('sale-median'));
          await page.waitForFunction((selector, before) => {
            const cell = document.querySelector(selector);
            return cell && cell.textContent !== before;
          }, { timeout: 90000 }, calculated, before);
          await page.$eval('.virtual-grid', (element, left) => { element.scrollLeft = left; }, medianPosition);
          await page.waitForSelector(`.virtual-grid-heading[data-column="${medianColumn}"]`);
        }
        await filter(medianColumn, 'present');
        await menu(medianColumn);
        await menuAction('Hide column');
        await page.waitForFunction(column => !new URL(location.href).searchParams.get('cols')?.split(',').includes(column), {}, medianColumn);
        assert(JSON.parse(new URL(page.url()).searchParams.get('gf'))[medianColumn], `${tool}: hidden filter survives`);
        await page.reload({ waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => window.__queryHydrated, { timeout: 90000 });
        await page.waitForSelector('[data-grid-query-summary]');
        assert(JSON.parse(new URL(page.url()).searchParams.get('gf'))[medianColumn], `${tool}: filter reload survives`);
        if (fixture) assert([...new URL(page.url()).searchParams.values()].includes('sale-median'), `${tool}: selected pricing basis reload survives`);
        console.log(`PASS ${tool}: shared market columns, median calculation, filter, hide and reload (${rowCount} initial rows)`);
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
