"use strict";
// Called by the real-priced acceptance runner after #1480 integration.
// All successful price reads come from its isolated DB fixture, never mocks.
const assert = require("node:assert/strict");
const { assertFrontier, activeTrip, reviewedLimitChange } = require("./list-travel-contract.cjs");
const { finishCleanup } = require("./list-fixture-cleanup.cjs");
const tid = id => `[data-testid="${id}"]`;

async function runTravelMatrix({ page, base, market, createList, load, click, replace, summary, stock, checked, record, refreshOwner }) {
  const devices = [];
  let originalError;
  const [low, home, foreign] = market.manifest.worlds;
  const homeDc = market.region.datacenters.find(dc => dc.id === home.datacenter_id);
  const foreignDc = market.region.datacenters.find(dc => dc.id === foreign.datacenter_id);
  assert(home.id > low.id && homeDc && foreignDc && homeDc.id !== foreignDc.id);
  const [itemA, itemB] = market.manifest.item_ids;
  const all = [
    { cost: 200, missing: 0, worlds: [home.id] },
    { cost: 160, missing: 0, worlds: [home.id, low.id] },
    { cost: 120, missing: 0, worlds: [home.id, foreign.id] },
    { cost: 80, missing: 0, worlds: [low.id, foreign.id] },
  ];
  const sameDc = all.slice(0, 2);
  async function fixture(scenario) { await refreshOwner(); await market.set(scenario); }
  async function pickScope(name) {
    await replace(`${tid("device-price-controls")} input[role="combobox"]`, name);
    await page.waitForFunction(name => [...document.querySelectorAll('button[role="option"]')].some(node => node.textContent.trim().endsWith(name)), {}, name);
    await page.evaluate(name => [...document.querySelectorAll('button[role="option"]')].find(node => node.textContent.trim().endsWith(name)).click(), name);
    await page.waitForFunction(() => document.querySelector('[data-testid="device-list-status"]')?.textContent.includes("Saved on this device"));
  }
  async function create(surface, scope = { Region: market.region.id }, scopeName = market.region.name) {
    await refreshOwner();
    if (surface === "account") {
      const id = await createList([[null, 1, itemA], [null, 1, itemB]], scope);
      if (scope.Region) await stock(id);
      else {
        const response = await checked("GET", `/api/v1/list/${id}/listings`);
        const allowed = scope.World ? [scope.World] : homeDc.worlds.map(world => world.id);
        for (const [row, offers] of response[1]) {
          const actual = offers.map(offer => offer.id).sort((a, b) => a - b);
          const expected = market.manifest.listings.filter(offer => offer.item_id === row.item_id && allowed.includes(offer.world_id)).map(offer => offer.id).sort((a, b) => a - b);
          assert.deepEqual(actual, expected, "narrow fetched scope contains only real manifest supply");
        }
      }
      return { surface, path: `/list/${id}` };
    }
    await load("/list"); await click(tid("list-new"));
    await replace(tid("device-list-name"), `Travel ${market.token} ${devices.length}`);
    await click(tid("device-list-create"));
    await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    const path = new URL(page.url()).pathname; devices.push(path);
    for (const name of market.manifest.item_names) {
      await replace('input[aria-label="Quantity to add"]', 1);
      await replace('input[aria-label="Add an item"]', name);
      await page.waitForSelector(`button[aria-label="Add ${name}"]`);
      await page.keyboard.press("Enter"); await page.keyboard.press("Escape");
      await page.waitForFunction(name => document.querySelector(`input[aria-label="Needed for ${name}"]`)?.value === "1", {}, name);
    }
    await pickScope(scopeName);
    return { surface, path };
  }
  async function open(cart, params = {}) {
    await refreshOwner();
    await load(`${cart.path}?${new URLSearchParams({ labs: "lists-sync", ...params })}`);
    await click(tid("guest-build-mode"));
    if (cart.surface === "device") await click(tid("device-prices-refresh"));
  }
  async function start(cart, { params = {}, total, incomplete = false, plans, tripCost = total, missing = 0 }) {
    const displayed = total === null ? "—" : `${total} gil`;
    await open(cart, params); await summary(displayed, incomplete);
    await click(tid("guest-shop-mode")); await click((tid("shop-route-option") + '[data-route-cheapest="true"]'));
    await page.waitForSelector(tid("shop-totals"));
    assert.match(await page.$eval(tid("shop-totals"), node => node.textContent), new RegExp(`${tripCost} gil.*${missing} missing`));
    await page.$eval(tid("shop-estimate"), node => { node.open = true; });
    assert((await page.$eval(tid("shop-build-reference"), node => node.textContent)).includes(`Build estimate: ${displayed}`));
    assert.equal(await page.$eval(tid("shop-build-reference"), node => node.dataset.incomplete), String(incomplete));
    await assertFrontier(page, plans);
  }
  const offer = (item, world) => {
    const matches = market.manifest.listings.filter(row => row.item_id === item && row.world_id === world);
    assert.equal(matches.length, 1, "exact physical fixture offer"); return matches[0];
  };
  async function atOffer(expected) {
    await page.waitForSelector(`[data-shop-key="${expected.id}"]`);
    assert.deepEqual(await page.$$eval('[data-shop-key]', rows => rows.map(row => Number(row.dataset.shopKey))), [expected.id]);
    const world = market.manifest.worlds.find(world => world.id === expected.world_id);
    assert((await page.$eval(tid("shop-stop-title"), node => node.textContent)).includes(world.name));
  }
  async function buy(expected) {
    await atOffer(expected);
    const row = `[data-shop-key="${expected.id}"]`;
    await replace(`${row} ${tid("shop-stack-quantity")}`, 1);
    await click(`${row} ${tid("shop-stack-bought")}`);
    await page.waitForFunction(id => document.querySelector(`[data-shop-key="${id}"] input[data-testid="shop-stack-quantity"]`)?.max === "0", {}, expected.id);
  }
  try {
    await page.setViewport({ width: 1280, height: 900 });
    await page.setCookie({ name: "HOME_WORLD", value: home.name, url: base, path: "/" });
    await fixture("travel_ladder");
    for (const surface of ["account", "device"]) {
      const cart = await create(surface);
      await start(cart, { total: 80, plans: all });
      await record(`${surface}-travel-complete-four-step-frontier`);
      await page.setViewport({ width: 390, height: 844 });
      await record(`${surface}-travel-mobile-four-step-frontier`);
      await page.setViewport({ width: 1280, height: 900 });
      // Every frontier identity is actionable, not only the three shortcuts.
      await click(`${tid("shop-route-option")}[data-route-cost="120"]`);
      await page.waitForSelector(tid("shop-review"));
      assert.match(await page.$eval(tid("shop-review-next"), node => node.textContent), /120 gil/);
      await click(tid("shop-review-apply"));
      await page.waitForSelector(tid("shop-review"), { hidden: true });
      await atOffer(offer(itemA, home.id));
      assert.match((await activeTrip(page)).total, /120 gil/);
      await record(`${surface}-travel-select-middle-frontier`);
      await start(cart, { params: { travel: "dc" }, total: 160, plans: sameDc });
      await record(`${surface}-travel-home-dc-limit`);
      await start(cart, { params: { travel: "world" }, total: 200, plans: [all[0]] });
      await record(`${surface}-travel-home-world-limit`);
      if (surface === "device") {
        // Account world/DC exclusion allocation already has exhaustive #1478
        // coverage; device parity plus travel cards is the missing seam here.
        await start(cart, { params: { "excluded-worlds": String(low.id) }, total: 120, plans: [all[0], all[2]] });
        await record("device-travel-world-exclusion-frontier");
        await start(cart, { params: { "excluded-datacenters": foreignDc.name }, total: 160, plans: sameDc });
        await record("device-travel-dc-exclusion-frontier");
      }
      for (const [label, scope, name, total, plans] of [
        ["world", { World: home.id }, home.name, 200, [all[0]]],
        ["dc", { Datacenter: homeDc.id }, homeDc.name, 160, sameDc],
      ]) {
        const narrow = await create(surface, scope, name);
        await start(narrow, { total, plans });
        // Scope travel may not broaden an already narrower fetched price set.
        assert.equal(await page.$eval(tid("list-travel-limit"), node => node.value), "scope");
        await record(`${surface}-travel-respects-fetched-${label}-scope`);
      }
      const outsideHome = await create(surface, { World: low.id }, low.name);
      await start(outsideHome, {
        params: { travel: "world" }, total: null, incomplete: true, tripCost: 0, missing: 2,
        plans: [{ cost: 0, missing: 2, worlds: [] }],
      });
      assert.equal((await page.$$('[data-shop-key]')).length, 0, "a home outside fetched scope cannot invent supply or a stop");
      await record(`${surface}-travel-home-outside-fetched-scope`);
      // A home stop with a HIGHER numeric ID must precede a genuinely purchased
      // lower-ID stop. Complete both stops before asserting final Next state.
      await start(cart, { params: { travel: "dc" }, total: 160, plans: sameDc });
      const first = offer(itemB, home.id), second = offer(itemA, low.id);
      await atOffer(first); const before = await activeTrip(page);
      await buy(first);
      assert.equal((await activeTrip(page)).world, before.world, "buying cannot reorder the active stop");
      await click(tid("shop-next-world")); await atOffer(second); await buy(second);
      assert(await page.$eval(tid("shop-next-world"), node => node.disabled), "both exact physical stops are complete");
      assert.match((await activeTrip(page)).total, /160 gil/, "purchases do not reprice the frozen trip");
      await click(tid("guest-build-mode")); await summary("0 gil", false);
      for (const item of market.manifest.item_names) {
        await click(`button[aria-label="Details for ${item}"]`);
        await page.waitForFunction(item => document.querySelector(`input[aria-label="Owned for ${item}"]`)?.value === "1", {}, item);
      }
      await record(`${surface}-travel-higher-id-home-two-stop-purchases`);

      const reviewed = await create(surface);
      await start(reviewed, { params: { travel: "dc" }, total: 160, plans: sameDc });
      await reviewedLimitChange(page, {
        limit: "world", expectedReview: /200 gil/, expectedAdopted: /100 gil/,
        beforeApply: async () => { await buy(offer(itemB, home.id)); },
      });
      await atOffer(offer(itemA, home.id));
      assert.equal(await page.$(`[data-shop-key="${offer(itemB, home.id).id}"]`), null, "reviewed receipts cannot rebuy the recorded home stack");
      await click(tid("guest-build-mode")); await summary("100 gil", false);
      await record(`${surface}-travel-keep-apply-stale-purchase-review`);
      if (surface === "device") {
        await page.setViewport({ width: 390, height: 844 });
        await click(tid("guest-shop-mode")); await record("device-travel-mobile-reviewed-route");
        await page.setViewport({ width: 1280, height: 900 });
      }
    }
    await fixture("travel_partial");
    for (const surface of ["account", "device"]) {
      const cart = await create(surface);
      await start(cart, {
        params: { travel: "dc" }, total: 60, incomplete: true, missing: 1,
        plans: [{ cost: 100, missing: 1, worlds: [home.id] }, { cost: 60, missing: 1, worlds: [low.id] }],
      });
      assert.match(await page.$eval(tid("shop-build-coverage"), node => node.textContent), /Known subtotal only.*Unpriced units: 1.*Fully priced items: 1\/2/, "#1439's exact partial-supply counts stay visible in Shop");
      assert(await page.$$eval('[data-testid="shop-route-saving"]', nodes => nodes.every(node => !node.textContent.trim())), "equally incomplete cheaper routes cannot claim marginal savings");
      await atOffer(offer(itemA, low.id));
      await record(`${surface}-travel-no-feasible-home-dc-route`);
    }
  } catch (error) { originalError = error; throw error; }
  finally {
    await finishCleanup([
      async () => {
        await refreshOwner();
        const failures = [];
        for (const device of devices) {
          try {
            await load(device); await click(tid("device-list-storage-toggle"));
            await click(tid("device-list-delete")); await click(tid("device-list-confirm-delete"));
            await page.waitForFunction(() => location.pathname === "/list");
          } catch (error) { failures.push(error); }
        }
        if (failures.length) throw new AggregateError(failures, "Travel device cleanup failed");
      },
      async () => {
        await fixture("baseline");
        await page.setCookie({ name: "HOME_WORLD", value: low.name, url: base, path: "/" });
        await page.setViewport({ width: 1280, height: 900 });
      },
    ], originalError);
  }
}
module.exports = { runTravelMatrix };
