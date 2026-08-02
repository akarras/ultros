// Regression probe for the Flip Finder's sticky control bar overflowing at
// phone widths (issues #1055 / #1057).
//
// The bar is height-locked to STICKY_BAR_HEIGHT (components/filter_chip.rs) so
// the table header can stick directly beneath it, which means it cannot wrap.
// It also must not scroll: its top row owns the saved-views and columns
// popovers, and an `overflow-x` there computes `overflow-y: auto` too and
// would trap them in a 32px strip. So the row has to *fit*, and when it
// didn't the surplus went past `html { overflow-x: hidden }` — leaving
// `Columns` and `Clear all` rendered outside the viewport with no scrollbar
// and no wrap to reach them. At 393px that was 177px of unreachable controls.
//
// Anything added to that row has to keep it fitting, by any means: a
// breakpoint, shrinking, truncation. This asserts the outcome rather than the
// mechanism, so it stays valid across whichever the row uses. Layout is the
// whole bug, so it can only be checked in a browser.
//
// Not covered: the 768-1024px band with German labels, where the side nav
// claims 240px and the row is barely wider than it is at 768px. Switching
// locale needs the in-app picker rather than a cookie, so it wants a separate
// probe.
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD = process.env.WORLD || 'Gilgamesh';
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
const SETTLE_MS = Number(process.env.SETTLE || 9000);

// Widths worth guarding. 360 is the common Android floor and 393/414 cover
// current iPhone and Pixel classes. 320 is deliberately absent: with the
// shell's 48px of padding it leaves 256px for the row, which the controls
// cannot fit even icon-only, and failing there would just be noise.
const WIDTHS = [360, 393, 414];

// Enough active filters that the chip strip is populated, since it shares the
// bar with the controls under test.
const ROUTE = `/flip-finder/${WORLD}?profit=10000&roi=50&ppd=5000`;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Measure the sticky bar against the layout viewport. */
function measureBar() {
  const bar = document.querySelector('.sticky-bar');
  if (!bar) return null;
  const row1 = bar.children[0];
  // `innerWidth` lies under Chrome's mobile emulation; the layout viewport is
  // what the CSS actually resolved against.
  const viewport = document.documentElement.clientWidth;
  const controls = [...row1.querySelectorAll('button')].map((b) => {
    const r = b.getBoundingClientRect();
    // A control the user cannot land a finger on is as good as absent, so
    // check the DOM at the button's own centre rather than trusting the rect.
    const hit = document.elementFromPoint((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    return {
      name: (b.getAttribute('aria-label') || b.textContent || '').trim().slice(0, 24) || '<button>',
      right: Math.round(r.right),
      reachable: Boolean(hit) && (hit === b || b.contains(hit)),
    };
  });
  return {
    viewport,
    barOverflow: bar.scrollWidth - bar.clientWidth,
    rowOverflow: row1.scrollWidth - row1.clientWidth,
    controls,
  };
}

async function main() {
  const browser = await puppeteer.launch({
    headless: 'new',
    args: ['--no-sandbox', '--disable-dev-shm-usage'],
  });
  const failures = [];

  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(TIMEOUT_MS);
    const { hostname } = new URL(BASE_URL);
    await page.setCookie({ name: 'HIDE_ADS', value: 'true', domain: hostname, path: '/' });
    await page.setViewport({ width: WIDTHS[0], height: 852, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
    await page.goto(`${BASE_URL}${ROUTE}`, { waitUntil: 'domcontentloaded', timeout: TIMEOUT_MS });
    await sleep(SETTLE_MS);

    for (const width of WIDTHS) {
      await page.setViewport({ width, height: 852, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
      await sleep(500);
      const m = await page.evaluate(measureBar);
      if (!m) throw new Error(`no .sticky-bar rendered at ${ROUTE}`);
      if (m.viewport !== width) {
        throw new Error(`asked for a ${width}px viewport but CSS resolved against ${m.viewport}px`);
      }

      if (m.rowOverflow > 0 || m.barOverflow > 0) {
        failures.push(
          `${width}px: control bar overflows its box by ${Math.max(m.rowOverflow, m.barOverflow)}px ` +
            `— the surplus is clipped by html{overflow-x:hidden} and cannot be scrolled to`,
        );
      }
      for (const c of m.controls) {
        if (c.right > width) {
          failures.push(`${width}px: "${c.name}" ends at x=${c.right}, past the viewport edge`);
        } else if (!c.reachable) {
          failures.push(`${width}px: "${c.name}" is within the viewport but not hit-testable`);
        }
      }
      console.log(
        `[info] ${width}px: ${m.controls.length} control(s), ` +
          `row overflow ${m.rowOverflow}px, bar overflow ${m.barOverflow}px`,
      );
    }
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] the Flip Finder control bar does not fit on mobile:`);
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log('[done] control bar fits, and every control is reachable, at every phone width tested');
}

main().catch((err) => {
  console.error(`[fail] ${err && err.stack ? err.stack : err}`);
  process.exit(1);
});
