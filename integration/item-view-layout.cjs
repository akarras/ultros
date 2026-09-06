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
// asserts the *outcomes* from mobile through stacked -> split, so it stays valid whatever
// mechanism the layout uses next:
//
//  1. desktop tables fit their columns, while narrow-screen horizontal
//     scrolling reaches the actual cells at both ends,
//  2. "Show more", when present, sits inside the viewport and is hittable,
//  3. the sections stack or split according to the width their container
//     actually has,
//  4. the crafting-recipes grid spans its panel (no half-empty panel), when
//     the item has recipes.
//  5. active listings use the same reachable, outside-the-scrollport footer,
//  6. market summaries sit above their tables without datacenter exclusions
//     or a duplicate price card, and live status sits beside the item actions.
//
// REQUIRE_MARKET_DATA=1 turns missing rows/footer scenarios into failures;
// otherwise an empty dev DB explicitly reports those coverage gaps.
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD = process.env.WORLD || 'Gilgamesh';
// A craftable item so assertion 4 has a recipes panel to measure.
const ITEM_ID = process.env.ITEM_ID || '46010';
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
const SETTLE_MS = Number(process.env.SETTLE || 9000);
const REQUIRE_MARKET_DATA = process.env.REQUIRE_MARKET_DATA === '1';

// 1280 is the old xl breakpoint that triggered the bad split; 1735 is the
// width of the issue's screenshots (stacked after the fix, and inside the
// >=1660px band where the ad rail used to overflow); 2560 is the 1440p-class
// width where the split is supposed to engage. Mobile/tablet and intermediate
// desktop widths guard the gaps between the generic runner's breakpoints.
const WIDTHS = [390, 768, 1024, 1280, 1440, 1660, 1735, 1920, 2560];

// The split threshold the layout promises: two columns only when the tables'
// container is at least this wide (94rem at the 16px root font size).
const SPLIT_MIN_CONTAINER_PX = 94 * 16;

const ROUTE = `/item/${WORLD}/${ITEM_ID}`;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Runs in the page. Everything is located by ids and visible headings so the
 * probe survives class churn. */
function measure() {
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
      left: Math.round(r.left),
      right: Math.round(r.right),
      reachable: Boolean(hit) && (hit === showMore || showMore.contains(hit)),
      obscuredBy:
        hit && !(hit === showMore || showMore.contains(hit))
          ? hit.tagName.toLowerCase() + (hit.className ? '.' + String(hit.className).split(' ')[0] : '')
          : null,
      insideScrollport: Boolean(historyTableBox && historyTableBox.contains(showMore)),
    };
  }

  const listingsTableBox = [...listings.querySelectorAll('div')].find(
    (d) => d.querySelector('table') && getComputedStyle(d).overflowX !== 'visible',
  );
  const listingsShowMore = listings.querySelector('[data-testid="listings-show-more"]');
  let listingsShowMoreInfo = null;
  if (listingsShowMore) {
    listingsShowMore.scrollIntoView({ block: 'center' });
    const r = listingsShowMore.getBoundingClientRect();
    const hit = document.elementFromPoint((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    listingsShowMoreInfo = {
      left: Math.round(r.left),
      right: Math.round(r.right),
      reachable: Boolean(hit) &&
        (hit === listingsShowMore || listingsShowMore.contains(hit)),
      insideScrollport: Boolean(
        listingsTableBox && listingsTableBox.contains(listingsShowMore),
      ),
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
    documentSurplus: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    tableScrollSurplus: historyTableBox
      ? historyTableBox.scrollWidth - historyTableBox.clientWidth
      : null,
    hasTable: Boolean(table),
    showMore: showMoreInfo,
    listingsShowMore: listingsShowMoreInfo,
    gridW: Math.round(gridW),
    sideBySide,
    tableTopDifference: listings.querySelector('table') && table
      ? Math.abs(listings.querySelector('table').getBoundingClientRect().top - table.getBoundingClientRect().top)
      : null,
    recipes,
  };
}

// Exercise the browser's real scroll geometry. Merely checking scrollWidth
// misses a nested scrollport that clips row cells while the header still moves.
async function verifyHorizontalScroll(page, section, width, failures) {
  const result = await page.evaluate(async (id) => {
    const panel = document.getElementById(id);
    const table = panel?.querySelector('table');
    if (!table) {
      // Listings intentionally replaces an empty table with a status message.
      return { missing: panel?.querySelector('[role="status"]') ? 'rows' : 'table' };
    }
    const row = table.querySelector('tbody tr');
    if (!row) return { missing: 'rows' };
    let scrollport = table.parentElement;
    while (scrollport && scrollport !== panel &&
      !['auto', 'scroll'].includes(getComputedStyle(scrollport).overflowX)) {
      scrollport = scrollport.parentElement;
    }
    if (!scrollport || scrollport === panel) return { missing: 'scrollport' };
    const footer = [...panel.querySelectorAll('button')].find((button) =>
      /show more/i.test(button.textContent));
    const footerBounds = () => {
      if (!footer) return null;
      const rect = footer.getBoundingClientRect();
      return { left: rect.left, right: rect.right };
    };
    const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const endpoint = async (end) => {
      scrollport.scrollLeft = end ? scrollport.scrollWidth : 0;
      scrollport.scrollTop = 0;
      await frame();
      const cell = end ? row.lastElementChild : row.firstElementChild;
      // Scroll only the page's vertical axis. scrollIntoView would also move
      // any accidentally nested horizontal scrollport, hiding the regression.
      const cellRect = cell.getBoundingClientRect();
      window.scrollBy({ top: (cellRect.top + cellRect.bottom - innerHeight) / 2, left: 0, behavior: 'instant' });
      await frame();
      const rect = cell.getBoundingClientRect();
      const box = scrollport.getBoundingClientRect();
      const left = Math.max(rect.left, box.left, 0);
      const right = Math.min(rect.right, box.right, innerWidth);
      const top = Math.max(rect.top, box.top, 0);
      const bottom = Math.min(rect.bottom, box.bottom, innerHeight);
      const hit = right > left && bottom > top
        ? document.elementFromPoint((left + right) / 2, (top + bottom) / 2)
        : null;
      return {
        scrollLeft: scrollport.scrollLeft,
        reachable: Boolean(hit && (hit === cell || cell.contains(hit))),
        footer: footerBounds(),
      };
    };
    const start = await endpoint(false);
    const end = await endpoint(true);
    const surplus = scrollport.scrollWidth - scrollport.clientWidth;
    scrollport.scrollLeft = 0;
    return { start, end, surplus };
  }, section);
  if (result.missing) {
    const message = `${width}px: ${section} horizontal probe has no ${result.missing}`;
    if (result.missing !== 'rows' || REQUIRE_MARKET_DATA) failures.push(message);
    else console.log(`[skip] ${message}`);
    return;
  }
  if (result.surplus > 1 && width >= 1280) {
    failures.push(`${width}px: ${section} table overflows its desktop column by ${result.surplus}px`);
  }
  if (!result.start.reachable || !result.end.reachable) {
    failures.push(`${width}px: ${section} edge cells are clipped or obscured after horizontal scrolling`);
  }
  if (result.start.scrollLeft > 1 || Math.abs(result.end.scrollLeft - result.surplus) > 1) {
    failures.push(`${width}px: ${section} cannot reach both horizontal scroll endpoints`);
  }
  if (result.start.footer && result.end.footer &&
    (Math.abs(result.start.footer.left - result.end.footer.left) > 1 ||
      Math.abs(result.start.footer.right - result.end.footer.right) > 1)) {
    failures.push(`${width}px: ${section} Show more moves with horizontal table scrolling`);
  }
}

async function verifyExpansion(page, failures) {
  for (const section of ['listings', 'history']) {
    const panel = await page.$(`#${section}`);
    const buttons = await panel.$$('button');
    let footer;
    for (const button of buttons) {
      if (await button.evaluate((node) => /show more/i.test(node.textContent))) {
        footer = button;
        break;
      }
    }
    if (!footer) {
      const message = `${section} expansion probe needs more than 10 fixture rows`;
      if (REQUIRE_MARKET_DATA) failures.push(message);
      else console.log(`[skip] ${message}`);
      continue;
    }
    const before = await panel.$$eval('tbody tr', (rows) => rows.length);
    // The preceding scroll probes leave the page at an arbitrary position.
    // Center the footer clear of sticky navigation before clicking it.
    await footer.evaluate((button) => button.scrollIntoView({ block: 'center', behavior: 'instant' }));
    await page.waitForFunction((button) => {
      const rect = button.getBoundingClientRect();
      const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
      return hit && (hit === button || button.contains(hit));
    }, { timeout: TIMEOUT_MS }, footer);
    await footer.click();
    await page.waitForFunction((id, count) =>
      document.querySelectorAll(`#${id} tbody tr`).length > count,
    { timeout: TIMEOUT_MS }, section, before);
    const after = await panel.$$eval('tbody tr', (rows) => rows.length);
    console.log(`[info] ${section} Show more expanded ${before} -> ${after} rows`);
    await verifyHorizontalScroll(page, section, 390, failures);
  }
}

async function verifyMarketSummaries(page, failures) {
  const result = await page.evaluate(() => {
    const listings = document.getElementById('listings');
    const history = document.getElementById('history');
    const summary = document.querySelector('[data-testid="market-price-strip"]');
    const realPrice = listings?.querySelector('[data-testid="real-price-summary"]');
    const actions = document.querySelector('[data-testid="item-actions"]');
    return {
      hasExclusions: Boolean(listings?.querySelector('[data-testid="datacenter-exclusions"], [data-datacenter]')),
      hasListingsSummary: Boolean(listings?.querySelector('[data-testid="listings-summary"]')),
      hasSalesSummary: Boolean(history?.querySelector('[data-testid="sales-summary"]')),
      priceHasSalesDetails: /recent average|median|filtered|based on/i.test(realPrice?.textContent || ''),
      hasPriceStrip: Boolean(summary),
      hasRealPrice: Boolean(realPrice),
      hasHeaderLiveBadge: Boolean(actions?.querySelector('[data-testid="realtime-status-indicator"]')),
      liveBadgeCount: document.querySelectorAll('[data-testid="realtime-status-indicator"]').length,
    };
  });
  if (result.hasExclusions) failures.push('item page still exposes datacenter exclusions');
  if (!result.hasListingsSummary || !result.hasSalesSummary) {
    failures.push('missing table-level market summaries');
  }
  if (result.hasPriceStrip || !result.hasRealPrice || result.priceHasSalesDetails) {
    failures.push('Real Price must be in active listings without the standalone price strip or sale statistics');
  }
  if (!result.hasHeaderLiveBadge || result.liveBadgeCount !== 1) {
    failures.push('the live badge must appear once beside the item actions');
  }
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

      if (m.documentSurplus > 1) {
        failures.push(`${width}px: document overflows horizontally by ${m.documentSurplus}px`);
      }
      if (m.tableScrollSurplus > 1 && (width >= 1280 || m.sideBySide)) {
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
        if (m.showMore.left < 0 || m.showMore.right > width) {
          failures.push(`${width}px: "Show more" spans x=${m.showMore.left}..${m.showMore.right}, outside the viewport`);
        } else if (!m.showMore.reachable) {
          failures.push(
            `${width}px: "Show more" is scrolled into view but not hit-testable` +
              (m.showMore.obscuredBy ? ` — covered by <${m.showMore.obscuredBy}>` : ''),
          );
        }
      }

      if (m.listingsShowMore) {
        if (m.listingsShowMore.insideScrollport) {
          failures.push(
            `${width}px: listings "Show more" is inside the table scrollport`,
          );
        }
        if (m.listingsShowMore.left < 0 || m.listingsShowMore.right > width) {
          failures.push(
            `${width}px: listings "Show more" spans x=${m.listingsShowMore.left}..${m.listingsShowMore.right}, outside the viewport`,
          );
        } else if (!m.listingsShowMore.reachable) {
          failures.push(`${width}px: listings "Show more" is not hit-testable`);
        }
      }
      if (m.sideBySide && m.tableTopDifference !== null && m.tableTopDifference > 2) {
        failures.push(`${width}px: table headers differ in height by ${m.tableTopDifference}px`);
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
      await verifyHorizontalScroll(page, 'listings', width, failures);
      await verifyHorizontalScroll(page, 'history', width, failures);
    }

    await page.setViewport({ width: 390, height: 1100, deviceScaleFactor: 1 });
    await sleep(500);
    await verifyExpansion(page, failures);
    await verifyMarketSummaries(page, failures);
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
