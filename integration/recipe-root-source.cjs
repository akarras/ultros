// Real recipe data/rendering/hydration; deterministic market and save transport.
// Requires a fresh server built with test-auth.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const OUT = path.join(__dirname, 'artifacts', 'recipe-root-source');

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  let page;
  let requestHandler;
  const requests = new Set();
  try {
    page = await browser.newPage();
    page.setDefaultTimeout(120000);
    await page.setCookie({ name: 'HIDE_ADS', value: 'true', url: BASE, path: '/' });
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    await page.evaluateOnNewDocument(() => {
      window.__recipeHydrated = false;
      window.addEventListener('ultros:hydrated', () => { window.__recipeHydrated = true; });
    });
    await page.setJavaScriptEnabled(false);
    const response = await page.goto(`${BASE}/test/login?user_id=990000001360&username=recipe-root-fixture`, { waitUntil: 'domcontentloaded' });
    assert.ok(response.ok(), 'test-auth login must be available');
    assert.ok((await page.cookies()).some(c => c.name === 'discord_auth'), 'test login must establish a session');
    let recipeUrl;
    for (const item of [5364, 13709, 39643, 23892]) {
      await page.goto(`${BASE}/item/Gilgamesh/${item}`, { waitUntil: 'domcontentloaded' });
      const links = await page.$$eval('a[href^="/recipe/"]', nodes => nodes.map(n => n.getAttribute('href')));
      for (const link of links.slice(0, 3)) {
        await page.goto(new URL(link, BASE).href, { waitUntil: 'domcontentloaded' });
        if (await page.$('[aria-label="HQ finished item"]') && await page.$$eval('select[aria-label^="Source for"]', nodes => nodes.some(n => n.options.length > 1))) {
          recipeUrl = new URL(link, BASE);
          break;
        }
      }
      if (recipeUrl) break;
    }
    assert.ok(recipeUrl, 'fixture requires an HQ-capable output with a craftable component');
    const output = await page.$eval('header a[href^="/item/"]', n => Number(n.getAttribute('href').split('/').at(-1)));
    recipeUrl.searchParams.set('world', 'Gilgamesh');
    recipeUrl.searchParams.set('quantity', '50');
    recipeUrl.searchParams.set('include-vendors', 'false');
    recipeUrl.searchParams.set('route', 'home');
    let shortage = false;
    const saves = [];
    await page.setRequestInterception(true);
    const intercept = request => {
      const url = new URL(request.url());
      // Only the external advertising SDK is outside this application probe.
      // Every application pageerror remains an assertion failure.
      if (url.hostname === 'pagead2.googlesyndication.com' && url.pathname.endsWith('/adsbygoogle.js')) {
        return request.respond({ status: 200, contentType: 'application/javascript', body: '/* Ad SDK disabled for application regression. */' });
      }
      const pathname = url.pathname;
      const respond = value => request.respond({ status: 200, contentType: 'application/json', body: JSON.stringify(value) });
      if (pathname === '/api/v1/list') return respond([{ list: { id: 1360, owner: 990000001360, name: 'Root source fixture', wdr_filter: { World: 63 } }, permission: 'Owner' }]);
      if (pathname === '/api/v1/list/1360/add/items') {
        saves.push(JSON.parse(request.postData()));
        return respond(null);
      }
      const match = pathname.match(/^\/api\/v1\/listings\/[^/]+\/(\d+)$/);
      if (!match) return request.continue();
      const item = Number(match[1]);
      const listings = [false, true].map((hq, index) => {
        const id = item * 10 + index;
        return [{ id, item_id: item, world_id: 63, retainer_id: id, hq, quantity: item === output ? (shortage ? 49 : 50) : 999, price_per_unit: hq ? 200 : 100, timestamp: '2026-09-16T12:00:00' }, { id, world_id: 63, name: 'Root source fixture', retainer_city_id: 1 }];
      });
      return respond({ listings, sales: [], last_updated: [{ world_id: 63, updated_at: '2026-09-16T12:00:00' }] });
    };
    requestHandler = request => {
      const pending = Promise.resolve().then(() => intercept(request)).catch(error => {
        errors.push(`Request interception: ${error.stack || error}`);
      });
      requests.add(pending);
      void pending.then(() => requests.delete(pending));
    };
    page.on('request', requestHandler);
    await page.setJavaScriptEnabled(true);
    const ready = async () => {
      await page.waitForFunction(() => window.__recipeHydrated);
      await page.waitForFunction(() => document.querySelector('[data-testid="plan-total"]')?.textContent.includes('gil'));
    };
    await page.goto(recipeUrl.href, { waitUntil: 'networkidle2' });
    await ready();
    const component = await page.$$eval('select[aria-label^="Source for"]', nodes => {
      const n = nodes.find(n => n.options.length > 1);
      return { label: n.getAttribute('aria-label'), value: n.options[1].value };
    });
    await page.select(`select[aria-label=${JSON.stringify(component.label)}]`, component.value);
    await page.waitForFunction(() => new URL(location.href).searchParams.has('craft'));
    const choice = await page.evaluate(() => new URL(location.href).searchParams.get('craft'));
    await page.select('[aria-label="Main item source"]', 'buy');
    await page.waitForFunction(() => new URL(location.href).searchParams.get('output-source') === 'buy');
    await page.waitForFunction(() => document.querySelector('[data-testid="plan-total"]')?.textContent === '5,000 gil');
    assert.equal(await page.$('[aria-label="Ingredients"]'), null);
    assert.equal(await page.$('[aria-label="Crafting order"]'), null);
    const onlyOutput = async () => assert.deepEqual(await page.$$eval('[data-testid^="stop-"]', nodes => nodes.map(n => Number(n.dataset.testid.split('-')[1]))), [output]);
    await onlyOutput();
    const save = async quality => {
      const before = saves.length;
      await page.$$eval('button', nodes => nodes.find(n => n.textContent === 'Add finished items to a list').click());
      await page.waitForFunction(() => Array.from(document.querySelectorAll('button')).some(n => n.textContent === 'Root source fixture'));
      await page.$$eval('button', nodes => nodes.find(n => n.textContent === 'Root source fixture').click());
      await page.waitForFunction(() => document.body.textContent.includes('Items added to your list.'));
      assert.equal(saves.length, before + 1);
      assert.deepEqual(saves.at(-1).map(({ item_id, quantity, hq }) => ({ item_id, quantity, hq })), [{ item_id: output, quantity: 50, hq: quality }]);
      await page.$$eval('[role="dialog"] button', nodes => nodes.find(n => n.textContent === 'Close').click());
    };
    await save(false);
    // A purchased NQ stack must not remain pinned after changing the output to HQ.
    await page.click('[data-testid^="stop-"] input[type="checkbox"]');
    await page.click('[aria-label="HQ finished item"]');
    await page.waitForFunction(() => document.querySelector('[data-testid="plan-total"]')?.textContent === '10,000 gil');
    await onlyOutput();
    assert.equal(await page.$eval('[data-testid^="stop-"] input', n => n.checked), false);
    await save(true);
    await page.click('[data-testid^="stop-"] input[type="checkbox"]');
    await page.evaluate(() => {
      const url = new URL(location.href);
      url.searchParams.set('output-hq', 'false');
      const link = document.createElement('a');
      link.href = url.href;
      document.body.append(link);
      link.click();
      link.remove();
    });
    await page.waitForFunction(() => document.querySelector('[data-testid="plan-total"]')?.textContent === '5,000 gil');
    assert.equal(await page.$eval('[aria-label="HQ finished item"]', n => n.checked), false);
    assert.equal(await page.$eval('[data-testid^="stop-"] input', n => n.checked), false, 'URL quality change drops an incompatible purchased stack');
    await page.goBack();
    await page.waitForFunction(() => document.querySelector('[data-testid="plan-total"]')?.textContent === '10,000 gil');
    assert.equal(await page.$eval('[aria-label="HQ finished item"]', n => n.checked), true, 'Back restores output quality');
    const shared = page.url();
    await page.reload({ waitUntil: 'networkidle2' });
    await ready();
    assert.equal(page.url(), shared);
    assert.equal(await page.$eval('[aria-label="Main item source"]', n => n.value), 'buy');
    assert.equal(await page.$eval('[aria-label="HQ finished item"]', n => n.checked), true);
    assert.equal(await page.$eval('[data-testid="plan-total"]', n => n.textContent), '10,000 gil');
    await page.setViewport({ width: 1440, height: 1000 });
    await page.screenshot({ path: path.join(OUT, 'desktop.png'), fullPage: true });
    await page.setViewport({ width: 390, height: 844 });
    await page.screenshot({ path: path.join(OUT, 'mobile.png'), fullPage: true });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'no mobile horizontal overflow');
    shortage = true;
    await page.$$eval('button', nodes => nodes.find(n => n.textContent === 'Refresh prices').click());
    await page.waitForFunction(() => document.querySelector('[data-testid="output-comparison"]')?.textContent.includes('1 units unavailable'));
    assert.equal(await page.$eval('[data-testid="output-savings"]', n => n.textContent), '', 'partial output supply cannot claim savings');
    assert.ok(await page.$('[data-incomplete="true"]'));
    await page.select('[aria-label="Main item source"]', 'craft');
    await page.waitForSelector('[aria-label="Ingredients"]');
    assert.equal(await page.evaluate(() => new URL(location.href).searchParams.get('craft')), choice);
    assert.equal(await page.$eval(`select[aria-label=${JSON.stringify(component.label)}]`, n => n.value), component.value);
    assert.ok(await page.$('[aria-label="Crafting order"]'));
    await Promise.all([...requests]);
    assert.deepEqual(errors, [], 'no hydration or application errors');
    console.log('recipe root source: quantity, quality, itinerary, save, restore, shortage and responsive checks passed');
  } finally {
    try {
      if (page && !page.isClosed() && requestHandler) {
        page.off('request', requestHandler);
        await Promise.all([...requests]);
        await page.setRequestInterception(false);
      }
    } finally {
      await browser.close();
    }
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
