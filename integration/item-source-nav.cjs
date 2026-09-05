// Focused item-source navigation probe. Use this worktree's freshly built server.
// ITEM_ROUTES is a comma-separated list of real item routes to exercise.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
// Real bundled-data items spanning crafting, exchange, vendors, leve rewards,
// and an item that currently has none of those acquisition methods.
const ROUTES = (process.env.ITEM_ROUTES ||
  '/item/Gilgamesh/5364,/item/Gilgamesh/23892,/item/Gilgamesh/13709,/item/Gilgamesh/39643,/item/Gilgamesh/2').split(',');
const OUTPUT = path.join(__dirname, 'artifacts', 'item-source-nav');
const NAV = '[data-item-section-nav]';
const SOURCE_ANCHORS = ['#crafting-recipes', '#exchange-sources', '#leve-sources', '#vendor-sources'];
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function readLinks(page) {
  return page.$$eval(`${NAV} a`, links => links.map(link => ({
    href: link.getAttribute('href'),
    text: link.textContent.trim(),
    label: link.getAttribute('aria-label'),
  })));
}

async function main() {
  fs.mkdirSync(OUTPUT, { recursive: true });
  const browser = await puppeteer.launch({ headless: 'new', args: ['--no-sandbox'] });
  console.log(`Browser: ${await browser.version()}`);
  const coverage = new Set();
  const results = [];
  const ignoredThirdPartyErrors = [];
  try {
    for (const route of ROUTES) {
      const page = await browser.newPage();
      page.setDefaultTimeout(60000);
      await page.evaluateOnNewDocument(() => {
        window.__sourceNavHydrated = false;
        window.addEventListener('ultros:hydrated', () => { window.__sourceNavHydrated = true; }, { once: true });
      });
      const errors = [];
      page.on('pageerror', error => {
        const detail = error.stack || error.message;
        // Match the existing runner's known AdSense/headless exception by
        // script origin, never by its unstable minified exception name.
        if (detail.includes('https://pagead2.googlesyndication.com/')) {
          ignoredThirdPartyErrors.push({ route, detail });
        } else {
          errors.push(detail);
        }
      });
      await page.setViewport({ width: 1440, height: 1024, deviceScaleFactor: 1 });
      await page.setJavaScriptEnabled(false);
      await page.goto(`${BASE_URL}${route}`, { waitUntil: 'networkidle2' });
      const ssr = await readLinks(page);
      assert.ok(ssr.length >= 5, `${route}: SSR navigation missing`);
      await page.setJavaScriptEnabled(true);
      await page.reload({ waitUntil: 'networkidle2' });
      await page.waitForFunction(() => window.__sourceNavHydrated === true);
      await sleep(500);
      assert.deepEqual(await readLinks(page), ssr, `${route}: source links changed during hydration`);
      const sources = ssr.filter(link => SOURCE_ANCHORS.includes(link.href));
      sources.forEach(link => coverage.add(link.href));
      const accents = await page.$$eval(`${NAV} a`, links => ({
        ordinary: getComputedStyle(links[0]).color,
        sources: links.slice(5).map(link => getComputedStyle(link).color),
      }));
      assert.ok(accents.sources.every(color => color !== accents.ordinary),
        `${route}: source accents were overridden by ordinary link styles`);

      for (const width of [1440, 390]) {
        await page.setViewport({ width, height: width === 390 ? 844 : 1024, deviceScaleFactor: 1 });
        await page.goto(`${BASE_URL}${route}`, { waitUntil: 'networkidle2' });
        await page.waitForFunction(() => window.__sourceNavHydrated === true);
        await sleep(500);
        const prefix = `${route.replace(/[^a-z0-9-]/gi, '_')}-${width}`;
        await page.screenshot({ path: path.join(OUTPUT, `${prefix}-initial.png`) });
        await page.$eval(NAV, nav => nav.scrollIntoView({ block: 'start' }));
        await sleep(500);
        const nav = await page.$(NAV);
        const bar = (await nav.evaluateHandle(node => node.parentElement.parentElement)).asElement();
        await bar.screenshot({ path: path.join(OUTPUT, `${prefix}-nav-start.png`) });

        // Tab from Related into every available source, including off-screen links.
        await page.focus(`${NAV} a[href="#related"]`);
        for (const source of sources) {
          await page.keyboard.press('Tab');
          // Keyboard focus can animate the native horizontal scrollport.
          await page.waitForFunction(() => {
            const rect = document.activeElement.getBoundingClientRect();
            const navRect = document.activeElement.closest('[data-item-section-nav]').getBoundingClientRect();
            return rect.left >= navRect.left - 1 && rect.right <= navRect.right + 1;
          }, { timeout: 3000 });
          const focused = await page.evaluate(() => ({
            href: document.activeElement.getAttribute('href'),
            left: document.activeElement.getBoundingClientRect().left,
            right: document.activeElement.getBoundingClientRect().right,
            outline: getComputedStyle(document.activeElement).outlineStyle,
          }));
          assert.equal(focused.href, source.href, `${route}: keyboard source order`);
          assert.ok(focused.left >= 0 && focused.right <= width + 1,
            `${route} ${width}: focused source clipped: ${JSON.stringify(focused)}`);
          assert.notEqual(focused.outline, 'none', `${route}: missing keyboard focus outline`);
        }
        await bar.screenshot({ path: path.join(OUTPUT, `${prefix}-nav-sources.png`) });

        for (const source of sources) {
          const link = await page.$(`${NAV} a[href="${source.href}"]`);
          await link.click();
          await page.waitForFunction(hash => location.hash === hash, {}, source.href);
          await sleep(3500);
          const placement = await page.evaluate(({ selector, href }) => {
            const bar = document.querySelector(selector).parentElement.parentElement;
            const target = document.querySelector(href);
            const rect = target.getBoundingClientRect();
            return { top: rect.top, bottom: rect.bottom, barBottom: bar.getBoundingClientRect().bottom,
              viewportHeight: innerHeight, visible: target.getClientRects().length > 0,
              scrollMarginTop: getComputedStyle(target).scrollMarginTop };
          }, { selector: NAV, href: source.href });
          await page.screenshot({ path: path.join(OUTPUT, `${prefix}-${source.href.slice(1)}.png`) });
          assert.ok(placement.visible, `${route}: hidden ${source.href}`);
          assert.ok(placement.top >= placement.barBottom - 1 && placement.top < placement.viewportHeight,
            `${route} ${width}: obscured destination ${source.href}: ${JSON.stringify(placement)}`);
        }
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
        assert.ok(overflow <= 1, `${route} ${width}: document overflow ${overflow}px`);
        results.push({ route, width, sources });
      }
      assert.deepEqual(errors, [], `${route}: browser errors`);
      await page.close();
    }
    const report = { passed: true, coverage: [...coverage], results, ignoredThirdPartyErrors };
    fs.writeFileSync(path.join(OUTPUT, 'report.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  } finally {
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
