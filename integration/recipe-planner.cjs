// Uses a freshly built local server. Market fixtures make this independent of
// a populated market DB; game data, routes, rendering and WASM are all real.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const OUT = path.join(__dirname, 'artifacts', 'recipe-planner');

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  let page;
  try {
    page = await browser.newPage();
    page.setDefaultTimeout(90000);
    const errors = [];
    page.on('pageerror', error => {
      const message = error.stack || String(error);
      if (!message.includes('pagead2.googlesyndication.com')) errors.push(message);
    });
    await page.evaluateOnNewDocument(() => {
      window.__recipeHydrated = false;
      window.addEventListener('ultros:hydrated', () => { window.__recipeHydrated = true; });
    });
    // Discover a real recipe with a craftable intermediate from the same
    // bundled-data item fixtures as item-source-nav. Verify the entry point too.
    await page.setJavaScriptEnabled(false);
    let href;
    for (const item of [5364, 13709, 39643, 23892]) {
      await page.goto(`${BASE}/item/Gilgamesh/${item}`, { waitUntil: 'domcontentloaded' });
      const links = await page.$$eval('a[href^="/recipe/"]', links => links.map(a => a.getAttribute('href')));
      for (const link of links.slice(0, 3)) {
        await page.goto(new URL(link, BASE).href, { waitUntil: 'domcontentloaded' });
        if (await page.$$eval('select[aria-label^="Source for"]', nodes => nodes.some(n => n.options.length > 1))) {
          href = link;
          break;
        }
      }
      if (href) break;
    }
    assert.ok(href, 'bundled fixtures must exercise a craftable intermediate');
    assert.match(href, /^\/recipe\/\d+\?/);
    const url = new URL(href, BASE);
    url.searchParams.set('world', 'Gilgamesh');
    url.searchParams.set('quantity', '2');
    url.searchParams.set('shards-exclude', 'false');
    await page.goto(url.href, { waitUntil: 'domcontentloaded' });
    const ssrTitle = await page.$eval('[data-testid="recipe-planner"] h1', e => e.textContent);
    assert.ok(ssrTitle.trim());
    assert.equal(await page.$eval('[aria-label="Items to make"]', e => e.value), '2', 'quantity must be visible before hydration');
    assert.equal(await page.$eval('[aria-label="Starting world"]', e => e.value), 'Gilgamesh');
    assert.equal(await page.$eval('[aria-label="Buy from"]', e => e.value), 'datacenter');
    await page.setJavaScriptEnabled(true);
    await page.setRequestInterception(true);
    page.on('request', request => {
      const match = new URL(request.url()).pathname.match(/^\/api\/v1\/listings\/[^/]+\/(\d+)$/);
      if (!match) return request.continue();
      const item = Number(match[1]);
      const listings = [
        { id: item * 10 + 1, world_id: 63, quantity: 99, price_per_unit: 100 },
        { id: item * 10 + 2, world_id: 63, quantity: 3, price_per_unit: 150 },
        { id: item * 10 + 3, world_id: 79, quantity: 12, price_per_unit: 50 },
      ].map(l => [{ ...l, item_id: item, retainer_id: l.id, hq: false, timestamp: '2026-09-05T12:00:00' },
        { id: l.id, world_id: l.world_id, name: 'Recipe fixture', retainer_city_id: 1 }]);
      return request.respond({ status: 200, contentType: 'application/json', body: JSON.stringify({ listings, sales: [], last_updated: [{ world_id: 63, updated_at: '2026-09-05T12:00:00' }, { world_id: 79, updated_at: '2026-09-05T12:00:00' }] }) });
    });
    await page.reload({ waitUntil: 'networkidle2' });
    await page.waitForFunction(() => window.__recipeHydrated);
    await page.waitForFunction(() => document.querySelector('[data-testid="plan-total"]')?.textContent.includes('gil'));
    assert.equal(await page.$eval('body', e => e.textContent.includes('Some ingredient markets could not be loaded.')), false, 'market fixtures must deserialize');
    assert.equal(await page.$eval('h1', e => e.textContent), ssrTitle);
    const first = await page.$eval('[data-testid="plan-total"]', e => e.textContent);
    await page.$eval('[aria-label="Items to make"]', e => { e.value = '5'; e.dispatchEvent(new Event('change', { bubbles: true })); });
    await page.waitForFunction(() => new URL(location.href).searchParams.get('quantity') === '5');
    const before = await page.$$eval('[data-testid^="material-"]', rows => rows.map(r => r.textContent));
    const source = await page.$$eval('select[aria-label^="Source for"]', selects => {
      const s = selects.find(s => s.options.length > 1);
      return s ? { label: s.getAttribute('aria-label'), value: s.options[1].value } : null;
    });
    assert.ok(source, 'the chosen fixture must have a craftable intermediate');
    {
      await page.select(`select[aria-label=${JSON.stringify(source.label)}]`, source.value);
      await page.waitForFunction(() => new URL(location.href).searchParams.has('craft'));
      await page.waitForFunction(() => document.querySelector('[data-testid="plan-total"]')?.textContent.includes('gil'));
      const after = await page.$$eval('[data-testid^="material-"]', rows => rows.map(r => r.textContent));
      assert.notDeepEqual(after, before, 'craft choice should change materials');
    }
    await page.$eval('input[aria-label^="Already have"]', e => { e.value = '1'; e.dispatchEvent(new Event('change', { bubbles: true })); });
    await page.waitForFunction(() => new URL(location.href).searchParams.has('owned'));
    const shared = await page.evaluate(() => location.href);
    const ownedLabel = await page.$eval('input[aria-label^="Already have"]', e => e.getAttribute('aria-label'));
    await page.reload({ waitUntil: 'networkidle2' });
    await page.waitForFunction(() => window.__recipeHydrated);
    await page.waitForFunction(() => document.querySelector('[data-testid="plan-total"]')?.textContent.includes('gil'));
    assert.equal(await page.evaluate(() => location.href), shared);
    assert.equal(await page.$eval('[aria-label="Items to make"]', e => e.value), '5');
    assert.equal(await page.$eval(`select[aria-label=${JSON.stringify(source.label)}]`, e => e.value), source.value);
    assert.equal(await page.$eval(`input[aria-label=${JSON.stringify(ownedLabel)}]`, e => e.value), '1');
    assert.equal(await page.$eval('[data-testid="recipe-planner"]', e => e.textContent.includes('Item -1')), false);
    const shareLabel = await page.$eval('header button[aria-label^="Copy https://ultros.app/recipe/"]', e => e.getAttribute('aria-label'));
    const shareUrl = new URL(shareLabel.replace(/^Copy /, '').replace(/ to clipboard$/, ''));
    assert.deepEqual(shareUrl.searchParams.getAll('world'), ['Gilgamesh']);
    await page.click('section[aria-label="World visit comparison"] button');
    await page.waitForFunction(() => new URL(location.href).searchParams.get('visits') === '0');
    assert.equal(await page.$eval('section[aria-label="World visit comparison"] button', e => e.getAttribute('aria-pressed')), 'true');
    assert.ok(await page.$eval('aside[aria-label="Plan summary"]', e => e.textContent.includes('0 additional worlds')));
    for (const width of [1440, 390]) {
      await page.setViewport({ width, height: width === 390 ? 844 : 1000 });
      await page.reload({ waitUntil: 'networkidle2' });
      await page.waitForFunction(() => window.__recipeHydrated);
      await page.waitForFunction(() => document.querySelector('[data-testid="plan-total"]')?.textContent.includes('gil'));
      await page.screenshot({ path: path.join(OUT, `${width}.png`), fullPage: true });
      await page.screenshot({ path: path.join(OUT, `${width}-viewport.png`) });
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      assert.ok(overflow <= 1, `page overflows at ${width}: ${overflow}`);
    }
    await page.$$eval('button', buttons => buttons.find(b => b.textContent === 'Add remaining materials to a list').click());
    await page.waitForFunction(() => document.body.textContent.includes('Sign in to save this plan to a list.'));
    assert.equal(await page.evaluate(() => new URL(location.href).searchParams.get('owned')), new URL(shared).searchParams.get('owned'), 'opening Save must preserve the public plan');
    assert.deepEqual(errors, [], 'browser errors');
    fs.writeFileSync(path.join(OUT, 'report.json'), JSON.stringify({ passed: true, href, first, shared, subcraft: source }, null, 2));
    console.log('Recipe planner: SSR, hydration, quantities, owned inventory, shared links and layouts passed.');
  } catch (error) {
    if (page && !page.isClosed()) {
      await page.screenshot({ path: path.join(OUT, 'failure.png'), fullPage: true }).catch(() => {});
    }
    throw error;
  } finally { await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
