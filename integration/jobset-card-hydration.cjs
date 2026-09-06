// Regression probe for the gear-set cards on `/items/jobset/<JOB>` losing
// their NQ/HQ totals on a direct (SSR + hydrate) load.
//
// The totals render outside any <Suspense>, so SSR ships the "—" placeholder
// with the cheapest-listings resource still pending, while the client has the
// resolved resource on its first render. tachys *adopts* the SSR text node
// during hydration without writing to it, so the DOM stayed on "—" even though
// the reactive graph held the real total — and only a later *change* to the
// value (switching worlds, hence a refetch) ever patched it in.
//
// Unit tests can't see this: it only exists in the SSR-payload/hydration seam.
// So we compare the same component reached two ways:
//
//   direct load  -> hydrated from SSR markup   (the broken path)
//   client nav   -> built fresh by the router  (the known-good path)
//
// If they disagree, hydration dropped the values. The check is deliberately
// data-independent: against a server with no market data both paths render
// "—" and the probe passes rather than failing spuriously.
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const JOBSET = process.env.JOBSET || 'SAM';
const WORLD = process.env.WORLD || 'Gilgamesh';
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
// Hydration + the cheapest-listings fetch both have to land before we read.
const SETTLE_MS = Number(process.env.SETTLE || 8000);

const ROUTE = `/items/jobset/${JOBSET}?world=${encodeURIComponent(WORLD)}`;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * Scrape one row per gear-set card: its heading plus the rendered NQ/HQ
 * totals. Keyed on the card's stem so a differing set order between the two
 * loads can't produce a false mismatch.
 */
function readCards() {
  const heading = [...document.querySelectorAll('h4')].find((el) =>
    /gear sets/i.test(el.textContent || ''),
  );
  if (!heading || !heading.nextElementSibling) return null;
  return [...heading.nextElementSibling.children].map((card) => {
    const lines = (card.innerText || '')
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean);
    const totalAfter = (label) => {
      const i = lines.findIndex((l) => l.toUpperCase() === label);
      return i >= 0 && i + 1 < lines.length ? lines[i + 1] : '?';
    };
    return {
      // The set name sits after the "iLvl N" and "N pieces" chips.
      stem: lines[2] || '?',
      nq: totalAfter('NQ TOTAL'),
      hq: totalAfter('HQ TOTAL'),
    };
  });
}

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

    // 1. Direct load: SSR markup, hydrated in place. This is the path that regressed.
    await page.goto(`${BASE_URL}${ROUTE}`, { waitUntil: 'domcontentloaded', timeout: TIMEOUT_MS });
    await sleep(SETTLE_MS);
    const hydrated = await page.evaluate(readCards);

    if (!hydrated) {
      throw new Error(`no "Gear sets" section rendered at ${ROUTE}`);
    }
    if (!hydrated.length) {
      throw new Error(`"Gear sets" section at ${ROUTE} rendered no cards`);
    }

    // 2. Client-side nav to the same route. Leaving and coming back rebuilds
    //    the cards through the router instead of hydrating them, so the
    //    resource value reaches the DOM the normal reactive way.
    await page.goto(`${BASE_URL}/items`, { waitUntil: 'domcontentloaded', timeout: TIMEOUT_MS });
    await sleep(SETTLE_MS);
    const navigated = await page.evaluate(async (route) => {
      const link = document.createElement('a');
      link.href = route;
      document.body.appendChild(link);
      link.dispatchEvent(
        new MouseEvent('click', { bubbles: true, cancelable: true, view: window, button: 0 }),
      );
      // The router updates history asynchronously — poll rather than read
      // location straight back, which still shows the old URL.
      const deadline = Date.now() + 10000;
      while (Date.now() < deadline && location.pathname === '/items') {
        await new Promise((r) => setTimeout(r, 100));
      }
      link.remove();
      return location.pathname;
    }, ROUTE);
    if (!navigated.includes(JOBSET)) {
      throw new Error(`client-side navigation to ${ROUTE} did not take (at ${navigated})`);
    }
    await sleep(SETTLE_MS);
    const fresh = await page.evaluate(readCards);
    if (!fresh || !fresh.length) {
      throw new Error(`"Gear sets" section rendered no cards after client-side nav`);
    }

    // 3. Differential. Every set present on both paths must show the same totals.
    const freshByStem = new Map(fresh.map((c) => [c.stem, c]));
    for (const card of hydrated) {
      const other = freshByStem.get(card.stem);
      if (!other) continue;
      if (card.nq !== other.nq || card.hq !== other.hq) {
        failures.push(
          `"${card.stem}": direct load showed NQ=${card.nq} HQ=${card.hq}, ` +
            `client-side nav showed NQ=${other.nq} HQ=${other.hq} ` +
            `— hydration dropped the totals`,
        );
      }
    }

    const withPrices = fresh.filter((c) => c.nq !== '—' || c.hq !== '—').length;
    console.log(
      `[info] ${hydrated.length} card(s) on direct load, ${fresh.length} after client-side nav, ` +
        `${withPrices} with price data`,
    );
    if (!withPrices) {
      console.log('[info] no market data for this job set — differential is vacuous here');
    }
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} gear-set card(s) lost their totals on hydration:`);
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log('[done] gear-set card totals survive hydration');
}

main().catch((err) => {
  console.error(`[fail] ${err && err.stack ? err.stack : err}`);
  process.exit(1);
});
