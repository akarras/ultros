// Search deep links: /venture-analyzer?item=<id> and /scrip-sources?item=<id>
// scroll that row into view and make it the grid's active row. Market data
// comes from the shared fixture; the rows themselves come from the real app.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const { marketFixture } = require('./shared-analyzer-market-fixture.cjs');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD = 'Gilgamesh';
const ROW_HEIGHT = 40;

async function main() {
  const fixture = marketFixture(Array.from({ length: 55000 }, (_, i) => i + 1));
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(90000);
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.evaluateOnNewDocument(() => window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; }));
  await page.setCookie({ name: 'HOME_WORLD', value: WORLD, url: BASE }, { name: 'HIDE_ADS', value: 'true', url: BASE });
  await page.setRequestInterception(true);
  page.on('request', request => {
    if (request.isInterceptResolutionHandled()) return;
    const response = fixture.reply(request);
    return response ? request.respond(response) : request.continue();
  });
  await page.setViewport({ width: 1400, height: 700 });

  async function openGrid(url) {
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__hydrated);
    await page.waitForFunction(() => Number(document.querySelector('.virtual-grid')?.getAttribute('aria-rowcount')) > 20);
  }

  // A target far enough down the unfiltered table that revealing it must scroll.
  async function targetRow(route) {
    await openGrid(`${BASE}${route}?lang=en`);
    return page.evaluate(async rowHeight => {
      const grid = document.querySelector('.virtual-grid');
      grid.scrollTop = 15 * rowHeight;
      await new Promise(resolve => setTimeout(resolve, 400));
      const row = [...grid.querySelectorAll('.virtual-grid-row')].find(r => Number(r.getAttribute('aria-rowindex')) === 17);
      const link = row.querySelector('a[href*="/item/"]');
      const id = Number(link.getAttribute('href').split('/item/').pop().split(/[/?]/)[0]);
      return { id, name: link.textContent.trim() };
    }, ROW_HEIGHT);
  }

  async function expectRevealed(route, target) {
    await openGrid(`${BASE}${route}?lang=en&item=${target.id}`);
    await page.waitForFunction(() => /-r[1-9]\d*-c\d+$/.test(document.querySelector('.virtual-grid')?.getAttribute('aria-activedescendant') || ''));
    const active = await page.evaluate(() => {
      const grid = document.querySelector('.virtual-grid');
      const r = Number(grid.getAttribute('aria-activedescendant').match(/-r(\d+)-c/)[1]);
      const row = [...grid.querySelectorAll('.virtual-grid-row')].find(el => Number(el.getAttribute('aria-rowindex')) === r + 1);
      return { r, text: row?.textContent || '', scrollTop: grid.scrollTop };
    });
    assert.ok(active.text.includes(target.name), `${route}: active row ${active.r} is "${active.text}", wanted ${target.name}`);
    assert.ok(active.scrollTop > 0, `${route}: grid did not scroll to reveal row ${active.r}`);
    console.log(`${route}: revealed row ${active.r} (${target.name}) at scrollTop ${active.scrollTop}`);
  }

  try {
    for (const route of [`/venture-analyzer/${WORLD}`, `/scrip-sources/${WORLD}`]) {
      const target = await targetRow(route);
      await expectRevealed(route, target);
    }
    assert.deepEqual(errors, []);
    console.log('search-deep-links: ok');
  } finally {
    await browser.close();
  }
}

main().catch(e => { console.error(e); process.exit(1); });
