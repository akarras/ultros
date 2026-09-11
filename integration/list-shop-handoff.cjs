#!/usr/bin/env node
"use strict";

// Account Build → Shop → Build handoff under Labs `lists-sync` (#1437).
// Requires a server built with the `test-auth` bin feature. Market data is
// optional: when the local market has stacks for ITEM_NAME the priced journey
// runs (partial purchase, owned count, refresh review); otherwise the
// unknown-price journey runs. The log says which one ran.
//
//   BASE_URL=http://127.0.0.1:8080 node ./list-shop-handoff.cjs
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { capture } = require("./capture.cjs");

const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
const ITEM = process.env.ITEM_NAME || "Maple Log";
const USER = { id: 990000000031, username: "ShopHandoffOwner" };
const testId = id => `[data-testid="${id}"]`;

async function main() {
  const { default: puppeteer } = await import("puppeteer");
  const browser = await puppeteer.launch({
    headless: process.env.HEADLESS !== "false", args: ["--no-sandbox"],
  });
  const page = await browser.newPage();
  page.setDefaultTimeout(TIMEOUT_MS);
  await page.setViewport({ width: 1280, height: 900 });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  let listId = null;

  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, {
      method,
      credentials: "include",
      headers: body === undefined ? {} : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await response.text();
    let parsed = null;
    try { parsed = text ? JSON.parse(text) : null; } catch { parsed = text; }
    return { status: response.status, body: parsed };
  }, { method, route, body });
  const visible = selector => page.$eval(selector, element => !element.closest(".hidden"));
  const text = selector => page.$eval(selector, element => element.textContent);
  const waitValue = (selector, expected) => page.waitForFunction(
    (selector, expected) => document.querySelector(selector)?.value === expected,
    {}, selector, String(expected));
  async function replace(selector, value) {
    await page.waitForSelector(selector, { visible: true });
    // Triple-click never selects a number input, and a right-aligned cell
    // puts the caret before the digits; select explicitly instead.
    await page.click(selector);
    await page.$eval(selector, input => input.select());
    await page.keyboard.press("Backspace");
    await page.type(selector, String(value));
  }
  // The account page scrolls smoothly; puppeteer's own scroll-into-view can
  // fire the click before the button has arrived, so settle it instantly.
  async function click(selector) {
    await page.waitForSelector(selector, { visible: true });
    await page.$eval(selector, element => element.scrollIntoView({ block: "center", behavior: "instant" }));
    await page.click(selector);
  }
  async function shopMode(shop) {
    await click(testId(shop ? "guest-shop-mode" : "guest-build-mode"));
    await page.waitForFunction((selector, shop) =>
      document.querySelector(selector)?.getAttribute("aria-pressed") === String(shop),
    {}, testId("guest-shop-mode"), shop);
  }

  try {
    const login = new URL("/test/login", BASE_URL);
    login.searchParams.set("user_id", String(USER.id));
    login.searchParams.set("username", USER.username);
    login.searchParams.set("redirect", "/list");
    const response = await page.goto(login.toString(), { waitUntil: "domcontentloaded" });
    assert(response && response.status() < 400, `test login failed: ${response?.status()}`);
    const worldData = await api("GET", "/api/v1/world_data");
    assert.equal(worldData.status, 200, "world_data");
    const region = worldData.body.regions[0];
    const world = region.datacenters[0].worlds[0];
    await page.setCookie(
      { name: "LABS", value: "lists-sync", url: BASE_URL, path: "/" },
      { name: "HIDE_ADS", value: "true", url: BASE_URL, path: "/" },
      { name: "HOME_WORLD", value: world.name, url: BASE_URL, path: "/" },
      { name: "i18n_pref_locale", value: "en", url: BASE_URL, path: "/" },
    );
    const name = `Shop handoff E2E ${Date.now()}`;
    const create = await api("POST", "/api/v1/list/create", { name, wdr_filter: { Region: region.id } });
    assert.equal(create.status, 200, `create list: ${JSON.stringify(create.body)}`);
    const lists = await api("GET", "/api/v1/list");
    listId = lists.body.find(entry => entry.list.name === name)?.list.id;
    assert(listId, "created list is listed for its owner");
    console.log(`[step] list ${listId} created; adding ${ITEM} ×4 in Build`);

    await page.goto(new URL(`/list/${listId}`, BASE_URL).href, { waitUntil: "domcontentloaded" });
    await page.waitForSelector(testId("list-view-sync"));
    await page.waitForSelector(testId("inline-list-add"));
    await replace('input[aria-label="Quantity to add"]', "4");
    await replace('input[aria-label="Add an item"]', ITEM);
    await page.waitForSelector(`button[aria-label="Add ${ITEM}"]`);
    await page.keyboard.press("Enter");
    const needed = `input[aria-label="Needed for ${ITEM}"]`;
    const owned = `input[aria-label="Owned for ${ITEM}"]`;
    await waitValue(needed, 4);
    await page.keyboard.press("Escape");
    await page.waitForFunction(async id => {
      const response = await fetch(`/api/v1/list/${id}/listings`);
      return response.ok && (await response.json())[1].length >= 1;
    }, {}, listId);
    const listings = await api("GET", `/api/v1/list/${listId}/listings`);
    const stacks = listings.body[1][0][1].length;
    const priced = stacks > 0;
    console.log(`[info] ${ITEM}: ${stacks} listings in scope → ${priced ? "priced" : "unknown-price"} journey`);

    // A row added locally shows no price until the listings cache is bumped
    // (docs/lists-sync.md). Reload straight into Shop so the server render
    // and the hydrated client both start from the synced row.
    await page.goto(new URL(`/list/${listId}?buy=true`, BASE_URL).href, { waitUntil: "domcontentloaded" });
    await page.waitForSelector(testId("inline-list-add"));
    await page.waitForSelector(testId("shop-cart-summary"), { visible: true });
    const summary = await text(testId("shop-cart-summary"));
    assert.match(summary, /1 items · 4 units left to buy/, summary);
    assert.match(summary, priced ? /1 priced/ : /0 priced/, summary);
    assert.match(await text(testId("shop-handoff")), new RegExp(`Home world: ${world.name}`),
      "the handoff names the home world the trip starts from");
    assert.equal(await visible(testId("shop-no-prices")), !priced, "unknown prices are called out only when nothing is priced");
    await click(testId("shop-cheapest"));
    await page.waitForSelector(testId("shop-totals"));
    const planned = await text(testId("shop-totals"));
    if (priced) {
      assert.doesNotMatch(planned, /^0 gil/, planned);
      await page.$eval(testId("shop-estimate"), details => { details.open = true; });
      assert.match(await text(testId("shop-estimate")), /Build estimate: [1-9]\d* gil \(1 of 1 items priced/);
      console.log("[step] recording a partial purchase of 1 unit from the first stack");
      const before = await text(testId("shop-stack-description"));
      await replace(testId("shop-stack-quantity"), "1");
      await click(testId("shop-stack-bought"));
      await page.waitForFunction((selector, before) => {
        const description = document.querySelector(selector)?.textContent;
        return description && description !== before;
      }, {}, testId("shop-stack-description"), before);
      assert.equal(await text(testId("shop-totals")), planned, "recording a purchase does not replan the trip");
    } else {
      assert.match(planned, /0 gil · 0 surplus · 4 missing/, planned);
    }
    assert.equal(await visible(testId("shop-drift")), false, "a fresh trip reports no drift");
    console.log("[ok] Shop hands off the cart with its totals explained");

    await shopMode(false);
    await page.waitForSelector(needed, { visible: true });
    assert.equal(await visible(testId("shop-totals")), false, "Shop stays mounted but hidden in Build");
    if (priced) {
      // Owned lives behind the compact cart's details toggle (#1434).
      await click(`button[aria-label="Details for ${ITEM}"]`);
      await waitValue(owned, 1);
      console.log("[ok] the partial purchase reached the Build grid as an owned unit");
    }
    await replace(needed, 6);
    await page.keyboard.press("Enter");
    await waitValue(needed, 6);
    await shopMode(true);
    await page.waitForSelector(testId("shop-totals"), { visible: true });
    assert.equal(await text(testId("shop-totals")), planned, "returning to Shop keeps the trip exactly as planned");
    await page.waitForFunction(selector => {
      const element = document.querySelector(selector);
      return element && !element.closest(".hidden") && /1 quantity change/.test(element.textContent);
    }, {}, testId("shop-drift"));
    await click(testId("shop-refresh"));
    await page.waitForSelector(testId("shop-review"));
    assert.equal(await text(testId("shop-totals")), planned, "a refresh is reviewed before it replaces the trip");
    await click(testId("shop-review-keep"));
    await page.waitForFunction(selector => !document.querySelector(selector), {}, testId("shop-review"));
    await click(testId("shop-refresh"));
    await page.waitForSelector(testId("shop-review-apply"));
    const refreshed = await text(testId("shop-review-next"));
    await click(testId("shop-review-apply"));
    await page.waitForFunction(selector => !!document.querySelector(selector)?.closest(".hidden"), {}, testId("shop-drift"));
    const adopted = await text(testId("shop-totals"));
    // 6 needed; the priced journey owns 1 by now, the unknown one nothing.
    assert.match(adopted, priced ? /gil/ : /6 missing/, `${adopted} after adopting ${refreshed}`);
    console.log("[ok] Build edits keep the chosen trip; the refresh was reviewed, kept, then adopted");

    if (priced) {
      // A listing that is gone in game is excluded, and the replacement is
      // reviewed like any other refresh rather than swapped in underneath.
      console.log("[step] excluding the first stack as gone and reviewing the replacement");
      const gone = await text(testId("shop-stack-description"));
      await click(testId("shop-stack-gone"));
      await page.waitForFunction(() => Array.from(document.querySelectorAll('[role="status"]'))
        .some(element => /Listing excluded/.test(element.textContent)));
      assert.equal(await text(testId("shop-stack-description")), gone, "excluding a listing does not change the trip by itself");
      await click(testId("shop-refresh"));
      await page.waitForSelector(testId("shop-review-apply"));
      await click(testId("shop-review-apply"));
      await page.waitForFunction((selector, gone) => {
        const description = document.querySelector(selector)?.textContent;
        return description && description !== gone;
      }, {}, testId("shop-stack-description"), gone);
      console.log("[ok] the gone listing was replaced through a reviewed refresh");
    }
    assert.deepEqual(errors, [], "no uncaught browser errors");
    console.log("Shop handoff account journey passed.");
  } catch (error) {
    const artifacts = path.join(__dirname, "artifacts", "list-shop-handoff");
    fs.mkdirSync(artifacts, { recursive: true });
    await capture(page, { path: path.join(artifacts, "failure.png"), fullPage: true }).catch(() => {});
    console.error("Page:", page.url(), await page.$eval("body", body => body.innerText.slice(0, 2000)).catch(() => "unavailable"));
    throw error;
  } finally {
    if (listId) await api("DELETE", `/api/v1/list/${listId}/delete`).catch(() => {});
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
