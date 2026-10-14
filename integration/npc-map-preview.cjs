// Exercise real hover/focus previews: a square map in a wide viewport must
// centre its pin vertically too, both before and after the lazy image loads.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';

(async () => {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  try {
    for (const width of [390, 1440]) {
      const page = await browser.newPage();
      page.setDefaultTimeout(90000);
      await page.setViewport({ width, height: 1000 });
      const errors = [];
      const maps = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.setRequestInterception(true);
      page.on('request', request => {
        const url = new URL(request.url());
        if (url.pathname.startsWith('/static/map/')) maps.push(request);
        else if (url.origin === new URL(BASE).origin || ['data:', 'blob:'].includes(url.protocol)) request.continue();
        else request.abort();
      });
      await page.evaluateOnNewDocument(() => {
        window.__hydrated = false;
        window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; });
      });
      await page.goto(`${BASE}/item/Gilgamesh/39595?lang=en`, { waitUntil: 'domcontentloaded' });
      await page.waitForFunction(() => window.__hydrated);
      const link = await page.waitForSelector('a[href="/npc/1001617"]');
      await link.evaluate(el => { el.scrollIntoView({ block: 'center' }); el.focus(); });
      await page.waitForSelector('[data-npc-map-preview]', { visible: true });

      async function checkGeometry(phase) {
        const g = await page.$eval('[data-npc-map-preview]', viewport => {
          const rect = viewport.getBoundingClientRect();
          const pin = viewport.querySelector('.zone-map-pin').getBoundingClientRect();
          const map = viewport.firstElementChild.getBoundingClientRect();
          return { x: pin.x + pin.width / 2 - rect.x, y: pin.y + pin.height / 2 - rect.y,
            width: rect.width, height: rect.height, mapWidth: map.width, mapHeight: map.height };
        });
        assert.ok(Math.abs(g.mapWidth - g.mapHeight) < 1, `${phase}: map is not square: ${JSON.stringify(g)}`);
        assert.ok(g.x > 0 && g.x < g.width && g.y > 0 && g.y < g.height,
          `${phase}: pin is clipped: ${JSON.stringify(g)}`);
        // This bundled city NPC is far enough from every image edge to centre.
        assert.ok(Math.abs(g.x - g.width / 2) < 3 && Math.abs(g.y - g.height / 2) < 3,
          `${phase}: pin is not centred: ${JSON.stringify(g)}`);
      }

      await checkGeometry('image pending');
      assert.ok(maps.length > 0, 'preview must request its map image');
      await Promise.all(maps.splice(0).map(request => request.continue()));
      await page.waitForFunction(() => {
        const img = document.querySelector('[data-npc-map-preview] img');
        return img?.complete && img.naturalWidth > 0;
      });
      await checkGeometry('image loaded');
      const out = path.join(__dirname, 'artifacts', 'npc-map-preview');
      fs.mkdirSync(out, { recursive: true });
      await page.screenshot({ path: path.join(out, `${width}.png`) });
      await page.keyboard.press('Escape');
      await page.waitForSelector('[data-npc-map-preview]', { hidden: true });
      await page.keyboard.press('Enter');
      await page.waitForFunction(() => location.pathname === '/npc/1001617');
      assert.deepEqual(errors, [], 'unexpected browser errors');
      console.log(`${width}: map pin centred before/after image load; Escape and NPC link passed`);
      await page.close();
    }
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
