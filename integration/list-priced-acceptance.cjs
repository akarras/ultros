"use strict";
// Server-backed pricing/exclusion matrix. Missing fixture data is a failure,
// never a successful no-data path. Permission/focus suites run separately.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { createMarketFixture } = require("./list-market-fixture.cjs");
const { cleanupOwnedLists, finishCleanup } = require("./list-fixture-cleanup.cjs");
const base = new URL(process.env.BASE_URL || "http://127.0.0.1:8080").origin;
const OWNER = 990000001478;
const READER = 990000001479;
const tid = id => `[data-testid="${id}"]`;

async function main() {
  const browser = await require("puppeteer").launch({ headless: true });
  let page;
  const errors = [];
  const failedRequests = [];
  const failedResponses = [];
  let lastNavigation;
  const artifacts = path.join(__dirname, "artifacts", "list-priced-acceptance");
  let artifactsReady = false;
  const results = [];
  const listIds = [];
  let market;
  let deviceUrl;
  let originalError;
  const startedAt = Date.now();
  let ownerSetupAt = 0;
  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, { method, headers: { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await response.text();
    let parsed; try { parsed = text ? JSON.parse(text) : null; } catch { parsed = text; }
    return { status: response.status, body: parsed };
  }, { method, route, body });
  async function checked(method, route, body) {
    const result = await api(method, route, body);
    assert.equal(result.status, 200, `${method} ${route}: ${JSON.stringify(result.body)}`);
    return result.body;
  }
  async function load(route) {
    await page.bringToFront();
    const response = await page.goto(`${base}${route}`, { waitUntil: "domcontentloaded" });
    lastNavigation = { requested: route, url: response.url(), status: response.status() };
    assert(response.ok(), `load ${route}: ${response.status()}`);
    await page.waitForFunction(() => window.__pricedHydrated);
  }
  async function login(user) {
    const setupStartedAt = Date.now();
    await load(`/test/login?user_id=${user}&username=PricedAcceptance${user}&redirect=/list`);
    if (user === OWNER) ownerSetupAt = setupStartedAt;
  }
  // Explicit setup only between independent scenarios. Never run while checking
  // revocation/deletion/sign-out, and never change the production cache TTL.
  async function refreshOwner() {
    await page.bringToFront();
    const setupStartedAt = Date.now();
    const response = await api("GET", `/test/login?user_id=${OWNER}&username=PricedAcceptance${OWNER}&redirect=/list`);
    assert.equal(response.status, 200, "refresh scenario owner's test-auth setup");
    ownerSetupAt = setupStartedAt;
    console.log(`[setup] owner test-auth refreshed at elapsed ${Date.now() - startedAt}ms`);
  }
  async function click(selector) {
    await page.waitForSelector(selector, { visible: true });
    await page.$eval(selector, element => element.scrollIntoView({ behavior: "instant", block: "center" }));
    await page.click(selector);
  }
  async function replace(selector, value) {
    await click(selector);
    await page.$eval(selector, input => input.select());
    await page.keyboard.press("Backspace"); await page.type(selector, String(value));
  }
  async function summary(total, incomplete) {
    await page.waitForFunction(({ total, incomplete }) => {
      const node = document.querySelector('[data-testid="list-estimate-total"]');
      return node?.textContent === total && node.dataset.incomplete === String(incomplete);
    }, {}, { total, incomplete });
  }
  async function checkBuildReference(returnBuild = true) {
    const build = await page.evaluate(() => ({
      total: document.querySelector('[data-testid="list-estimate-total"]').textContent,
      incomplete: document.querySelector('[data-testid="list-estimate-total"]').dataset.incomplete,
      coverage: document.querySelector('[data-testid="list-estimate-status"]').textContent,
      refreshFailed: /refresh failed/i.test(document.querySelector('[data-testid="list-estimate-freshness"]')?.textContent || ""),
    }));
    await click(tid("guest-shop-mode")); await click(tid("shop-cheapest"));
    // With a trip already active, a quick pick is reviewed before adoption
    // (#1480); the reference must describe the adopted source.
    if (await page.$(tid("shop-review-apply"))) await click(tid("shop-review-apply"));
    await page.waitForSelector(tid("shop-build-reference"));
    await page.$eval(tid("shop-estimate"), details => { details.open = true; });
    assert((await page.$eval(tid("shop-build-reference"), node => node.textContent)).includes(`Build estimate: ${build.total} (`), "Shop reference exactly matches actual Build total");
    assert.equal(await page.$eval(tid("shop-build-reference"), node => node.dataset.incomplete), build.incomplete);
    assert.equal(await page.$eval(tid("shop-build-coverage"), node => node.textContent), build.coverage, "Shop preserves actual Build partial/acquired/no-price coverage");
    assert.equal(await page.$eval(tid("shop-build-reference"), node => node.dataset.refreshFailed), String(build.refreshFailed), "Shop preserves the source's failed refresh marker");
    if (returnBuild) await click(tid("guest-build-mode"));
  }
  async function record(name, evidencePage = page) {
    assert(await evidencePage.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `${name}: no horizontal overflow`);
    const screenshot = `${name}.png`;
    await evidencePage.screenshot({ path: path.join(artifacts, screenshot), fullPage: true });
    results.push({ name, url: evidencePage.url(), screenshot, viewport: evidencePage.viewport(), elapsedMs: Date.now() - startedAt, ownerSetupAgeMs: Date.now() - ownerSetupAt, status: "passed" });
    console.log(`[PASS] ${name}`);
  }
  async function createList(rows) {
    await refreshOwner();
    const name = `Priced acceptance ${market.token} ${listIds.length}`;
    await checked("POST", "/api/v1/list/create", { name, wdr_filter: { Region: market.region.id } });
    const id = (await checked("GET", "/api/v1/list")).find(entry => entry.list.name === name)?.list.id;
    assert(id, "created account list must exist"); listIds.push(id);
    for (const [hq, quantity, item_id = 5056, acquired = 0] of rows) {
      await checked("POST", `/api/v1/list/${id}/add/item`, { id: 0, list_id: id, item_id, hq, quantity, acquired });
    }
    return id;
  }
  async function stock(id) {
    const rows = (await checked("GET", `/api/v1/list/${id}/listings`))[1];
    for (const [row, offers] of rows) {
      market.assertStock(offers, row.item_id);
    }
  }
  try {
    page = await browser.newPage();
    page.setDefaultTimeout(Number(process.env.TIMEOUT_MS || 90000));
    await page.setCacheEnabled(false);
    await page.evaluateOnNewDocument(() => {
      window.__pricedHydrated = false;
      addEventListener("ultros:hydrated", () => { window.__pricedHydrated = true; });
    });
    page.on("pageerror", error => errors.push(String(error.stack || error)));
    page.on("requestfailed", request => {
      if (failedRequests.length < 100) failedRequests.push({ url: request.url(), error: request.failure()?.errorText });
    });
    page.on("response", response => {
      if (response.status() >= 400 && failedResponses.length < 100)
        failedResponses.push({ url: response.url(), status: response.status() });
    });
    page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
    fs.mkdirSync(artifacts, { recursive: true });
    artifactsReady = true;
    await page.setCookie(...[["LABS", "lists-sync"], ["HIDE_ADS", "true"], ["i18n_pref_locale", "en"]]
      .map(([name, value]) => ({ name, value, url: base, path: "/" })));
    await page.setViewport({ width: 1280, height: 900 });
    await login(READER); await login(OWNER);
    market = await createMarketFixture(api, await checked("GET", "/api/v1/world_data"));
    const itemName = market.manifest.item_names[0];
    assert(itemName, "fixture catalog item name");
    await page.setCookie({ name: "HOME_WORLD", value: market.manifest.worlds[0].name, url: base, path: "/" });
    await require("./list-priced-cart-journey.cjs").runPricedCartJourney({ page, market, createList, load, click, replace, summary, stock, checkBuildReference, checked, record, refreshOwner });
    const id = await createList([[null, 4], [true, 2], [false, 2]]);
    await stock(id);
    for (const [label, width, height] of [["desktop", 1280, 900], ["mobile", 390, 844]]) {
      await page.setViewport({ width, height });
      await refreshOwner();
      await load(`/list/${id}`);
      await summary("121 gil", false);
      await page.waitForFunction(() => {
        const expected = { any: 61, hq: 40, nq: 20 };
        return [...document.querySelectorAll('[data-testid="cart-rows"] > li')].every(row =>
          row.querySelector('[data-testid="cart-line-estimate"]')?.textContent.includes(`${expected[row.querySelector("select")?.value]} gil`));
      });
      assert.equal(await page.$$eval(`${tid("cart-rows")} > li`, rows => rows.length), 3);
      await checkBuildReference();
      await record(`account-${label}-direct-overlapping-quality`);
      await load("/list");
      const marker = `spa-${Date.now()}`;
      await page.evaluate(marker => { window.__pricedDocument = marker; }, marker);
      await click(`a[href="/list/${id}"]`);
      await summary("121 gil", false);
      assert.equal(await page.evaluate(() => window.__pricedDocument), marker, "directory entry must use client navigation");
      await record(`account-${label}-client-overlapping-quality`);
    }
    const first = market.manifest.worlds[0];
    const otherDc = market.region.datacenters.find(dc => dc.id === market.manifest.worlds[2].datacenter_id);
    const laterHome = market.manifest.worlds[1];
    assert(first.id < laterHome.id, "exclusion matrix must include a home world that is not the lowest fixture world ID");
    await page.setCookie({ name: "HOME_WORLD", value: laterHome.name, url: base, path: "/" });
    for (const [label, query, total, tripMissing, forbidden] of [
      ["world", `excluded-worlds=${first.id}`, "136 gil", 4, [first.name]],
      ["datacenter", `excluded-datacenters=${encodeURIComponent(otherDc.name)}`, "96 gil", 1, otherDc.worlds.map(world => world.name)],
    ]) {
      // Recording each stack is necessary to unlock Next world. Use a fresh
      // list per case so exhaustive traversal cannot change the next case.
      const exclusionId = await createList([[null, 4], [true, 2], [false, 2]]);
      await stock(exclusionId);
      await load(`/list/${exclusionId}?${query}`);
      await summary(total, true);
      await record(`build-excludes-${label}`);
      await checkBuildReference(false);
      await page.waitForSelector(tid("shop-totals"));
      await page.$eval(tid("shop-estimate"), details => { details.open = true; });
      assert.match(await page.$eval(tid("shop-estimate"), node => node.textContent), new RegExp(`Build estimate: ${total}`));
      assert.match(await page.$eval(tid("shop-totals"), node => node.textContent), new RegExp(`${total}.*${tripMissing} missing`));
      const expectedWorldIds = market.manifest.worlds.filter(world => !forbidden.includes(world.name)).map(world => world.id).sort((a, b) => a - b);
      assert(expectedWorldIds.length >= 2, "exclusion fixture must require multiple planned stops");
      const stops = [];
      for (let index = 0; index < 10; index += 1) {
        const heading = await page.$eval(tid("shop-stop-title"), node => node.textContent);
        const keys = await page.$$eval('[data-shop-key]', rows => rows.map(row => Number(row.dataset.shopKey)));
        assert(keys.length > 0, "each planned stop must contain fixture stacks");
        const offers = keys.map(key => {
          const offer = market.manifest.listings.find(offer => offer.id === key);
          assert(offer, `planned offer ${key} must exist in real fixture manifest`);
          return offer;
        });
        const worldIds = [...new Set(offers.map(offer => offer.world_id))];
        assert.equal(worldIds.length, 1, "current stop contains offers from exactly one world");
        const [worldId] = worldIds;
        assert(!stops.includes(worldId), "Shop itinerary must progress without cycling");
        stops.push(worldId);
        assert(expectedWorldIds.includes(worldId), `every Shop offer must respect excluded ${label}: ${worldId}`);
        assert(!forbidden.some(world => heading.includes(world)), `every Shop stop must respect excluded ${label}: ${heading}`);
        // Next is disabled both at the final stop AND while this stop has
        // unfinished stacks. Complete all current purchases before deciding.
        for (let purchase = 0; purchase < 20; purchase += 1) {
          if (await page.$$eval('[data-shop-key] input[data-testid="shop-stack-quantity"]', inputs => inputs.length > 0 && inputs.every(input => Number(input.max) === 0))) break;
          const next = await page.$$eval('[data-shop-key]', rows => {
            const row = rows.find(row => !row.querySelector('[data-testid="shop-stack-bought"]').disabled);
            return row ? { key: row.dataset.shopKey, remaining: Number(row.querySelector('input[data-testid="shop-stack-quantity"]').max) } : null;
          });
          assert(next && next.remaining > 0, "unfinished current stop must offer a valid purchase action");
          const row = `[data-shop-key="${next.key}"]`;
          await replace(`${row} ${tid("shop-stack-quantity")}`, next.remaining);
          await click(`${row} ${tid("shop-stack-bought")}`);
          await page.waitForFunction(({ key, before }) => Number(document.querySelector(`[data-shop-key="${key}"] input[data-testid="shop-stack-quantity"]`)?.max) < before, {}, { key: next.key, before: next.remaining });
        }
        assert(await page.$$eval('[data-shop-key] input[data-testid="shop-stack-quantity"]', inputs => inputs.length > 0 && inputs.every(input => Number(input.max) === 0)), "all current-stop stacks must be recorded before checking final-stop status");
        if (await page.$eval(tid("shop-next-world"), button => button.disabled)) break;
        await click(tid("shop-next-world"));
        await page.waitForFunction(heading => document.querySelector('[data-testid="shop-stop-title"]')?.textContent !== heading, {}, heading);
      }
      assert(await page.$eval(tid("shop-next-world"), button => button.disabled), "all planned stops were checked through the final world");
      assert.deepEqual([...stops].sort((a, b) => a - b), expectedWorldIds, "exhaustively visited every expected fixture world and no excluded world");
      assert.match(await page.$eval(tid("shop-totals"), node => node.textContent), new RegExp(`${total}.*${tripMissing} missing`));
      await record(`shop-excludes-${label}`);
    }
    await page.setCookie({ name: "HOME_WORLD", value: first.name, url: base, path: "/" });
    await checked("POST", `/api/v1/list/${id}/share/user`, { user_id: READER, permission: "Read" });
    await login(READER);
    await checked("DELETE", `/test/list/market-fixture/${market.token}`);
    await stock(id); // A different account's cleanup cannot remove the fixture.
    await load(`/list/${id}`); await summary("121 gil", false);
    assert.equal((await checked("GET", `/api/v1/list/${id}/listings`))[0].permission, "Read");
    assert.equal(await page.$(tid("inline-list-add")), null, "reader cannot add items");
    await click(tid("guest-shop-mode")); await click(tid("shop-cheapest"));
    await page.waitForSelector(tid("shop-stack-bought"));
    assert(await page.$$eval(tid("shop-stack-bought"), buttons => buttons.length > 0 && buttons.every(button => button.disabled)), "reader cannot record purchases");
    await record("shared-reader-mobile-priced-shop");
    await login(OWNER);
    await require("./list-real-relay-recovery.cjs").runRealRelayRecovery({ browser, base, market, ownerId: OWNER, createList, ownerApi: checked, record });
    await require("./list-companion-lifecycle.cjs").runCompanionLifecycle({ base, userId: READER, createList, ownerApi: checked, record });
    const availabilityList = await createList([[false, 5], [true, 6]]);
    await page.bringToFront();
    await require("./list-shop-availability.cjs").runShopAvailability(page, {
      url: `${base}/list/${availabilityList}`,
      listId: availabilityList,
      readProjection: async () => {
        const response = await checked("GET", `/api/v1/list/${availabilityList}/listings`);
        return [response[0], response[1].map(([row]) => row)];
      },
    });
    await record("account-companion-incompatible-document");
    // Reuse #1474's focus contract against actual server-seeded offers. Its
    // standalone regression fixture is intentionally not acceptance evidence.
    const focusList = await createList([[false, 5], [true, 6]]);
    await stock(focusList);
    const remote = await browser.newPage();
    try {
      remote.setDefaultTimeout(Number(process.env.TIMEOUT_MS || 90000));
      await remote.evaluateOnNewDocument(() => {
        window.__pricedHydrated = false;
        addEventListener("ultros:hydrated", () => { window.__pricedHydrated = true; });
      });
      await remote.bringToFront();
      await remote.goto(`${base}/list/${focusList}`, { waitUntil: "domcontentloaded" });
      await remote.waitForFunction(() => window.__pricedHydrated);
      await load(`/list/${focusList}`);
      await click(tid("guest-shop-mode")); await click(tid("shop-cheapest"));
      await page.waitForSelector('[data-shop-key]');
      await require("./list-shop-focus.cjs").runShopFocus(page, { label: "real-market-account", remotePurchase: async (key, delta) => {
        await remote.bringToFront();
        const offer = market.manifest.listings.find(offer => String(offer.id) === key);
        assert(offer, `planned key ${key} must refer to an actual fixture listing`);
        const quality = offer.hq ? "hq" : "nq";
        const index = await remote.$$eval(`${tid("cart-rows")} > li`, (rows, quality) => rows.findIndex(row => row.querySelector("select")?.value === quality), quality);
        assert(index >= 0, `remote ${quality} row`);
        const row = `${tid("cart-rows")} > li:nth-child(${index + 1})`;
        const details = `${row} button[aria-label="Details for ${itemName}"]`;
        if (await remote.$eval(details, node => node.getAttribute("aria-expanded")) !== "true") await remote.click(details);
        const owned = `${row} input[aria-label="Owned for ${itemName}"]`;
        const before = Number(await remote.$eval(owned, node => node.value));
        await remote.focus(owned); await remote.$eval(owned, node => node.select());
        await remote.keyboard.press("Backspace"); await remote.type(owned, String(before + delta));
        await remote.keyboard.press("Enter"); await page.bringToFront();
      } });
      await record("account-real-market-shop-focus");
    } finally { await remote.close(); }
    // Device prices must come through the same real database rows. The Build
    // price toolbar is supplied by #1471, a required dependency of this gate.
    await load("/list"); await click(tid("list-new"));
    await replace(tid("device-list-name"), `Priced device ${market.token}`);
    // Signed in: the modal defaults to Online; this gate transitions the list itself later.
    await click(tid("device-list-storage-local"));
    await page.waitForFunction(sel => document.querySelector(sel)?.getAttribute("aria-pressed") === "true", {}, tid("device-list-storage-local"));
    await click(tid("device-list-create"));
    await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    deviceUrl = new URL(page.url()).pathname;
    await replace('input[aria-label="Quantity to add"]', 5);
    await replace('input[aria-label="Add an item"]', itemName);
    await page.waitForSelector(`button[aria-label="Add ${itemName}"]`);
    await page.keyboard.press("Enter"); await page.keyboard.press("Escape");
    await page.waitForSelector(tid("device-price-controls"));
    await summary("—", false); await checkBuildReference();
    await record("device-prices-not-requested-reference");
    await replace(`${tid("device-price-controls")} input[role="combobox"]`, market.region.name);
    await page.waitForFunction(name => [...document.querySelectorAll('button[role="option"]')]
      .some(button => button.textContent.trim().endsWith(name)), {}, market.region.name);
    await page.evaluate(name => [...document.querySelectorAll('button[role="option"]')]
      .find(button => button.textContent.trim().endsWith(name)).click(), market.region.name);
    // Fault only the transport; every successful response still comes from
    // the real server fixture, with no injected price response or DOM state.
    let lookupMode = "hold";
    let heldLookup;
    const intercept = async request => {
      if (new URL(request.url()).pathname.startsWith("/api/v1/bulkListings/")) {
        if (lookupMode === "hold") { heldLookup = request; return; }
        if (lookupMode === "abort") { await request.abort("failed"); return; }
      }
      await request.continue();
    };
    await page.setRequestInterception(true); page.on("request", intercept);
    try {
      await click(tid("device-prices-refresh"));
      await page.waitForFunction(() => /Loading prices/.test(document.querySelector('[data-testid="list-estimate-status"]')?.textContent));
      assert(heldLookup, "the actual market request must be held while testing loading");
      await checkBuildReference(); await record("device-prices-loading-reference");
      lookupMode = "abort"; await heldLookup.abort("failed"); heldLookup = null;
      await page.waitForFunction(() => /Prices unavailable/.test(document.querySelector('[data-testid="list-estimate-status"]')?.textContent));
      await checkBuildReference(); await record("device-prices-initial-failure-reference");
      lookupMode = "pass"; await click(tid("device-prices-refresh")); await summary("56 gil", false);
      lookupMode = "abort"; await click(tid("device-prices-refresh"));
      await page.waitForFunction(() => /refresh failed/i.test(document.querySelector('[data-testid="list-estimate-freshness"]')?.textContent));
      await summary("56 gil", false); await checkBuildReference();
      await record("device-cached-prices-failed-refresh-reference");
      lookupMode = "pass"; await click(tid("device-prices-refresh")); await summary("56 gil", false);
      await page.waitForFunction(() => !/refresh failed/i.test(document.querySelector('[data-testid="list-estimate-freshness"]')?.textContent));
    } finally {
      if (heldLookup) await heldLookup.abort("failed").catch(() => {});
      page.off("request", intercept); await page.setRequestInterception(false);
    }
    const newItemName = market.manifest.item_names[1];
    await replace('input[aria-label="Quantity to add"]', 1);
    await replace('input[aria-label="Add an item"]', newItemName);
    await page.waitForSelector(`button[aria-label="Add ${newItemName}"]`);
    await page.keyboard.press("Enter"); await page.keyboard.press("Escape");
    await page.waitForFunction(name => document.querySelector(`input[aria-label="Needed for ${name}"]`)
      ?.closest("li")?.querySelector('[data-testid="cart-line-estimate"]')?.dataset.priceState === "not-requested", {}, newItemName);
    await summary("56 gil", true); await checkBuildReference();
    await record("device-new-item-not-requested-with-existing-prices");
    await click(tid("device-prices-refresh"));
    await page.waitForFunction(name => document.querySelector(`input[aria-label="Needed for ${name}"]`)
      ?.closest("li")?.querySelector('[data-testid="cart-line-estimate"]')?.dataset.priceState === "no-supply", {}, newItemName);
    await summary("56 gil", true); await checkBuildReference();
    await record("device-new-item-confirmed-no-supply-after-lookup");
    await click(`button[aria-label="Remove ${newItemName}"]`);
    await page.waitForFunction(name => !document.querySelector(`input[aria-label="Needed for ${name}"]`), {}, newItemName);
    await summary("56 gil", false);
    await checkBuildReference();
    await record("device-mobile-build-real-market");
    await click(tid("guest-shop-mode")); await click(tid("shop-cheapest"));
    await page.waitForSelector(tid("shop-stack-quantity"));
    await replace(tid("shop-stack-quantity"), 1); await click(tid("shop-stack-bought"));
    await click(tid("guest-build-mode"));
    await click(`button[aria-label="Details for ${itemName}"]`);
    await page.waitForFunction(name => document.querySelector(`input[aria-label="Owned for ${name}"]`)?.value === "1", {}, itemName);
    await summary("44 gil", false);
    await click(tid("guest-shop-mode"));
    await page.$eval(tid("shop-estimate"), details => { details.open = true; });
    assert.match(await page.$eval(tid("shop-build-reference"), node => node.textContent), /Build estimate: 56 gil/, "active trip retains its pre-purchase Build reference");
    await click(tid("guest-build-mode"));
    await record("device-mobile-partial-purchase-build");
    await page.waitForFunction(() => document.querySelector('[data-testid="device-list-status"]')?.textContent.includes("Saved on this device"));
    await page.setViewport({ width: 1280, height: 900 });
    await load(deviceUrl);
    await click(tid("device-prices-refresh")); await summary("44 gil", false);
    await checkBuildReference();
    await record("device-desktop-direct-reload");
    await load("/list");
    await page.evaluate(() => { window.__pricedDeviceSpa = true; });
    await click(`a[href^="${deviceUrl}"]`);
    await click(tid("device-prices-refresh")); await summary("44 gil", false);
    assert.equal(await page.evaluate(() => window.__pricedDeviceSpa), true);
    await record("device-desktop-client-navigation");
    const single = await createList([[null, 5]]);
    await refreshOwner(); await market.set("partial"); await stock(single);
    await load(`/list/${single}`); await summary("20 gil", true);
    await checkBuildReference();
    assert.match(await page.$eval(tid("list-estimate-status"), node => node.textContent), /3 units still unpriced/);
    await record("partial-supply-known-subtotal");
    await refreshOwner(); await market.set("empty"); await stock(single);
    await load(`/list/${single}`); await summary("—", true);
    await checkBuildReference();
    await record("empty-supply-unknown-price");
    await refreshOwner(); await market.set("baseline"); await stock(single);
    const noSupply = await createList([[null, 1, 5057]]);
    await stock(noSupply); await load(`/list/${noSupply}`); await summary("—", true);
    await checkBuildReference();
    await record("unlisted-item-alongside-priced-market");
    const acquired = await createList([[null, 5, 5056, 5]]);
    await load(`/list/${acquired}`); await summary("0 gil", false);
    await checkBuildReference(); await record("fully-acquired-build-reference");
    await load(`/list/${single}`); await summary("56 gil", false);
    await refreshOwner(); await market.set("changed"); await stock(single);
    await load(`/list/${single}`); await summary("76 gil", false);
    await record("same-listing-id-changed-price");
    await refreshOwner(); await market.set("disappeared"); await stock(single);
    await load(`/list/${single}`); await summary("76 gil", false);
    await record("listing-disappeared-repriced");
    assert.deepEqual(errors, [], "no uncaught application errors");
  } catch (error) {
    originalError = error;
    console.error(`[timing] failure at ${Date.now() - startedAt}ms; owner test-auth setup age ${Date.now() - ownerSetupAt}ms`);
    if (page && !page.isClosed()) {
      try {
        const document = await page.evaluate(() => ({ url: location.href, title: document.title,
          readyState: document.readyState, hydrated: window.__pricedHydrated,
          bootStatus: document.querySelector("#boot-progress-status")?.textContent,
          body: document.body?.innerText.slice(0, 3000) }));
        const diagnostic = { lastNavigation, document, failedRequests, failedResponses, pageErrors: errors };
        console.error(`[diagnostic] ${JSON.stringify(diagnostic)}`);
        if (artifactsReady) {
          fs.writeFileSync(path.join(artifacts, "failure.json"), JSON.stringify(diagnostic, null, 2));
          await page.screenshot({ path: path.join(artifacts, "failure.png"), fullPage: true });
        }
      } catch (diagnosticError) { console.error(`[diagnostic capture failed] ${diagnosticError}`); }
    }
    throw error;
  } finally {
    const cleanup = (name, operation) => async () => {
      try { await operation(); } catch (error) {
        errors.push(`${name}: ${error}; ${error.errors?.map(String).join("; ") || ""}`);
        throw error;
      }
    };
    await finishCleanup([
      cleanup("Owner login", async () => {
        if (market || listIds.length || deviceUrl) await login(OWNER);
      }),
      cleanup("Device list", async () => {
        if (!deviceUrl) return;
        await load(deviceUrl); await click(tid("device-list-storage-toggle"));
        await click(tid("device-list-delete")); await click(tid("device-list-confirm-delete"));
        await page.waitForFunction(() => location.pathname === "/list");
      }),
      cleanup("Owned account lists", async () => {
        if (listIds.length) await cleanupOwnedLists(api, listIds);
      }),
      cleanup("Market fixture", async () => { if (market) await market.cleanup(); }),
      async () => {
        if (artifactsReady) fs.writeFileSync(path.join(artifacts, "results.json"), JSON.stringify({ build: process.env.LISTS_ACCEPTANCE_BUILD || null, fixture: market?.manifest, results, errors }, null, 2));
      },
      () => browser.close(),
    ], originalError);
  }
  assert.deepEqual(errors, [], "cleanup must succeed");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
