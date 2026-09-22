// Browser back navigation across client-side route changes.
//
// Regression probe for the `ShareLocale` / router race: the router defers its
// pushState for anchor clicks until the route's loaders finish, and the
// `?lang=` rewrite used to fire in that window as a *replace* navigation,
// clobbering the entry the user was leaving. Symptom: the back button skipped
// straight past the previous page.
//
//   BASE_URL=http://127.0.0.1:8080 node ./back-nav-history.cjs
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const START = process.env.START_PATH || '/';
const TARGET = process.env.TARGET_PATH || '/items';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function waitForPath(page, pathname, timeout = 30000) {
  await page.waitForFunction(p => location.pathname === p, { timeout }, pathname);
}

async function main() {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(60000);
    const { hostname } = new URL(BASE_URL);
    await page.setCookie({ name: 'HIDE_ADS', value: 'true', domain: hostname, path: '/' });
    await page.setRequestInterception(true);
    page.on('request', req => {
      if (/googlesyndication|doubleclick|adtrafficquality|googletagservices|google-analytics/.test(req.url())) {
        return req.abort().catch(() => {});
      }
      req.continue().catch(() => {});
    });
    await page.evaluateOnNewDocument(() => {
      window.__hydrated = false;
      window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; }, { once: true });
    });

    await page.goto(`${BASE_URL}${START}`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__hydrated, { timeout: 60000 });
    // Let the landing-page `?lang=` replace settle so it isn't counted below.
    await page.waitForFunction(() => /[?&]lang=/.test(location.search), { timeout: 15000 });
    await sleep(500);

    await page.evaluate(() => { window.__spaMarker = 'alive'; });
    const before = await page.evaluate(() => ({ length: history.length, href: location.href }));
    console.log(`start: ${before.href} (history.length=${before.length})`);

    const clicked = await page.evaluate(target => {
      const link = [...document.querySelectorAll(`a[href^="${target}"]`)][0];
      if (!link) return null;
      link.click();
      return link.getAttribute('href');
    }, TARGET);
    assert.ok(clicked, `no in-app link to ${TARGET} on ${START}`);
    await waitForPath(page, TARGET);
    await page.waitForFunction(() => /[?&]lang=/.test(location.search), { timeout: 15000 });
    await sleep(1500);

    const after = await page.evaluate(() => ({ length: history.length, href: location.href, marker: window.__spaMarker }));
    console.log(`after click: ${after.href} (history.length=${after.length})`);
    assert.equal(after.marker, 'alive', 'link click must be a client-side route change');
    assert.equal(after.length, before.length + 1, 'route change must push exactly one history entry');

    await page.goBack({ waitUntil: 'domcontentloaded' });
    await waitForPath(page, START);
    const back = await page.evaluate(() => ({ length: history.length, href: location.href, marker: window.__spaMarker }));
    console.log(`after back: ${back.href} (history.length=${back.length})`);
    assert.equal(new URL(back.href).pathname, START, 'back must return to the previous route');
    assert.equal(back.marker, 'alive', 'back must stay client-side (no full reload)');

    await page.goForward({ waitUntil: 'domcontentloaded' });
    await waitForPath(page, TARGET);
    console.log(`after forward: ${await page.evaluate(() => location.href)}`);
    console.log('PASS back-nav-history');
  } finally {
    await browser.close();
  }
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
