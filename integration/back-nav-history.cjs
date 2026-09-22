// Browser back navigation across client-side route changes.
//
// Regression probe for the `ShareLocale` / router race: the router defers its
// pushState for anchor clicks until the route's loaders finish, and the
// `?lang=` rewrite used to fire in that window as a *replace* navigation,
// clobbering the entry the user was leaving. Symptom: the back button skipped
// straight past the previous page.
//
//   BASE_URL=http://127.0.0.1:8080 node ./back-nav-history.cjs
// Defaults to the bare-link probe plus explicit-locale recommended/last/default
// cases for /items. BACK_NAV_CASE selects one case by name for diagnosis.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const START = process.env.START_PATH || '/';
const TARGET = process.env.TARGET_PATH || '/items';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

const cases = TARGET === '/items' ? [
  { name: 'bare-link', explicitLocale: false, expected: 'recommended' },
  { name: 'recommended', explicitLocale: true, expected: 'recommended' },
  { name: 'last-view', explicitLocale: true, expected: 'remembered', last: '?v=1&q=remembered' },
  { name: 'chosen-default', explicitLocale: true, expected: 'chosen', last: '?v=1&q=remembered', preferred: '?v=1&q=chosen' },
] : [{ name: 'bare-link', explicitLocale: false }];

function assertView(href, scenario) {
  if (!scenario.expected) return;
  const query = new URL(href).searchParams;
  assert.equal(query.get('v'), '1', `${scenario.name}: applied view must be explicit`);
  if (scenario.expected === 'recommended') {
    const filters = JSON.parse(query.get('gf') || '{}');
    assert.deepEqual(filters['market-listing-assessment'], { op: 'ne', value: 'suspicious' }, `${scenario.name}: recommended filter must land`);
    assert.equal(query.get('q'), null, `${scenario.name}: unrelated saved filters must not leak`);
  } else {
    assert.equal(query.get('q'), scenario.expected, `${scenario.name}: intended saved view must win`);
    assert.equal(query.get('gf'), null, `${scenario.name}: recommendations must not layer over the saved view`);
  }
}

async function waitForPath(page, pathname, timeout = 30000) {
  await page.waitForFunction(p => location.pathname === p, { timeout }, pathname);
}

async function runCase(browser, scenario) {
  // Cookies and device-local preferences from one case must not turn a fresh
  // landing in the next case into a restored visit.
  const context = await browser.createBrowserContext();
  try {
    const page = await context.newPage();
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

    await page.evaluate(({ last, preferred }) => {
      for (const key of ['ultros.last-view.items', 'ultros.grid.items-grid.default_view']) localStorage.removeItem(key);
      if (last !== undefined) localStorage.setItem('ultros.last-view.items', last);
      if (preferred !== undefined) localStorage.setItem('ultros.grid.items-grid.default_view', preferred);
    }, scenario);

    await page.evaluate(() => { window.__spaMarker = 'alive'; });
    const before = await page.evaluate(() => ({ length: history.length, href: location.href }));
    console.log(`[${scenario.name}] start: ${before.href} (history.length=${before.length})`);

    const clicked = await page.evaluate(({ target, explicitLocale }) => {
      const link = [...document.querySelectorAll(`a[href^="${target}"]`)][0];
      if (!link) return null;
      // Exercise the real app anchor/router handler. An explicit locale keeps
      // ShareLocale from accidentally hiding a seed/restore history race.
      if (explicitLocale) link.setAttribute('href', `${target}?lang=en`);
      link.click();
      return link.getAttribute('href');
    }, { target: TARGET, explicitLocale: scenario.explicitLocale });
    assert.ok(clicked, `no in-app link to ${TARGET} on ${START}`);
    await waitForPath(page, TARGET);
    await page.waitForFunction(() => /[?&]lang=/.test(location.search), { timeout: 15000 });
    await sleep(1500);

    const after = await page.evaluate(() => ({ length: history.length, href: location.href, marker: window.__spaMarker }));
    console.log(`[${scenario.name}] after click: ${after.href} (history.length=${after.length})`);
    assert.equal(after.marker, 'alive', 'link click must be a client-side route change');
    assert.equal(after.length, before.length + 1, 'route change must push exactly one history entry');
    assertView(after.href, scenario);

    await page.goBack({ waitUntil: 'domcontentloaded' });
    await waitForPath(page, START);
    const back = await page.evaluate(() => ({ length: history.length, href: location.href, marker: window.__spaMarker }));
    console.log(`[${scenario.name}] after back: ${back.href} (history.length=${back.length})`);
    assert.equal(new URL(back.href).pathname, START, 'back must return to the previous route');
    assert.equal(back.href, before.href, 'back must preserve the complete previous URL');
    assert.equal(back.marker, 'alive', 'back must stay client-side (no full reload)');

    await page.goForward({ waitUntil: 'domcontentloaded' });
    await waitForPath(page, TARGET);
    await sleep(500);
    const forward = await page.evaluate(() => ({ href: location.href, marker: window.__spaMarker, length: history.length }));
    console.log(`[${scenario.name}] after forward: ${forward.href}`);
    assert.equal(forward.marker, 'alive', 'forward must stay client-side (no full reload)');
    assert.equal(forward.length, after.length, 'back/forward must not add history entries');
    assertView(forward.href, scenario);
    console.log(`PASS back-nav-history ${scenario.name}`);
  } finally {
    await context.close();
  }
}

async function main() {
  const selected = process.env.BACK_NAV_CASE ? cases.filter(c => c.name === process.env.BACK_NAV_CASE) : cases;
  assert.ok(selected.length, `unknown BACK_NAV_CASE: ${process.env.BACK_NAV_CASE}`);
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const failures = [];
  try {
    for (const scenario of selected) {
      try { await runCase(browser, scenario); }
      catch (error) {
        console.error(`FAIL back-nav-history ${scenario.name}:`, error);
        failures.push(scenario.name);
      }
    }
    assert.deepEqual(failures, [], `back-nav cases failed: ${failures.join(', ')}`);
  } finally {
    await browser.close();
  }
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
