"use strict";
// Executable on the final #1480 build, or callable from #1478's priced matrix.
// Requires its real DB market fixture helpers and test-auth writer isolation.
const assert = require("node:assert/strict");
const { cleanupOwnedLists, finishCleanup } = require("./list-fixture-cleanup.cjs");
const { assertFrontier } = require("./list-travel-contract.cjs");
const tid = id => `[data-testid="${id}"]`;
const OWNER = 990000001480;

const apiFor = page => (method, route, body) => page.evaluate(async ({ method, route, body }) => {
  const response = await fetch(route, { method, headers: { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await response.text();
  let parsed; try { parsed = text ? JSON.parse(text) : null; } catch { parsed = text; }
  return { status: response.status, body: parsed };
}, { method, route, body });

async function runTravelOnline({ browser, base, market, record = async () => {} }) {
  const context = await browser.createBrowserContext();
  let page;
  const errors = [];
  let api;
  let name;
  let adoptionAttempted = false;
  const ids = [];
  let originalError;
  let devicePath;
  let home;
  let foreignDc;
  let exclusions;
  async function load(route) {
    await page.bringToFront();
    const response = await page.goto(new URL(route, base).href, { waitUntil: "domcontentloaded" });
    assert(response.ok(), `load ${route}: ${response.status()}`);
    await page.waitForFunction(() => window.__travelHydrated);
  }
  async function click(selector) {
    await page.waitForSelector(selector, { visible: true });
    await page.$eval(selector, node => node.scrollIntoView({ block: "center", behavior: "instant" }));
    await page.click(selector);
  }
  async function replace(selector, value) {
    await click(selector); await page.$eval(selector, node => node.select());
    await page.keyboard.press("Backspace"); await page.type(selector, String(value));
  }
  async function summary() {
    await page.waitForFunction(() => {
      const node = document.querySelector('[data-testid="list-estimate-total"]');
      return node?.textContent === "36 gil" && node.dataset.incomplete === "false";
    });
  }
  function query(url, device) {
    assert.equal(url.origin, new URL(base).origin);
    assert.equal(url.searchParams.get("travel"), "dc");
    assert.equal(url.searchParams.get("excluded-worlds"), exclusions);
    assert.equal(url.searchParams.get("excluded-datacenters"), foreignDc.name);
    assert.equal(url.searchParams.get("buy"), "true");
    assert.equal(url.searchParams.get("labs"), "lists-sync");
    assert.equal(url.searchParams.get("make_online"), device ? "1" : null);
    assert.equal(url.searchParams.has("recovery"), false);
    // The app appends its own `lang` after navigation; it is not a handoff key.
    assert.deepEqual([...url.searchParams.keys()].filter(key => key !== "lang").sort(), ["labs", "buy", "travel", "excluded-worlds", "excluded-datacenters", ...(device ? ["make_online"] : [])].sort(), "handoff only forwards explicitly supported list settings");
    assert.match(url.search, /excluded-worlds=\d+%2C\d+/i, "world list remains one encoded value");
  }
  async function shop() {
    await click(tid("guest-shop-mode"));
    await click((tid("shop-route-option") + '[data-route-cheapest="true"]'));
    await page.waitForSelector(tid("shop-totals"));
    assert.match(await page.$eval(tid("shop-totals"), node => node.textContent), /36 gil.*0 missing/);
    await page.$eval(tid("shop-estimate"), node => { node.open = true; });
    assert.match(await page.$eval(tid("shop-build-reference"), node => node.textContent), /Build estimate: 36 gil/);
    assert.equal(await page.$eval(tid("shop-build-reference"), node => node.dataset.incomplete), "false");
    await assertFrontier(page, [{ cost: 36, missing: 0, worlds: [home.id] }]);
    const keys = await page.$$eval('[data-shop-key]', rows => rows.map(row => Number(row.dataset.shopKey)));
    const expected = market.manifest.listings.filter(row => row.world_id === home.id && row.item_id === market.manifest.item_ids[0]);
    assert.equal(expected.length, 1, "baseline home contains one actual three-unit stack");
    assert.deepEqual(keys, expected.map(row => row.id), "Shop retains the same filtered physical offer IDs");
    assert((await page.$eval(tid("shop-stop-title"), node => node.textContent)).includes(home.name));
  }
  try {
    page = await context.newPage();
    page.setDefaultTimeout(Number(process.env.TIMEOUT_MS || 90000));
    await page.setViewport({ width: 1280, height: 900 });
    await page.evaluateOnNewDocument(() => {
      window.__travelHydrated = false;
      addEventListener("ultros:hydrated", () => { window.__travelHydrated = true; });
    });
    page.on("pageerror", error => errors.push(String(error.stack || error)));
    api = apiFor(page);
    name = `Travel online ${market.token}`;
    const [first, fixtureHome, foreign] = market.manifest.worlds;
    home = fixtureHome;
    foreignDc = market.region.datacenters.find(dc => dc.id === foreign.datacenter_id);
    assert(foreignDc && home.id > first.id, "fixture has actual higher-ID home and foreign DC");
    exclusions = `${first.id},${foreign.id}`;
    await page.setCookie(...[["LABS", "lists-sync"], ["HIDE_ADS", "true"], ["i18n_pref_locale", "en"], ["HOME_WORLD", home.name]]
      .map(([name, value]) => ({ name, value, url: base, path: "/" })));
    await load("/list"); await click(tid("list-new"));
    await replace(tid("device-list-name"), name); await click(tid("device-list-create"));
    await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    devicePath = new URL(page.url()).pathname;
    const item = market.manifest.item_names[0];
    await replace('input[aria-label="Quantity to add"]', 3);
    await replace('input[aria-label="Add an item"]', item);
    await page.waitForSelector(`button[aria-label="Add ${item}"]`);
    await page.keyboard.press("Enter"); await page.keyboard.press("Escape");
    await replace(`${tid("device-price-controls")} input[role="combobox"]`, market.region.name);
    await page.waitForFunction(name => [...document.querySelectorAll('button[role="option"]')].some(node => node.textContent.trim().endsWith(name)), {}, market.region.name);
    await page.evaluate(name => [...document.querySelectorAll('button[role="option"]')].find(node => node.textContent.trim().endsWith(name)).click(), market.region.name);
    await page.waitForFunction(() => document.querySelector('[data-testid="device-list-status"]')?.textContent === "Saved on this device");
    // Leaving the device page while the guest-offline worker is still caching
    // the wasm aborts the next document's own wasm fetch (lists-v2 waits too).
    await page.waitForFunction(() => typeof window.__ULTROS_GUEST_OFFLINE_READY__ === "boolean");
    const filtered = new URL(devicePath, base);
    filtered.search = new URLSearchParams({ labs: "lists-sync", "excluded-worlds": exclusions, "excluded-datacenters": foreignDc.name, debug: "must-not-forward" }).toString();
    await load(filtered.href);
    await page.select(tid("list-travel-limit"), "dc");
    await page.waitForFunction(() => new URL(location.href).searchParams.get("travel") === "dc");
    await click(tid("device-prices-refresh")); await summary(); await shop();
    await record("device-travel-before-sign-in", page);
    assert.equal((await context.cookies()).some(cookie => cookie.name === "discord_auth"), false);
    const signIn = new URL(await page.$eval(tid("device-list-make-online-sign-in"), node => node.href));
    assert.equal(signIn.origin, new URL(base).origin); assert.equal(signIn.pathname, "/login");
    const next = new URL(signIn.searchParams.get("next"), base);
    assert.equal(next.pathname, devicePath); query(next, true);
    // Test-auth replaces only Discord OAuth; actual device return and automatic
    // Make online use production adoption, exact-ack and draft guards.
    const login = new URL("/test/login", base);
    login.search = new URLSearchParams({ user_id: String(OWNER), username: "TravelOnline", redirect: next.pathname + next.search }).toString();
    adoptionAttempted = true;
    await load(login.href);
    await page.waitForFunction(() => /^\/list\/\d+$/.test(location.pathname));
    const online = new URL(page.url()); ids.push(Number(online.pathname.split("/").at(-1)));
    query(online, false);
    await page.waitForFunction(() => document.querySelector('[data-testid="guest-shop-mode"]')?.getAttribute("aria-pressed") === "true");
    await click(tid("guest-build-mode")); await summary(); await shop();
    const stock = await api("GET", `/api/v1/list/${ids[0]}/listings`);
    assert.equal(stock.status, 200);
    assert.equal(stock.body[1].length, 1);
    assert.equal(stock.body[1][0][0].quantity, 3);
    market.assertStock(stock.body[1][0][1], market.manifest.item_ids[0]);
    await record("online-travel-after-sign-in", page);
    await page.setViewport({ width: 390, height: 844 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "mobile travel controls must fit");
    await record("online-travel-mobile", page);
    assert.deepEqual(errors, [], "no browser errors throughout actual handoff");
  } catch (error) { originalError = error; throw error; }
  finally {
    await finishCleanup([
      async () => {
        if (!adoptionAttempted) return;
        // Recover our acknowledged account ID even if an assertion failed
        // immediately after adoption; the unique name belongs to this run.
        await load(`/test/login?user_id=${OWNER}&username=TravelOnline&redirect=/list`);
        const inventory = await api("GET", "/api/v1/list");
        assert.equal(inventory.status, 200);
        ids.push(...inventory.body.filter(entry => entry.list.name === name).map(entry => entry.list.id));
        await cleanupOwnedLists(api, ids);
      },
      () => context.close(), // Ephemeral browser storage contains the device backup.
    ], originalError);
  }
}

async function main() {
  assert(process.env.LISTS_ACCEPTANCE_BUILD, "set LISTS_ACCEPTANCE_BUILD to the exact tested source/build fingerprint");
  const base = new URL(process.env.BASE_URL || "http://127.0.0.1:53134").origin;
  const browser = await require("puppeteer").launch({ headless: true });
  let market; let originalError;
  try {
    const owner = await browser.newPage();
    await owner.goto(`${base}/test/login?user_id=${OWNER}&username=TravelOnline&redirect=/list`, { waitUntil: "domcontentloaded" });
    const api = apiFor(owner);
    const worlds = await api("GET", "/api/v1/world_data"); assert.equal(worlds.status, 200);
    market = await require("./list-market-fixture.cjs").createMarketFixture(api, worlds.body);
    const fs = require("node:fs"); const path = require("node:path");
    const dir = path.join(__dirname, "artifacts", "list-travel-online"); fs.mkdirSync(dir, { recursive: true });
    await runTravelOnline({ browser, base, market, record: async (name, page) => {
      await page.screenshot({ path: path.join(dir, `${name}.png`), fullPage: true });
      console.log(`[PASS] ${name} ${page.url()}`);
    } });
    console.log(`Travel Make online passed: ${process.env.LISTS_ACCEPTANCE_BUILD}`);
  } catch (error) { originalError = error; throw error; }
  finally { await finishCleanup([async () => { if (market) await market.cleanup(); }, () => browser.close()], originalError); }
}
module.exports = { runTravelOnline };
if (require.main === module) main().catch(error => { console.error(error); process.exitCode = 1; });
