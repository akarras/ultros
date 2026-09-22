// Real hydrated analyzer interactions with deterministic market API responses.
// BASE_URL must point at this worktree's debug server; no production writes.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const { marketFixture, itemIds } = require('./shared-analyzer-market-fixture.cjs');

const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const artifacts = path.join(__dirname, 'artifacts', 'analyzer-consistency');
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));

async function main() {
  assert(['127.0.0.1', 'localhost', '[::1]'].includes(new URL(BASE).hostname), 'Use an isolated local server');
  fs.mkdirSync(artifacts, { recursive: true });
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(90000);
  await page.setViewport({ width: 1700, height: 1000 });
  const errors = [];
  const requests = [];
  // Ruthenium Vambraces of Fending (recipe 5771) and its non-crystal inputs.
  const suspiciousItem = 42014;
  const fixture = marketFixture([...itemIds, suspiciousItem, 43997, 44009, 43998]);
  let scenario = 'recipe';
  page.on('pageerror', error => errors.push(error.message));
  await page.evaluateOnNewDocument(() => window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; }));
  await page.setCookie({ name: 'HOME_WORLD', value: 'Gilgamesh', url: BASE }, { name: 'HIDE_ADS', value: 'true', url: BASE });
  await page.setRequestInterception(true);
  page.on('request', request => {
    if (request.isInterceptResolutionHandled()) return;
    const url = new URL(request.url());
    if (url.origin !== new URL(BASE).origin && !['data:', 'blob:'].includes(url.protocol)) return request.abort();
    const response = fixture.reply(request, body => {
      if (scenario === 'recipe') {
        if (body.cheapest_listings) for (const row of body.cheapest_listings) {
          // Reproduce the exact item and valuation reported on production.
          // Other real recipes supply unknown-evidence controls.
          if (row.item_id === suspiciousItem) row.cheapest_price = 999999999;
          if (row.item_id === 5366) row.cheapest_price = 10000;
        }
        // Preserve known demand but make the median unusable: missing price
        // evidence must not be mistaken for a suspicious valuation.
        if (url.pathname.includes('/sale_stats/')) body.stats = body.stats.map(row => row.item_id === suspiciousItem ? row : { ...row, median_price: 0 });
      } else if (body.cheapest_listings) {
        for (const row of body.cheapest_listings) {
          row.cheapest_price = scenario === 'vendor-suspicious' ? 999999999 : scenario === 'vendor-loss' ? 10000 : 1;
          row.world_id = [34, 63, 73, 79][row.item_id % 4];
        }
      }
    });
    if (!response) return request.continue();
    requests.push(url.pathname);
    return request.respond(response);
  });
  async function navigate(route) {
    await page.goto(`${BASE}/__test/shared-analyzer-data?lang=en`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__hydrated);
    await page.evaluate(href => {
      const link = document.createElement('a'); link.href = href; document.querySelector('main').prepend(link); link.click();
    }, `${BASE}${route}`);
    await page.waitForFunction(pathname => location.pathname === pathname, {}, new URL(`${BASE}${route}`).pathname);
    await page.waitForSelector('.virtual-grid');
    await page.waitForFunction(() => Number(document.querySelector('.virtual-grid')?.getAttribute('aria-rowcount')) > 1);
  }
  async function clickText(selector, text) {
    for (const element of await page.$$(selector)) {
      if ((await element.evaluate(el => el.textContent.trim())) === text) { await element.click(); return; }
    }
    assert.fail(`Missing ${text}`);
  }
  async function viewAction(label) {
    await page.click('[data-grid-saved-views] > button');
    await page.waitForSelector('[data-grid-saved-views] .sticky-bar-popover');
    await clickText('[data-grid-saved-views] a, [data-grid-saved-views] button', label);
  }
  async function queryValue(key, value) {
    await page.waitForFunction((key, value) => new URL(location.href).searchParams.get(key) === value, {}, key, value);
  }
  async function reveal(column) {
    const heading = `.virtual-grid-heading[data-column="${column}"]`;
    const width = await page.$eval('.virtual-grid', grid => grid.scrollWidth);
    for (let left = 0; left <= width; left += 400) {
      await page.$eval('.virtual-grid', (grid, left) => { grid.scrollLeft = left; }, left);
      await wait(40);
      if (await page.$(heading)) { await page.$eval(heading, el => el.scrollIntoView({ block: 'nearest', inline: 'center' })); return heading; }
    }
    assert.fail(`Column not mounted: ${column}`);
  }
  async function checkScopes(tool) {
    for (const [scope, market] of [['world', 'Gilgamesh'], ['datacenter', 'Aether'], ['region', 'North-America']]) {
      const before = requests.length;
      await page.select('[data-testid="analyzer-price-scope"] select', scope);
      await queryValue('market-scope', scope);
      await page.waitForFunction(market => document.querySelector('[data-testid="analyzer-price-scope"]')?.textContent.includes(market), {}, market);
      await page.waitForFunction(() => Number(document.querySelector('.virtual-grid')?.getAttribute('aria-rowcount')) > 1);
      for (let attempts = 0; attempts < 100 && !requests.slice(before).some(url => decodeURIComponent(url).endsWith(`/${market}`)); attempts++) await wait(100);
      assert(requests.slice(before).some(url => decodeURIComponent(url).endsWith(`/${market}`)), `${tool}: ${scope} refetches its market`);
    }
  }
  try {
    await navigate('/recipe-analyzer/Gilgamesh?v=1&lang=en&sort=price&dir=desc');
    await page.waitForFunction(() => document.querySelector('.virtual-grid')?.textContent.includes('Ruthenium Vambraces of Fending'));
    await page.waitForSelector('[data-listing-evidence="unverified"]');
    await viewAction('Recommended');
    await queryValue('hide-suspicious', 'true');
    await queryValue('profit', '1');
    await page.waitForFunction(() => !document.querySelector('.virtual-grid')?.textContent.includes('Ruthenium Vambraces of Fending'));
    await page.waitForSelector('[data-listing-evidence="unverified"]');
    await page.screenshot({ path: path.join(artifacts, 'recipe-recommended.png'), fullPage: true });
    await viewAction('Unrestricted');
    await queryValue('v', '1');
    await queryValue('hide-suspicious', null);
    await viewAction('Make my default');
    await page.waitForFunction(() => localStorage.getItem('ultros.grid.recipe-analyzer-grid.default_view') === '?v=1');
    await page.keyboard.press('Escape');
    await navigate('/recipe-analyzer/Gilgamesh?lang=en');
    await queryValue('v', '1');
    assert.equal(new URL(page.url()).searchParams.get('hide-suspicious'), null, 'empty default remains unrestricted');
    await viewAction('Reset to recommended');
    await queryValue('hide-suspicious', 'true');
    await page.waitForFunction(() => localStorage.getItem('ultros.grid.recipe-analyzer-grid.default_view')?.includes('hide-suspicious=true'));

    scenario = 'vendor-loss';
    await navigate('/vendor-sell/Gilgamesh?v=1&lang=en');
    await page.waitForFunction(() => [...document.querySelectorAll('.virtual-grid-cell[data-column="profit"]')]
      .some(cell => /-\s*[0-9]/.test(cell.textContent)));
    await viewAction('Recommended');
    await queryValue('profit', '1');
    await page.waitForFunction(() => document.querySelector('.virtual-grid')?.getAttribute('aria-rowcount') === '1');
    await viewAction('Unrestricted');
    await queryValue('profit', null);
    await page.waitForFunction(() => [...document.querySelectorAll('.virtual-grid-cell[data-column="profit"]')]
      .some(cell => /-\s*[0-9]/.test(cell.textContent)));
    await page.screenshot({ path: path.join(artifacts, 'vendor-unrestricted-losses.png'), fullPage: true });

    scenario = 'vendor-suspicious';
    await navigate('/vendor-resale/Gilgamesh?v=1&lang=en&sort=market-price&dir=desc');
    await reveal('market-price');
    await page.waitForFunction(() => [...document.querySelectorAll('.virtual-grid-cell[data-column="market-price"]')]
      .some(cell => cell.textContent.includes('999,999,999')));
    await viewAction('Recommended');
    await queryValue('show-suspicious', 'false');
    await page.waitForFunction(() => ![...document.querySelectorAll('.virtual-grid-cell[data-column="market-price"]')]
      .some(cell => cell.textContent.includes('999,999,999')));
    await page.click('[aria-label="Clear all filters"]');
    await queryValue('show-suspicious', null);
    await reveal('market-price');
    await page.waitForFunction(() => [...document.querySelectorAll('.virtual-grid-cell[data-column="market-price"]')]
      .some(cell => cell.textContent.includes('999,999,999')));
    await page.screenshot({ path: path.join(artifacts, 'vendor-resale-cleared-guard.png'), fullPage: true });

    scenario = 'vendor';
    await navigate('/vendor-sell/Gilgamesh?v=1&lang=en&cols=world,listing,vendor,profit');
    const heading = await reveal('world');
    await page.click(`${heading} a`);
    await queryValue('sort', 'grid:world');
    await page.waitForFunction(selector => document.querySelector(selector)?.getAttribute('aria-sort') === 'ascending', {}, heading);
    await page.waitForFunction(() => {
      const names = [...document.querySelectorAll('.virtual-grid-cell[data-column="world"]')].map(cell => cell.textContent.trim());
      return names.length > 1 && names.join('|') === [...names].sort((a, b) => a.localeCompare(b)).join('|');
    });
    const values = () => page.$$eval('.virtual-grid-cell[data-column="world"]', cells => cells.map(cell => cell.textContent.trim()));
    const headerOrder = await values();
    assert(headerOrder.length > 1, 'sort fixture has multiple rows');
    assert.deepEqual(headerOrder, [...headerOrder].sort((a, b) => a.localeCompare(b)), 'World heading sorts displayed names');
    await page.click(heading, { button: 'right' });
    await clickText('.grid-menu-panel a', 'Sort descending');
    await queryValue('dir', 'desc');
    await page.click(heading, { button: 'right' });
    await clickText('.grid-menu-panel a', 'Sort ascending');
    await queryValue('dir', 'asc');
    await page.waitForFunction(selector => document.querySelector(selector)?.getAttribute('aria-sort') === 'ascending', {}, heading);
    await page.waitForFunction(expected => JSON.stringify([...document.querySelectorAll('.virtual-grid-cell[data-column="world"]')].map(cell => cell.textContent.trim())) === JSON.stringify(expected), {}, headerOrder);
    assert.deepEqual(await values(), headerOrder, 'header/menu use one comparator');
    await page.keyboard.press('Escape');
    await checkScopes('Vendor Sell');
    await navigate('/venture-analyzer/Gilgamesh?v=1&lang=en');
    await checkScopes('Ventures');
    await page.setViewport({ width: 393, height: 844 });
    await page.click('[data-grid-saved-views] > button');
    await page.waitForSelector('[data-grid-saved-views] .sticky-bar-popover');
    const mobileViews = await page.$eval('[data-grid-saved-views] .sticky-bar-popover', el => {
      const rect = el.getBoundingClientRect();
      return { left: rect.left, right: rect.right, viewport: innerWidth };
    });
    assert(mobileViews.left >= 0 && mobileViews.right <= mobileViews.viewport,
      `mobile Views stays inside viewport: ${JSON.stringify(mobileViews)}`);
    await page.screenshot({ path: path.join(artifacts, 'venture-mobile-views.png'), fullPage: true });
    await page.keyboard.press('Escape');
    await page.screenshot({ path: path.join(artifacts, 'venture-mobile.png'), fullPage: true });
    await navigate('/flip-finder/Gilgamesh?v=1&lang=en');
    await page.click('button[aria-label="Views"]');
    await page.waitForSelector('.sticky-bar-popover');
    const flipViews = await page.$eval('.sticky-bar-popover', el => {
      const rect = el.getBoundingClientRect();
      return { left: rect.left, right: rect.right, viewport: innerWidth };
    });
    assert(flipViews.left >= 0 && flipViews.right <= flipViews.viewport,
      `mobile Flip Views stays inside viewport: ${JSON.stringify(flipViews)}`);
    await page.screenshot({ path: path.join(artifacts, 'flip-mobile-views.png'), fullPage: true });
    assert.deepEqual(errors, []);
    console.log('PASS analyzer consistency: suspicious listing default, unknown retention, unrestricted/default/reset, editable NPC-profit and resale-suspicion defaults, header/menu sorting, world/DC/region, mobile');
  } catch (error) {
    await page.screenshot({ path: path.join(artifacts, 'failure.png'), fullPage: true }).catch(() => {});
    console.error({ url: page.url(), errors, requests: requests.slice(-20), body: await page.$eval('body', el => el.innerText).catch(() => '') });
    throw error;
  } finally { await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
