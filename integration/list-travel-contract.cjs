"use strict";

// Reusable real-browser assertions for the #1478 market fixture matrix.
// Callers seed actual database offers and open an account/device list; this
// module neither intercepts pricing nor invents a fallback market fixture.
const assert = require("node:assert/strict");
const tid = value => `[data-testid="${value}"]`;

async function activeTrip(page) {
  return page.evaluate(() => ({
    total: document.querySelector('[data-testid="shop-totals"]')?.textContent,
    world: document.querySelector('[data-testid="shop-stop-title"]')?.textContent,
    offers: [...document.querySelectorAll('[data-shop-key]')].map(row => ({
      key: row.dataset.shopKey,
      remaining: row.querySelector('[data-testid="shop-stack-quantity"]')?.max,
    })),
  }));
}

async function assertFrontier(page, expected) {
  const actual = await page.$$eval('[data-testid="shop-route-option"]', cards => cards.map(card => ({
    cost: Number(card.dataset.routeCost),
    missing: Number(card.dataset.routeMissing),
    worlds: card.dataset.routeWorlds.split(",").filter(Boolean).map(Number).sort((a, b) => a - b),
    saving: card.querySelector('[data-testid="shop-route-saving"]')?.textContent.trim() || "",
  })));
  assert.equal(actual.length, expected.length, "every expected frontier step is rendered");
  expected.forEach((plan, index) => {
    assert.equal(actual[index].cost, plan.cost);
    assert.equal(actual[index].missing, plan.missing);
    assert.deepEqual(actual[index].worlds, [...plan.worlds].sort((a, b) => a - b));
    if (index && !plan.missing && !expected[index - 1].missing) {
      assert(actual[index].saving.includes(`${expected[index - 1].cost - plan.cost} gil`), "adjacent complete plans show their actual marginal saving");
    } else {
      assert.equal(actual[index].saving, "", "partial plans never claim savings");
    }
  });
}

async function reviewedLimitChange(page, { limit, expectedReview, expectedAdopted, beforeApply }) {
  const before = await activeTrip(page);
  assert(before.total && before.offers.length, "a real priced trip must already be active");
  await page.select(tid("list-travel-limit"), limit);
  await page.waitForFunction(() => {
    const notice = document.querySelector('[data-testid="shop-travel-drift"]');
    return notice && !notice.classList.contains("hidden");
  });
  assert.deepEqual(await activeTrip(page), before, "travel changes cannot rewrite an active trip");
  await page.click(tid("shop-refresh"));
  await page.waitForSelector(tid("shop-review"));
  assert.match(await page.$eval(tid("shop-review-next"), node => node.textContent), expectedReview);
  await page.click(tid("shop-review-keep"));
  await page.waitForSelector(tid("shop-review"), { hidden: true });
  assert.deepEqual(await activeTrip(page), before, "Keep preserves the same physical trip");
  await page.click(tid("shop-refresh"));
  await page.waitForSelector(tid("shop-review"));
  if (beforeApply) {
    // The caller performs an actual purchase/edit while the review is open.
    // First Apply must regenerate the proposal, never consume stale receipts.
    await beforeApply();
    const progressed = await activeTrip(page);
    await page.click(tid("shop-review-apply"));
    await page.waitForFunction(() => document.querySelector('[data-testid="shop-notice"]')?.textContent.includes("This proposal changed."));
    assert.deepEqual(await activeTrip(page), progressed, "a stale review cannot replace the progressed trip");
    assert(await page.$(tid("shop-review")), "updated proposal still awaits confirmation");
  }
  await page.click(tid("shop-review-apply"));
  await page.waitForSelector(tid("shop-review"), { hidden: true });
  assert.match(await page.$eval(tid("shop-totals"), node => node.textContent), expectedAdopted);
}

module.exports = { activeTrip, assertFrontier, reviewedLimitChange };
