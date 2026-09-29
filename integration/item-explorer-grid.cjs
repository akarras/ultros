// Item Explorer on the shared market grid (#1346): no pagination, legacy
// links and filters keep working, and sale statistics load only on demand.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
// Gladiator's Arms: a gear category, so every optional column has data.
const CATEGORY = process.env.CATEGORY || '10';
const WORLD = process.env.WORLD || 'Gilgamesh';

async function main() {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(60000);
  await page.setViewport({ width: 1280, height: 850 });
  await page.setCookie({ name: 'HIDE_ADS', value: 'true', url: BASE }, { name: 'HOME_WORLD', value: WORLD, url: BASE });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    // A local ClickHouse without history answers sale_stats with 5xx; the
    // probe counts those requests, it does not need them to succeed.
    const url = message.location().url || '';
    if (message.type() === 'error' && !url.includes('/sale_stats/')
      && !/favicon|ERR_BLOCKED_BY_CLIENT|net::ERR_ABORTED/.test(message.text())) errors.push(message.text());
  });
  const stats = [];
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.pathname.startsWith('/api/v1/sale_stats/')) stats.push(`${decodeURIComponent(url.pathname.split('/').at(-1))}/${url.searchParams.get('window')}`);
  });
  await page.evaluateOnNewDocument(() => {
    window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; });
  });
  async function open(query, category = CATEGORY) {
    const url = new URL(`/items/category/${category}${query}`, BASE).href;
    console.log(`Opening ${url}`);
    stats.length = 0;
    const response = await page.goto(url, { waitUntil: 'domcontentloaded' });
    assert(response.ok(), `${url}: HTTP ${response.status()}`);
    await page.waitForFunction(() => window.__hydrated);
    await page.waitForSelector('.virtual-grid');
    await page.waitForSelector('.virtual-grid-cell[data-column="item"]');
    // Prices and the shared statistics arrive after hydration.
    await new Promise(resolve => setTimeout(resolve, 1500));
  }
  async function revealColumn(column) {
    const width = await page.$eval('.virtual-grid', grid => grid.scrollWidth);
    for (let left = 0; left <= width; left += 400) {
      await page.$eval('.virtual-grid', (grid, left) => { grid.scrollLeft = left; }, left);
      await new Promise(resolve => setTimeout(resolve, 50));
      if (await page.$(`.virtual-grid-heading[data-column="${column}"]`)) return;
    }
    assert.fail(`column ${column} never appeared while scrolling the grid`);
  }
  const summaryCount = () => page.$eval('.sticky-bar', bar => Number((bar.textContent.match(/(\d[\d,]*)\s/) || [])[1]?.replace(/,/g, '')));

  // A bare category page: full set, no pagination chrome, no statistics.
  await open('');
  assert.equal(await page.$('[data-market-window]') !== null, true, 'the market window control is in the bar');
  assert.equal(await page.$$eval('a', links => links.filter(a => /[?&]page=/.test(a.getAttribute('href') || '')).length), 0, 'no pagination links remain');
  const total = await summaryCount();
  assert(total > 50, `expected the whole category, saw ${total} rows`);
  assert.deepEqual(stats, [], 'a bare category page requests no sale statistics');
  const headings = await page.$$eval('.virtual-grid-heading', cells => cells.map(cell => cell.dataset.column));
  for (const column of ['item', 'ilvl', 'lv', 'price', 'hq', 'vendor', 'actions']) assert(headings.includes(column), `missing ${column} in ${headings}`);
  assert(!headings.some(column => column.startsWith('market-')), `no shared column is on by default: ${headings}`);

  // Old paginated links resolve to the same, unpaginated category.
  await open('?page=3&per_page=25');
  assert.equal(await summaryCount(), total, 'a paginated deep link shows the whole category');
  assert.deepEqual(stats, []);

  // Legacy range filters become one inclusive shared bound and still bite.
  await open('?min-ilvl=600&max-ilvl=700');
  await page.waitForSelector('[data-registered-filter="ilvl"]');
  const filtered = await summaryCount();
  assert(filtered > 0 && filtered < total, `filter should narrow ${total} rows, saw ${filtered}`);
  const levels = await page.$$eval('.virtual-grid-cell[data-column="ilvl"]', cells => cells.map(cell => Number(cell.textContent.trim())));
  assert(levels.length > 0 && levels.every(level => level >= 600 && level <= 700), `ilvl cells out of range: ${levels}`);
  assert.deepEqual(stats, []);

  // A native price sort orders by NQ price once prices have loaded.
  await open('?sort=price');
  const prices = await page.$$eval('.virtual-grid-cell[data-column="price"]', cells => cells.map(cell => Number(cell.textContent.replace(/[^0-9]/g, ''))).filter(Boolean));
  assert(prices.length > 2, 'NQ prices rendered after hydration');
  for (let index = 1; index < prices.length; index += 1) assert(prices[index] <= prices[index - 1], `not descending: ${prices}`);

  // A shared column requests exactly its window, once, and the window
  // control requests the next one.
  await open('?cols=ilvl,lv,hq,vendor,world,market-sale-median');
  await revealColumn('market-sale-median');
  await page.waitForSelector('.virtual-grid-heading[data-column="market-sale-median"]');
  assert.equal(stats.filter(key => key.endsWith('/7')).length, 1, `one 7-day request, saw ${stats}`);
  assert.equal(stats.length, 1, `only the needed window, saw ${stats}`);
  await Promise.all([
    page.waitForRequest(request => request.url().includes('sale_stats/') && request.url().includes('window=30')),
    page.select('[data-market-window]', '30'),
  ]);

  // A grid sort must drive the toolbar too. Minions default to native Name
  // ascending, whereas every grid sort defaults descending; conflating those
  // defaults makes this direction button ineffective on the first click.
  await open(`?sort=grid:item&world=${encodeURIComponent(WORLD)}&probe=keep#scope`, '75');
  assert.equal(await page.$eval('select[aria-label="Sort By"]', select => select.value), 'grid:item');
  const beforeToggle = await page.evaluate(() => history.length);
  await page.click('button[aria-label="Sort descending"]');
  await page.waitForFunction(() => new URL(location.href).searchParams.get('dir') === 'asc');
  assert.equal(await page.$eval('.virtual-grid-heading[data-column="item"]', heading => heading.getAttribute('aria-sort')), 'ascending');
  await page.click('button[aria-label="Sort ascending"]');
  await page.waitForFunction(() => !new URL(location.href).searchParams.has('dir'));
  assert.equal(await page.$eval('.virtual-grid-heading[data-column="item"]', heading => heading.getAttribute('aria-sort')), 'descending');
  assert.equal(new URL(page.url()).hash, '#scope');
  assert.equal(new URL(page.url()).searchParams.get('probe'), 'keep');
  assert.equal(await page.evaluate(() => history.length), beforeToggle);
  assert.deepEqual(stats, [], 'sorting an item name does not request market history');

  // A phone scrolls the grid, not the page.
  await page.setViewport({ width: 393, height: 800, isMobile: true, hasTouch: true });
  await open('');
  const overflow = await page.evaluate(() => ({
    page: document.documentElement.scrollWidth - window.innerWidth,
    grid: (() => { const grid = document.querySelector('.virtual-grid'); return grid.scrollWidth - grid.clientWidth; })(),
  }));
  assert(overflow.page <= 1, `the page scrolls horizontally by ${overflow.page}px`);
  assert(overflow.grid > 0, 'the grid itself scrolls horizontally on a phone');

  assert.deepEqual(errors, [], 'browser errors');
  await browser.close();
  console.log('item-explorer-grid: ok');
}

main().catch(error => { console.error(error); process.exit(1); });
