// Regression probe for the item page's wide-viewport layout (issue #1234).
//
// Bugs shipped together when listings and sale history went side by side at
// xl (#1220), all invisible to the runner's 1280px desktop pass:
//
//  - Both tables force `min-w-[720px]`, so a half-width column gave each of
//    them a horizontal scrollport, and the sale history's "Show more" button
//    (then a <td> spanning the 720px table) scrolled and clipped with it.
//  - The ad rail's fixed-width slot overflowed the document by 16px at
//    >=1660px, giving every page a horizontal scrollbar.
//
// The fix sizes everything with container queries: the two-column split only
// happens when the tables' container is >= 94rem (1504px). This probe
// asserts the *outcomes* at three widths spanning stacked -> split, so it stays valid whatever
// mechanism the layout uses next:
//
//  1. the sale-history table never actually scrolls horizontally (its
//     min-width fits the column it was given),
//  2. "Show more", when present, sits inside the viewport and is hittable,
//  3. the sections stack or split according to the width their container
//     actually has,
//  4. the crafting-recipes grid spans its panel (no half-empty panel), when
//     the item has recipes.
//
// Document-level overflow (the ad rail bug) is the generic runner's job —
// its DEVICE=wide pass covers that for every route, not just this one.
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD = process.env.WORLD || 'Gilgamesh';
// A craftable item so assertion 5 has a recipes panel to measure.
const ITEM_ID = process.env.ITEM_ID || '46010';
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
const SETTLE_MS = Number(process.env.SETTLE || 9000);

// 1280 is the old xl breakpoint that triggered the bad split; 1735 is the
// width of the issue's screenshots (stacked after the fix, and inside the
// >=1660px band where the ad rail used to overflow); 2560 is the 1440p-class
// width where the split is supposed to engage.
const WIDTHS = [1280, 1735, 2560];

// The split threshold the layout promises: two columns only when the tables'
// container is at least this wide (94rem at the 16px root font size).
const SPLIT_MIN_CONTAINER_PX = 94 * 16;

const ROUTE = `/item/${WORLD}/${ITEM_ID}`;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Runs in the page. Everything is located by ids and visible headings so the
 * probe survives class churn. */
function measure() {
  const byHeading = (tag, text) =>
    [...document.querySelectorAll(tag)].find((h) => h.textContent.includes(text));

  const listings = document.getElementById('listings');
  const history = document.getElementById('history');
  if (!listings || !history) return { missing: '#listings / #history' };

  // (1) + (2): the sale-history table and its Show more button.
  const historyTableBox = [...history.querySelectorAll('div')].find(
    (d) => d.querySelector('table') && getComputedStyle(d).overflowX !== 'visible',
  );
  const table = historyTableBox && historyTableBox.querySelector('table');
  const showMore = [...history.querySelectorAll('button')].find((b) =>
    /show more/i.test(b.textContent),
  );
  let showMoreInfo = null;
  if (showMore) {
    // `elementFromPoint` takes *viewport* coordinates and returns null for
    // anything outside it. The item page is several thousand pixels tall, so
    // the button is always far below the fold on first paint and the hit test
    // would report "unreachable" for a perfectly reachable button. Scroll it
    // into view first, then hit-test where it actually is. The horizontal
    // measurement is unaffected by a vertical scroll.
    showMore.scrollIntoView({ block: 'center' });
    const r = showMore.getBoundingClientRect();
    const hit = document.elementFromPoint((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    showMoreInfo = {
      right: Math.round(r.right),
      reachable: Boolean(hit) && (hit === showMore || showMore.contains(hit)),
      obscuredBy:
        hit && !(hit === showMore || showMore.contains(hit))
          ? hit.tagName.toLowerCase() + (hit.className ? '.' + String(hit.className).split(' ')[0] : '')
          : null,
      insideScrollport: Boolean(historyTableBox && historyTableBox.contains(showMore)),
    };
  }

  // (3): stacked vs split, judged against the width the grid actually has.
  const grid = listings.parentElement;
  const gridW = grid.getBoundingClientRect().width;
  const lRect = listings.getBoundingClientRect();
  const hRect = history.getBoundingClientRect();
  const sideBySide = hRect.top < lRect.bottom && hRect.left > lRect.left;

  // (4): the recipes grid should span its panel. Skipped when the item has
  // no recipes (the panel is `hidden`).
  const recipesPanel = document.getElementById('crafting-recipes');
  let recipes = null;
  if (recipesPanel && recipesPanel.getBoundingClientRect().width > 0) {
    const panelStyle = getComputedStyle(recipesPanel);
    const panelContentW =
      recipesPanel.clientWidth -
      parseFloat(panelStyle.paddingLeft) -
      parseFloat(panelStyle.paddingRight);
    const recipeGrid = [...recipesPanel.querySelectorAll(':scope > div')].find(
      (d) => getComputedStyle(d).display === 'grid',
    );
    if (recipeGrid) {
      recipes = {
        panelContentW: Math.round(panelContentW),
        gridW: Math.round(recipeGrid.getBoundingClientRect().width),
      };
    }
  }

  return {
    viewport: document.documentElement.clientWidth,
    tableScrollSurplus: historyTableBox
      ? historyTableBox.scrollWidth - historyTableBox.clientWidth
      : null,
    hasTable: Boolean(table),
    showMore: showMoreInfo,
    gridW: Math.round(gridW),
    sideBySide,
    recipes,
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
    await page.setViewport({ width: WIDTHS[0], height: 1100, deviceScaleFactor: 1 });
    await page.goto(`${BASE_URL}${ROUTE}`, { waitUntil: 'domcontentloaded', timeout: TIMEOUT_MS });
    await sleep(SETTLE_MS);

    for (const width of WIDTHS) {
      await page.setViewport({ width, height: 1100, deviceScaleFactor: 1 });
      await sleep(500);
      const m = await page.evaluate(measure);
      if (m.missing) throw new Error(`${ROUTE} did not render ${m.missing}`);
      if (m.viewport !== width) {
        throw new Error(`asked for a ${width}px viewport but CSS resolved against ${m.viewport}px`);
      }
      if (!m.hasTable) throw new Error(`no sale-history table found at ${ROUTE}`);

      if (m.tableScrollSurplus > 0) {
        failures.push(
          `${width}px: sale-history table scrolls horizontally by ${m.tableScrollSurplus}px ` +
            `— the column it was given is narrower than the table's min-width`,
        );
      }

      if (m.showMore) {
        if (m.showMore.insideScrollport) {
          failures.push(
            `${width}px: "Show more" is inside the table's horizontal scrollport again ` +
              `— it will clip whenever the table is wider than the column`,
          );
        }
        if (m.showMore.right > width) {
          failures.push(`${width}px: "Show more" ends at x=${m.showMore.right}, past the viewport edge`);
        } else if (!m.showMore.reachable) {
          failures.push(
            `${width}px: "Show more" is scrolled into view but not hit-testable` +
              (m.showMore.obscuredBy ? ` — covered by <${m.showMore.obscuredBy}>` : ''),
          );
        }
      }


      const expectSplit = m.gridW >= SPLIT_MIN_CONTAINER_PX;
      if (m.sideBySide !== expectSplit) {
        failures.push(
          `${width}px: listings/history are ${m.sideBySide ? 'side by side' : 'stacked'} in a ` +
            `${m.gridW}px container — expected ${expectSplit ? 'side by side' : 'stacked'} ` +
            `(threshold ${SPLIT_MIN_CONTAINER_PX}px)`,
        );
      }

      if (m.recipes && m.recipes.gridW < m.recipes.panelContentW - 2) {
        failures.push(
          `${width}px: recipes grid spans ${m.recipes.gridW}px of a ${m.recipes.panelContentW}px ` +
            `panel — the panel is trailing empty space again`,
        );
      }

      console.log(
        `[info] ${width}px: ${m.sideBySide ? 'split' : 'stacked'} (grid ${m.gridW}px), ` +
          `table surplus ${m.tableScrollSurplus}px, ` +
          `show-more ${m.showMore ? 'present' : 'absent'}, recipes ${m.recipes ? 'measured' : 'absent'}`,
      );
    }
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error('[fail] item view layout regressions:');
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log('[done] item view lays out correctly at every guarded width');
}

main().catch((err) => {
  console.error(`[fail] ${err && err.stack ? err.stack : err}`);
  process.exit(1);
});
