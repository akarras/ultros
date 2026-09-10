// Run against this worktree's server. Market data is deterministic; game shop
// definitions and all currency-exchange calculations come from the real app.
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const puppeteer = require('puppeteer');
const { marketFixture } = require('./shared-analyzer-market-fixture.cjs');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const ROUTE = '/currency-exchange/26807'; // Bicolor Gemstone

async function main() {
  // Cover every received item, including additions to the bundled shop data.
  const fixture = marketFixture(Array.from({ length: 55000 }, (_, i) => i + 1));
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(90000);
  const errors = [];
  const worlds = new Set();
  page.on('pageerror', e => errors.push(e.message));
  await page.evaluateOnNewDocument(() => window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; }));
  await page.setCookie({ name: 'HOME_WORLD', value: 'Gilgamesh', url: BASE }, { name: 'HIDE_ADS', value: 'true', url: BASE });
  await page.setRequestInterception(true);
  page.on('request', request => {
    if (request.isInterceptResolutionHandled()) return;
    if (new URL(request.url()).pathname.startsWith('/api/v1/sale_stats/')) worlds.add(new URL(request.url()).pathname.split('/').pop());
    const response = fixture.reply(request);
    return response ? request.respond(response) : request.continue();
  });
  async function open(query) {
    // Client navigation makes the real resource adapters consume the wire fixtures.
    await page.goto(`${BASE}/currency-exchange?lang=en`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__hydrated);
    await page.evaluate(href => {
      const link = document.createElement('a'); link.href = href; link.id = 'currency-probe'; document.body.append(link); link.click();
    }, `${ROUTE}?lang=en&${query}`);
    await page.waitForSelector('.virtual-grid');
    await page.waitForFunction(() => Number(document.querySelector('.virtual-grid').getAttribute('aria-rowcount')) > 1);
  }
  async function cell(column, expected) {
    // Columns can be virtualized horizontally. Scroll to the column via its menu
    // independent of browser viewport and automatic column sizing.
    await page.$eval('.virtual-grid', (g, column) => {
      const heading = g.querySelector(`[data-column="${column}"]`);
      if (heading) heading.scrollIntoView({ block: 'nearest', inline: 'nearest' });
      else g.scrollLeft = g.scrollWidth;
    }, column);
    await page.waitForFunction((column, expected) => [...document.querySelectorAll(`.virtual-grid-cell[data-column="${column}"]`)]
      .some(e => e.textContent.trim().replaceAll(',', '') === String(expected)), {}, column, expected);
  }
  try {
    await page.setViewport({ width: 1800, height: 900 });
    await open('currency_amount=2000&cols=price_per_item,market-listing,market-quality,market-sale-median');
    await cell('price_per_item', 399);
    await cell('market-listing', 400);
    await cell('market-quality', 'NQ');
    await cell('market-sale-median', 900);
    await page.select('[data-market-window]', '30');
    await cell('market-sale-median', 1800);
    await cell('price_per_item', 399);
    assert.deepEqual([...worlds], ['Gilgamesh']);
    const saved = page.url();
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__hydrated);
    await page.waitForSelector('.virtual-grid');
    assert.equal(page.url(), saved);
    assert.equal(await page.$eval('[data-market-window]', e => e.value), '30');
    assert.equal(await page.$eval('#currency-quantity', e => e.value), '2000');
    assert.equal(await page.$('.virtual-grid-heading[data-column="shops"]'), null);
    await open(new URL(saved).searchParams.toString());
    await cell('market-sale-median', 1800);
    await open('currency_amount=2000&cols=&price_per_item_min=399&price_per_item_max=399');
    assert.equal(await page.$('.virtual-grid-heading[data-column="price_per_item"]'), null);
    assert.equal(await page.$$eval('.virtual-grid-heading', cells => cells.length), 3, 'explicit empty cols preserves only required native columns');
    await page.setViewport({ width: 393, height: 844 });
    await page.waitForSelector('.virtual-grid-cell');
    const misaligned = await page.$$eval('.virtual-grid-cell', cells => cells.filter(cell => {
      const heading = document.querySelector(`.virtual-grid-heading[data-column="${cell.dataset.column}"]`);
      if (!heading) return false;
      const a = cell.getBoundingClientRect(), b = heading.getBoundingClientRect();
      return Math.abs(a.left - b.left) > 1 || Math.abs(a.width - b.width) > 1;
    }).map(cell => cell.dataset.column));
    assert.deepEqual(misaligned, [], 'mobile cells align with their headers');
    const artifacts = path.join(__dirname, 'artifacts', 'currency-exchange'); fs.mkdirSync(artifacts, { recursive: true });
    await page.screenshot({ path: path.join(artifacts, 'mobile.png'), fullPage: true });
    assert.deepEqual(errors, []);
    console.log('PASS currency exchange: native estimates, raw listings, NQ stats, window, legacy filters, saved columns, quantity, mobile');
  } catch (error) {
    console.error('Currency Exchange browser state:', {
      url: page.url(), requests: Object.fromEntries(fixture.hits), errors,
      body: await page.$eval('body', e => e.innerText).catch(() => ''),
    });
    throw error;
  } finally { await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
