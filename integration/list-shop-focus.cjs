"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { capture } = require("./capture.cjs");
const artifacts = path.join(__dirname, "artifacts/list-shop-focus");
fs.mkdirSync(artifacts, { recursive: true });
const testId = id => `[data-testid="${id}"]`;

async function typeQuantity(page, selector, value) {
  console.log(`[quantity] focus ${selector}`);
  await page.focus(selector);
  console.log(`[quantity] select ${selector}`);
  await page.$eval(selector, input => input.select());
  console.log(`[quantity] type ${value}`);
  await page.keyboard.press("Backspace");
  await page.keyboard.type(String(value));
}

// Reusable by the strict real-market acceptance suite. The caller supplies
// another real list editor, not a synthetic snapshot or local DOM mutation.
// First stop: two purchasable stacks of at least two units; at least one later
// stop. remotePurchase(offerKey, delta) changes the corresponding owned count.
async function runShopFocus(page, { remotePurchase, label, onRemoteCompletion }) {
  const rows = await page.$$eval('[data-shop-key]', nodes => nodes
    .filter(node => !node.querySelector('[data-testid="shop-stack-bought"]').disabled)
    .map(node => ({ key: node.dataset.shopKey, quantity: Number(node.querySelector('input').max) })));
  assert(rows.length >= 2 && rows.slice(0, 2).every(row => row.quantity >= 2), `${label}: required priced first-stop stacks missing`);
  const [first, other] = rows;
  const row = `[data-shop-key="${first.key}"]`;
  const quantity = `${row} ${testId("shop-stack-quantity")}`;
  const bought = `${row} ${testId("shop-stack-bought")}`;
  await typeQuantity(page, quantity, 1);
  assert.notEqual(await page.$eval(quantity, node => node.value), await page.$eval(quantity, node => node.dataset.handoffCommitted), `${label}: changed quantity fences Make online handoff`);
  await page.keyboard.press("Escape");
  assert.equal(await page.$eval(quantity, node => node.value), await page.$eval(quantity, node => node.dataset.handoffCommitted), `${label}: Escape clears the handoff draft fence`);
  await typeQuantity(page, quantity, first.quantity);
  await page.evaluate(selector => {
    window.shopDraftNode = document.querySelector(selector);
    window.shopDraftScroll = window.scrollY;
  }, quantity);
  await remotePurchase(other.key, 1);
  await page.waitForFunction(({ key, before }) => Number(document.querySelector(`[data-shop-key="${key}"] input`).max) === before - 1, {}, { key: other.key, before: other.quantity });
  assert.equal(await page.$eval(quantity, node => node === window.shopDraftNode && node === document.activeElement), true, `${label}: unrelated purchase preserves editor node and focus`);
  assert.equal(await page.$eval(quantity, node => node.value), String(first.quantity), `${label}: unrelated purchase preserves draft`);
  assert.equal(await page.evaluate(() => window.scrollY), await page.evaluate(() => window.shopDraftScroll), `${label}: unrelated purchase preserves scroll`);
  await capture(page, { path: path.join(artifacts, `${label.replace(/[^a-z0-9]+/gi, "-")}-draft.png`), fullPage: false });
  await page.keyboard.press("Tab");
  assert.equal(await page.$eval(bought, node => node === document.activeElement), true);
  await remotePurchase(first.key, 1);
  await page.waitForFunction(({ selector, max }) => Number(document.querySelector(selector)?.max) === max, {}, { selector: quantity, max: first.quantity - 1 });
  assert.equal(await page.$eval(quantity, node => node.value), String(first.quantity), `${label}: changed remaining limit preserves draft for correction`);
  assert.equal(await page.$eval(bought, node => node === document.activeElement), true, `${label}: live limit change keeps Bought focus`);
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => [...document.querySelectorAll('[role="status"]')].some(node => /whole quantity/.test(node.textContent)));
  assert.equal(await page.$eval(quantity, node => Number(node.max)), first.quantity - 1, `${label}: invalid draft did not record a purchase`);

  // A remotely completed stack must move a focused quantity to its Copy
  // control; restore one unit, then finish it through keyboard-only Bought.
  await page.focus(quantity);
  await remotePurchase(first.key, first.quantity - 1);
  await page.waitForFunction(selector => document.querySelector(selector)?.disabled, {}, quantity);
  await page.waitForFunction(selector => document.querySelector(selector)?.querySelector('button') === document.activeElement, {}, row);
  assert.equal(await page.$eval(quantity, node => node.value), await page.$eval(quantity, node => node.dataset.handoffCommitted), `${label}: completed stack retires its non-actionable draft`);
  if (onRemoteCompletion) {
    await onRemoteCompletion();
    return;
  }
  await remotePurchase(first.key, -1);
  await page.waitForFunction(selector => !document.querySelector(selector)?.disabled, {}, quantity);
  await typeQuantity(page, quantity, 1);
  await page.keyboard.press("Tab");
  await page.keyboard.press("Enter");
  await page.waitForFunction(selector => document.querySelector(selector)?.disabled, {}, bought);
  await page.waitForFunction(selector => document.querySelector(selector)?.querySelector('button') === document.activeElement, {}, row);

  const next = testId("shop-next-world");
  assert.equal(await page.$eval(next, node => node.disabled), true, `${label}: the other unfinished stack correctly blocks Next`);
  // Next is gated on the whole stop, not just the first completed stack.
  // Buy each currently actionable remainder through the real controls; later
  // stacks of an item become actionable only after the earlier stack finishes.
  async function finishStop() {
    for (let purchases = 0; purchases < 100; purchases += 1) {
      const pending = await page.$$eval('[data-shop-key]', nodes => {
        const row = nodes.find(node => !node.querySelector('[data-testid="shop-stack-bought"]').disabled);
        return row ? { key: row.dataset.shopKey, quantity: Number(row.querySelector('input').max) } : null;
      });
      if (!pending) {
        const limits = await page.$$eval('[data-shop-key] input', nodes => nodes.map(node => Number(node.max)));
        assert(limits.length > 0 && limits.every(limit => limit === 0), `${label}: disabled controls must represent a completed stop, not a permission or ordering failure: ${limits}`);
        return;
      }
      const stack = `[data-shop-key="${pending.key}"]`;
      await typeQuantity(page, `${stack} ${testId("shop-stack-quantity")}`, pending.quantity);
      await page.keyboard.press("Tab");
      await page.keyboard.press("Enter");
      await page.waitForFunction(selector => document.querySelector(selector)?.disabled, {}, `${stack} ${testId("shop-stack-bought")}`);
      await page.waitForFunction(selector => document.querySelector(selector)?.querySelector('button') === document.activeElement, {}, stack);
    }
    throw new Error(`${label}: stop never completed after 100 purchases`);
  }
  let transitions = 0;
  for (let worlds = 0; worlds < 20; worlds += 1) {
    await finishStop();
    // Disabled only means final after every purchase in this stop is done.
    if (await page.$eval(next, node => node.disabled)) break;
    const world = await page.$eval(testId("shop-stop-title"), node => node.textContent);
    await page.focus(next);
    await page.keyboard.press("Enter");
    await page.waitForFunction(({ selector, world }) => document.querySelector(selector)?.textContent !== world, {}, { selector: testId("shop-stop-title"), world });
    transitions += 1;
    assert.equal(await page.evaluate(() => document.activeElement.closest('[data-shop-key]') !== null), true, `${label}: disabled Next on the new unfinished stop chooses its first stack control`);
    assert.equal(await page.$eval(next, node => node.disabled), true, `${label}: a newly reached unfinished stop gates Next`);
    if (worlds === 19) throw new Error(`${label}: route never reached its final stop`);
  }
  assert(transitions >= 1, `${label}: fixture must exercise a later world`);
  assert.equal(await page.$eval(next, node => node.disabled), true, `${label}: fully completed final stop has no Next`);
  assert.equal(await page.evaluate(() => document.activeElement.closest('[data-shop-key]') !== null), true, `${label}: final purchase retains a usable stack control`);
  console.log(`[ok] ${label}: remote updates, draft limits, keyboard completion and Next world preserve focus`);
}

async function main() {
  const { default: puppeteer } = await import("puppeteer");
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  const errors = [];
  const consoleEvidence = [];
  const requestEvidence = [];
  let listId;
  const createdListIds = new Set();
  const pendingRequests = new Set();
  const requestHandlers = new Map();
  let offers = [];
  let cookie = "";
  async function intercept(target, request) {
    const url = new URL(request.url());
    if (url.hostname === "pagead2.googlesyndication.com" && url.pathname.endsWith("/adsbygoogle.js")) {
      await request.respond({ status: 200, contentType: "application/javascript", body: "/* Third-party ads SDK excluded from this application interaction fixture. */" });
    } else if (target === page && offers.length && url.origin === base && /\/api\/v1\/list\/\d+\/listings$/.test(url.pathname)) {
      const response = await fetch(url, { headers: { cookie } });
      assert(response.ok);
      const body = await response.json();
      body[1] = body[1].map(([row]) => [row, offers]);
      await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
    } else if (target === page && offers.length && url.origin === base && url.pathname.startsWith("/api/v1/bulkListings/")) {
      await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify({ 5056: offers.map(offer => [offer, null]) }) });
    } else await request.continue();
  }
  const prepare = async target => {
    target.setDefaultTimeout(Number(process.env.TIMEOUT_MS || 120000));
    target.setDefaultNavigationTimeout(Number(process.env.TIMEOUT_MS || 120000));
    await target.setCacheEnabled(false);
    await target.setCookie({ name: "HIDE_ADS", value: "true", url: base, path: "/" });
    await target.setRequestInterception(true);
    const handler = request => {
      const pending = intercept(target, request).catch(async error => {
        errors.push(String(error));
        if (!request.isInterceptResolutionHandled()) await request.abort().catch(() => {});
      }).finally(() => pendingRequests.delete(pending));
      pendingRequests.add(pending);
    };
    requestHandlers.set(target, handler);
    target.on("request", handler);
    await target.setViewport({ width: 1280, height: 900 });
    target.on("pageerror", error => errors.push(error.message));
    target.on("console", message => { if (["error", "warn"].includes(message.type())) consoleEvidence.push({ type: message.type(), text: message.text() }); });
    target.on("requestfailed", request => requestEvidence.push({ path: new URL(request.url()).pathname, failure: request.failure()?.errorText }));
    target.on("response", response => { if (response.status() >= 400) requestEvidence.push({ path: new URL(response.url()).pathname, status: response.status() }); });
    target.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
    await target.evaluateOnNewDocument(() => {
      window.shopHydrated = false;
      window.addEventListener("ultros:hydrated", () => { window.shopHydrated = true; });
    });
  };
  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, { method, headers: { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await response.text();
    if (!response.ok) throw new Error(`${route}: ${response.status} ${text}`);
    return text ? JSON.parse(text) : null;
  }, { method, route, body });
  const load = async (target, url) => {
    await target.bringToFront();
    console.log(`[load] ${new URL(url).pathname}`);
    await target.goto(url, { waitUntil: "domcontentloaded" });
    await target.waitForFunction(() => window.shopHydrated);
    console.log(`[hydrated] ${new URL(target.url()).pathname}`);
  };
  try {
    await prepare(page);
    await load(page, `${base}/test/login?user_id=990000001474&username=ShopFocusQA&redirect=/list`);
    const worlds = await api("GET", "/api/v1/world_data");
    const region = worlds.regions.find(region => region.datacenters.some(dc => dc.worlds.length >= 3));
    const [home, second, last] = [...region.datacenters.find(dc => dc.worlds.length >= 3).worlds].sort((a, b) => a.id - b.id);
    await page.setCookie(...[["LABS", "lists-sync"], ["HOME_WORLD", home.name], ["PRICE_ZONE", home.name], ["HIDE_ADS", "true"], ["i18n_pref_locale", "en"]].map(([name, value]) => ({ name, value, url: base, path: "/" })));
    offers = [[-1474001, home, false, 2, 10], [-1474002, home, true, 2, 20], [-1474003, second, false, 3, 12], [-1474004, last, true, 4, 25]].map(([id, world, hq, quantity, price_per_unit]) => ({ id, world_id: world.id, item_id: 5056, retainer_id: 1, price_per_unit, quantity, hq, timestamp: "2026-09-16T00:00:00" }));
    cookie = (await page.cookies()).map(entry => `${entry.name}=${entry.value}`).join("; ");
    // Only prices and the unrelated ads SDK are intercepted. Both editors
    // exercise real persistence/synchronization, and every app error fails.
    const name = `Shop keyboard ${Date.now()}`;
    await api("POST", "/api/v1/list/create", { name, wdr_filter: { Region: region.id } });
    listId = (await api("GET", "/api/v1/list")).find(entry => entry.list.name === name).list.id;
    createdListIds.add(listId);
    for (const hq of [false, true]) await api("POST", `/api/v1/list/${listId}/add/item`, { id: 0, list_id: listId, item_id: 5056, hq, quantity: hq ? 6 : 5, acquired: 0 });
    const remote = await browser.newPage();
    await prepare(remote);
    async function journey(url, label, onRemoteCompletion) {
      await load(page, url);
      await load(remote, url);
      await remote.waitForSelector('[data-testid="cart-rows"] > li');
      await page.bringToFront();
      await page.waitForSelector(testId("guest-shop-mode"));
      await page.click(testId("guest-shop-mode"));
      await page.waitForFunction(() => document.querySelector('[data-testid="guest-shop-mode"]')?.getAttribute("aria-pressed") === "true");
      await page.waitForSelector((testId("shop-route-option") + '[data-route-cheapest="true"]'), { visible: true });
      if (new URL(url).pathname.startsWith("/list/device/")) {
        const lookup = await page.waitForSelector(testId("device-prices-refresh"));
        const response = page.waitForResponse(response => new URL(response.url()).pathname.startsWith("/api/v1/bulkListings/"));
        await lookup.click();
        assert((await response).ok(), "device price fixture must load");
        await page.waitForFunction(() => { const button = document.querySelector('[data-testid="device-prices-refresh"]'); return button?.textContent.trim() === "Refresh prices" && !button.disabled; });
      }
      await page.click((testId("shop-route-option") + '[data-route-cheapest="true"]'));
      await page.waitForSelector('[data-shop-key]');
      await runShopFocus(page, { label, onRemoteCompletion, remotePurchase: async (key, delta) => {
        console.log(`[remote] ${key} ${delta}: foreground`);
        await remote.bringToFront();
        const quality = offers.find(offer => String(offer.id) === key).hq ? "hq" : "nq";
        const index = await remote.$$eval('[data-testid="cart-rows"] > li', (rows, quality) => rows.findIndex(row => row.querySelector('select')?.value === quality), quality);
        assert(index >= 0, `remote ${quality} row missing`);
        const selector = `[data-testid="cart-rows"] > li:nth-child(${index + 1})`;
        const details = `${selector} button[aria-label="Details for Bronze Ingot"]`;
        if (await remote.$eval(details, button => button.getAttribute("aria-expanded")) !== "true") await remote.click(details);
        const owned = `${selector} input[aria-label="Owned for Bronze Ingot"]`;
        const before = Number(await remote.$eval(owned, node => node.value));
        await typeQuantity(remote, owned, before + delta);
        await remote.keyboard.press("Enter");
        console.log(`[remote] ${key} ${delta}: committed`);
        await page.bringToFront();
      } });
    }
    await journey(`${base}/list/${listId}`, "account");
    async function createDevice(suffix) {
      await load(page, `${base}/list`);
      await page.click(testId("list-new"));
      await page.type(testId("device-list-name"), `${name} ${suffix}`);
      // Signed in: the modal defaults to Online; this flow needs a device-only list.
      await page.waitForSelector(testId("device-list-storage-local"), { visible: true });
      await page.click(testId("device-list-storage-local"));
      await page.waitForFunction(sel => document.querySelector(sel)?.getAttribute("aria-pressed") === "true", {}, testId("device-list-storage-local"));
      await page.click(testId("device-list-create"));
      await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
      const deviceUrl = page.url();
      await page.waitForSelector(testId("inline-list-add"));
      for (const hq of [false, true]) {
        await page.select('select[aria-label="Quality to add"]', hq ? "hq" : "nq");
        await typeQuantity(page, 'input[aria-label="Quantity to add"]', hq ? 6 : 5);
        await typeQuantity(page, 'input[aria-label="Add an item"]', "Bronze Ingot");
        await page.waitForSelector('button[aria-label="Add Bronze Ingot"]');
        await page.keyboard.press("Enter");
        await page.keyboard.press("Escape");
      }
      await page.waitForFunction(() => document.querySelector('[data-testid="device-list-status"]')?.textContent.includes("Saved on this device"));
      return deviceUrl;
    }
    await journey(await createDevice("device"), "device");
    await page.waitForFunction(() => document.querySelector('[data-testid="device-list-status"]')?.textContent.includes("Saved on this device"));
    await journey(await createDevice("promotion after completion"), "device promotion", async () => {
      await page.click(testId("device-list-adopt"));
      await page.waitForFunction(() => /^\/list\/\d+$/.test(location.pathname));
      const promoted = Number(new URL(page.url()).pathname.split("/").at(-1));
      createdListIds.add(promoted);
      await page.waitForSelector(testId("list-view-sync"));
      assert.equal(new URL(page.url()).searchParams.get("buy"), "true", "Make online retains Shop mode");
      const projected = await api("GET", `/api/v1/list/${promoted}/listings`);
      assert.deepEqual(projected[1].map(([row]) => row.acquired).sort((a, b) => a - b), [1, 2], "promotion preserves both remote purchases");
      console.log("[ok] remote completion retires a disabled draft and Make online reaches the private account list");
    });
    await Promise.allSettled([...pendingRequests]);
    assert.deepEqual(errors, []);
  } catch (error) {
    console.error("Application errors:", errors);
    console.error("Console evidence:", consoleEvidence.slice(-20));
    console.error("Request evidence:", requestEvidence.slice(-20));
    let failureIndex = 0;
    for (const target of requestHandlers.keys()) {
      try {
        await capture(target, { path: path.join(artifacts, `failure-${failureIndex++}.png`), fullPage: false });
        console.error(await target.evaluate(() => ({ url: location.href, hydrated: window.shopHydrated, visibility: document.visibilityState, active: document.activeElement?.outerHTML.slice(0, 600), text: document.body.innerText.slice(-5000) })));
      } catch {}
    }
    throw error;
  } finally {
    try {
      if (!page.isClosed()) for (const id of createdListIds) await api("DELETE", `/api/v1/list/${id}/delete`).catch(() => {});
      for (const [target, handler] of requestHandlers) target.off("request", handler);
      await Promise.allSettled([...pendingRequests]);
      for (const target of requestHandlers.keys()) if (!target.isClosed()) await target.setRequestInterception(false);
    } finally {
      await browser.close();
    }
  }
}

module.exports = { runShopFocus };
if (require.main === module) main().catch(error => { console.error(error); process.exitCode = 1; });
