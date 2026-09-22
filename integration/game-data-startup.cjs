// Exercise actual hydration with the projected catalog, not the full SSR data.
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { createHash } = require('node:crypto');
const path = require('node:path');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const hash = (directory, lang) => createHash('sha256').update(readFileSync(path.join(__dirname, `../data/${directory}/${lang}.rkyv`))).digest('hex').slice(0, 16);

(async () => {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox', '--disable-dev-shm-usage'] });
  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(90000);
    const requests = [];
    const errors = [];
    page.on('request', request => requests.push(new URL(request.url()).pathname));
    page.on('pageerror', error => errors.push(error.message));
    await page.setRequestInterception(true);
    page.on('request', request => {
      const url = new URL(request.url());
      if (url.origin === new URL(BASE).origin || ['data:', 'blob:'].includes(url.protocol)) request.continue();
      else request.abort();
    });
    await page.evaluateOnNewDocument(() => {
      window.__hydrated = false;
      window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; });
      const open = IDBFactory.prototype.open;
      IDBFactory.prototype.open = function (name, ...args) {
        if (name === 'ultros') throw new Error('game-data IndexedDB must not be required');
        return open.call(this, name, ...args);
      };
    });
    await page.goto(`${BASE}/item/Gilgamesh/5333?lang=en`, { waitUntil: 'domcontentloaded', timeout: 90000 });
    await page.waitForFunction(() => window.__hydrated);
    assert.ok(requests.includes(`/static/startup/${hash('xiv-startup', 'en')}/en.rkyv`));
    assert.ok(!requests.some(url => url.startsWith('/static/data/')), 'full catalog downloaded');
    assert.ok(!requests.some(url => url.includes('/description/')), 'description fetched before hover');
    await page.hover('img.icon-large');
    await page.waitForSelector('[data-testid="item-description"]');
    assert.ok(await page.$eval('[data-testid="item-description"]', el => el.textContent.trim().length > 0));
    assert.ok(requests.includes(`/static/game-detail/${hash('xiv-db', 'en')}/en/description/5333`));

    // Delivery moogle: verified present in the full pack but omitted from
    // the startup projection, exercising the fallback rather than a vendor.
    const npcId = 1000063;
    const npcResponse = await fetch(`${BASE}/static/game-detail/${hash('xiv-db', 'en')}/en/npc/${npcId}`);
    assert.equal(npcResponse.status, 200);
    const row = await npcResponse.json();
    assert.ok(row?.singular.trim(), 'NPC fixture missing');
    const npc = { id: npcId, name: row.singular };
    await page.evaluate(id => {
      const link = document.createElement('a');
      link.href = `/npc/${id}?lang=en`;
      document.body.append(link);
      link.click();
      link.remove();
    }, npc.id);
    await page.waitForFunction(name => document.querySelector('h1')?.textContent.includes(name), {}, npc.name);
    assert.ok(requests.some(url => url.endsWith(`/npc/${npc.id}`)), 'client navigation did not fetch NPC details');
    const response = await page.goto(`${BASE}/npc/${npc.id}?lang=en`, { waitUntil: 'domcontentloaded', timeout: 90000 });
    assert.ok((await response.text()).includes(npc.name), 'NPC absent from SSR response');
    await page.waitForFunction(() => window.__hydrated);
    await page.waitForFunction(name => document.querySelector('h1')?.textContent.includes(name), {}, npc.name);
    assert.ok((await page.$eval('meta[property="og:title"]', el => el.content)).includes(npc.name), 'NPC social metadata lost on hydration');
    await page.goto(`${BASE}/item/Gilgamesh/5333?lang=ja`, { waitUntil: 'domcontentloaded', timeout: 90000 });
    await page.waitForFunction(() => window.__hydrated);
    assert.ok(requests.includes(`/static/startup/${hash('xiv-startup', 'ja')}/ja.rkyv`));
    await page.hover('img.icon-large');
    await page.waitForSelector('[data-testid="item-description"]');
    assert.ok(requests.includes(`/static/game-detail/${hash('xiv-db', 'ja')}/ja/description/5333`), 'description did not follow locale');
    assert.deepEqual(errors, []);
    console.log('PASS: projected startup, no game-data IndexedDB, deferred localized tooltip, NPC navigation and SSR hydration');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
