// Regression probe for issue #1283: an item hover card left on screen after
// the user navigates.
//
// `HoverCard` (components/hover_card.rs) opens on `mouseenter` and closes on
// `mouseleave`. A navigation fires neither. On `/item/:world/:id` the hero
// icon's card is the clean case: the route matches the next URL too, so Leptos
// keeps the anchor element and patches it in place — the pointer never leaves
// anything, no `mouseleave` is dispatched, and the portal on `<body>` outlives
// the page it described. The user is then stuck with a card for the previous
// item that nothing on screen can dismiss.
//
// This is browser-only by construction: the dismissal rides a Leptos `Effect`,
// which does not run under the `ssr` feature the unit tests build with, and the
// symptom is a portal that survives a client-side route change.
//
// Asserts the outcome (no `[role="tooltip"]` survives a navigation) rather than
// the mechanism, so it stays valid whichever way the card closes.
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD = process.env.WORLD || 'Gilgamesh';
// A real bundled-data item, shared with item-source-nav.cjs.
const ITEM = process.env.ITEM || '5364';
const ROUTE = `/item/${WORLD}/${ITEM}`;
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
// `ItemTooltip` waits out 300ms of sustained hover before opening.
const OPEN_DELAY_MS = 300;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Same-route destination: another item under the same `/item/:world/:id`. */
function findSiblingItemLink() {
  const here = location.pathname;
  const link = [...document.querySelectorAll('a[href]')].find((a) => {
    const href = a.getAttribute('href') || '';
    return /^\/item\/[^/]+\/\d+$/.test(href) && href !== here;
  });
  return link ? link.getAttribute('href') : null;
}

/**
 * Follow `href` the way a link click does, without moving the pointer — the
 * whole point is that the cursor stays parked on the hovered anchor. The
 * router intercepts clicks on same-origin anchors through one global handler,
 * so a programmatic click navigates client-side exactly as a real one does.
 */
function clickLink(href) {
  const link = document.querySelector(`a[href="${href}"]`);
  if (!link) throw new Error(`link to ${href} vanished`);
  link.click();
}

const countTooltips = () => document.querySelectorAll('[role="tooltip"]').length;

async function main() {
  const browser = await puppeteer.launch({
    headless: true,
    args: ['--no-sandbox', '--disable-dev-shm-usage'],
  });
  const failures = [];

  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(TIMEOUT_MS);
    const { hostname } = new URL(BASE_URL);
    await page.setCookie({ name: 'HIDE_ADS', value: 'true', domain: hostname, path: '/' });
    await page.evaluateOnNewDocument(() => {
      window.__hydrated = false;
      window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; }, { once: true });
    });
    await page.setViewport({ width: 1440, height: 1024, deviceScaleFactor: 1 });
    await page.goto(`${BASE_URL}${ROUTE}`, { waitUntil: 'domcontentloaded', timeout: TIMEOUT_MS });
    await page.waitForFunction(() => window.__hydrated === true);

    // The hero icon is the anchor that survives an item -> item navigation.
    await page.waitForSelector('img.icon-large');
    await page.hover('img.icon-large');
    await sleep(OPEN_DELAY_MS + 400);

    const opened = await page.evaluate(countTooltips);
    if (opened === 0) {
      throw new Error(
        'hovering the hero item icon opened no hover card — the probe cannot ' +
          'see the bug it is guarding against',
      );
    }
    console.log(`[info] ${ROUTE}: hovering the hero icon opened ${opened} hover card(s)`);

    const href = await page.evaluate(findSiblingItemLink);
    if (!href) throw new Error(`${ROUTE} offers no other /item/:world/:id link to navigate to`);

    await page.evaluate(clickLink, href);
    await page.waitForFunction((from) => location.pathname !== from, {}, ROUTE);
    // Give the route's render, and any effect it schedules, room to settle.
    await sleep(1500);

    const left = await page.evaluate(() => ({
      count: document.querySelectorAll('[role="tooltip"]').length,
      texts: [...document.querySelectorAll('[role="tooltip"]')]
        .map((t) => (t.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 60)),
      pathname: location.pathname,
    }));
    console.log(`[info] navigated ${ROUTE} -> ${left.pathname}`);
    if (left.count > 0) {
      failures.push(
        `${left.count} hover card(s) survived the navigation to ${left.pathname}: ` +
          `${JSON.stringify(left.texts)} — nothing on the new page can dismiss them`,
      );
    }
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error('[fail] hover cards outlive navigation:');
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log('[done] the hover card is dismissed by navigation');
}

main().catch((err) => {
  console.error(`[fail] ${err && err.stack ? err.stack : err}`);
  process.exit(1);
});
