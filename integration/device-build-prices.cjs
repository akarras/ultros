"use strict";
// Actual anonymous/account editors, IndexedDB, CRDT and routing; only market
// offers and network timing are controlled. #1478 separately certifies real
// database market rows. No application storage is fabricated by this probe.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const puppeteer = require("puppeteer");
const { runShopAvailability } = require("./list-shop-availability.cjs");
const base = new URL(process.env.BASE_URL || "http://127.0.0.1:8080").origin;
const tid = id => `[data-testid="${id}"]`;
const row = id => `[data-testid="cart-rows"] > li[data-item-id="${id}"]`;
const item = 5056;
let added;

async function main() {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  page.setDefaultTimeout(90000);
  page.setDefaultNavigationTimeout(90000);
  await page.setViewport({ width: 1280, height: 900 });
  const artifacts = path.join(__dirname, "artifacts", "device-build-prices"); fs.mkdirSync(artifacts, { recursive: true });
  const capture = async state => {
    await page.screenshot({ path: path.join(artifacts, `${state}-desktop.png`), fullPage: true });
    await page.setViewport({ width: 390, height: 844 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "mobile price controls do not overflow");
    await page.screenshot({ path: path.join(artifacts, `${state}-mobile.png`), fullPage: true });
    fs.writeFileSync(path.join(artifacts, `${state}.json`), JSON.stringify({ route: page.url(), viewports: [{width:1280,height:900},{width:390,height:844}], builtProduct: process.env.QA_PRODUCT_FINGERPRINT || "record separately" }, null, 2));
    await page.setViewport({ width: 1280, height: 900 });
  };
  const errors = [];
  page.on("pageerror", error => errors.push(String(error.stack || error)));
  page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
  await page.evaluateOnNewDocument(() => {
    window.__pricesHydrated = false;
    window.addEventListener("ultros:hydrated", () => { window.__pricesHydrated = true; });
    window.__priceSockets = [];
    window.__priceDocUpdates = [];
    const NativeSocket = WebSocket;
    window.WebSocket = class extends NativeSocket {
      send(data) {
        if (typeof data === "string" && data.includes('"ListDocUpdate"')) window.__priceDocUpdates.push(data);
        return super.send(data);
      }
      constructor(...args) {
        super(...args); window.__priceSockets.push(this);
        this.addEventListener("message", event => {
          let message; try { message = JSON.parse(event.data); } catch { return; }
          const subscription = message.ListDocSubscribed;
          if (subscription && subscription.list_id === window.__holdInitialPriceDoc) {
            event.stopImmediatePropagation();
            (window.__heldInitialPriceDocs ||= []).push({ socket: this, data: event.data, payload: subscription.payload });
          }
        });
      }
    };
    Object.defineProperty(window, "documentPictureInPicture", { value: undefined, configurable: true });
  });
  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, { method, headers: { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
    if (!response.ok) throw new Error(`${route}: ${response.status}`);
    const text = await response.text(); return text ? JSON.parse(text) : null;
  }, { method, route, body });
  const text = selector => page.$eval(selector, el => el.textContent.trim());
  const replace = async (selector, value) => {
    await page.waitForSelector(selector, { visible: true });
    await page.$eval(selector, el => { el.focus(); el.select(); });
    await page.keyboard.press("Backspace"); await page.type(selector, String(value));
  };
  const setNumber = async (selector, value) => { await replace(selector, value); await page.keyboard.press("Tab"); };
  const load = async url => {
    await page.goto(url, { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__pricesHydrated);
  };
  const saved = () => page.waitForFunction(() => document.querySelector('[data-testid="device-list-status"]')?.textContent.includes("Saved on this device"));
  const addItem = async name => {
    await replace('input[aria-label="Add an item"]', name);
    await page.waitForSelector(`button[aria-label="Add ${name}"]`);
    await page.keyboard.press("Enter");
    await page.keyboard.press("Escape");
  };
  const expectState = (id, state) => page.waitForFunction(({ id, state }) => document.querySelector(`[data-item-id="${id}"] [data-testid="cart-line-estimate"]`)?.dataset.priceState === state, {}, { id, state });
  const expectTotal = value => page.waitForFunction(value => document.querySelector('[data-testid="list-estimate-total"]')?.textContent.trim() === value, {}, value);
  const chooseScope = async name => {
    await replace(`${tid("device-price-controls")} input[role="combobox"]`, name);
    await page.waitForFunction(name => [...document.querySelectorAll('button[role="option"]')].some(el => el.textContent.includes(name)), {}, name);
    await page.evaluate(name => [...document.querySelectorAll('button[role="option"]')].find(el => el.textContent.includes(name)).click(), name);
  };
  const waitHeld = async count => {
    const until = Date.now() + 15000;
    while (held.length < count && Date.now() < until) await new Promise(resolve => setTimeout(resolve, 25));
    assert.equal(held.length, count, "expected held market requests");
  };
  const held = [];
  let holdBulk = false, failBulk = false, failAccount = false, holdAccount = false, unavailableAccount = false, accountId, secondAccountId, world, secondWorld, cookie;
  const heldAccount = [];
  let requests = 0, accountResponses = 0, testError;
  const offer = scope => ({ id: -1471001, world_id: scope === secondWorld?.name ? secondWorld.id : world.id, item_id: item, retainer_id: 1, price_per_unit: scope === secondWorld?.name ? 30 : 10, quantity: 100, hq: false, timestamp: "2026-09-16T00:00:00" });
  const respondBulk = async request => {
    const parts = decodeURIComponent(new URL(request.url()).pathname).split("/");
    const ids = parts.at(-1).split(",").map(Number), scope = parts.at(-2);
    await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(Object.fromEntries(ids.map(id => [id, id === item ? [[offer(scope), null]] : []]))) });
  };
  await page.setRequestInterception(true);
  page.on("request", async request => {
    try {
      const url = new URL(request.url());
      if (url.origin === base && url.pathname.startsWith("/api/v1/bulkListings/")) {
        requests += 1;
        if (holdBulk) held.push(request);
        else if (failBulk) await request.respond({ status: 503, body: "pricing temporarily unavailable" });
        else await respondBulk(request);
      } else if (accountId && url.origin === base && [accountId, secondAccountId].filter(Boolean).some(id => url.pathname === `/api/v1/list/${id}/listings`)) {
        if (holdAccount) { heldAccount.push(request); return; }
        if (failAccount) return await request.respond({ status: 503, body: "offline price refresh" });
        const response = await fetch(url, { headers: { cookie } });
        assert(response.ok); const body = await response.json();
        if (unavailableAccount) body[0].permission = "None";
        body[1] = body[1].map(([itemRow]) => [itemRow, itemRow.item_id === item ? [offer(body[0].list.wdr_filter.World === secondWorld.id ? secondWorld.name : world.name)] : []]);
        await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
        accountResponses += 1;
      } else if (/(^|\.)(googlesyndication\.com|doubleclick\.net|googleadservices\.com)$/.test(url.hostname)) await request.abort();
      else await request.continue();
    } catch (error) { errors.push(String(error.stack || error)); if (!request.isInterceptResolutionHandled()) await request.abort(); }
  });
  try {
    await page.goto(base, { waitUntil: "domcontentloaded" });
    const data = await api("GET", "/api/v1/world_data");
    const worlds = data.regions.flatMap(region => region.datacenters.flatMap(dc => dc.worlds));
    [world, secondWorld] = worlds;
    assert(world && secondWorld, "two worlds required for scope replacement");
    await page.setCookie(...[["LABS", "lists-sync"], ["HIDE_ADS", "true"], ["i18n_pref_locale", "en"], ["HOME_WORLD", world.name], ["PRICE_ZONE", world.name]].map(([name, value]) => ({ name, value, url: base, path: "/" })));
    await load(`${base}/list?labs=lists-sync`);
    await page.click(tid("list-new")); await replace(tid("device-list-name"), `Device prices ${Date.now()}`); await page.click(tid("device-list-create"));
    await page.waitForSelector(tid("device-list-editor")); const deviceUrl = page.url();
    console.log("Device editor ready; adding catalog items");
    // Build prices itself as soon as the list has a row. Fault that first
    // eager lookup so the not-requested state is reachable, then hold the
    // manual retry: a failed lookup is not retried until the list changes.
    assert.equal(requests, 0, "an empty list has nothing to price");
    failBulk = true;
    await addItem("Bronze Ingot"); await page.waitForSelector(row(item));
    await page.waitForSelector(tid("device-prices-error"));
    assert.equal(await text(tid("device-prices-error")), "Could not load prices. Try again.");
    assert.equal(requests, 1, "Build looks prices up on its own once a row lands");
    await expectTotal("—"); await expectState(item, "not-requested");
    failBulk = false;
    holdBulk = true; await page.click(tid("device-prices-refresh"));
    await page.waitForFunction(() => document.querySelector('[data-testid="list-estimate-status"]')?.textContent.includes("Loading prices"));
    await waitHeld(1); holdBulk = false; await respondBulk(held.shift());
    await expectState(item, "priced"); await expectTotal("10 gil");
    // A new row is priced on its own as well; when that lookup fails the
    // row stays unpriced beside the known subtotal.
    failBulk = true;
    await addItem("Iron Ingot");
    await page.waitForSelector('input[aria-label="Needed for Iron Ingot"]');
    added = await page.$eval('input[aria-label="Needed for Iron Ingot"]', el => Number(el.closest("[data-item-id]").dataset.itemId));
    await page.waitForSelector(row(added));
    await page.waitForSelector(tid("device-prices-error"));
    assert.equal(requests, 3, "the added row triggers exactly one more lookup");
    await expectState(item, "priced"); await expectState(added, "not-requested"); await expectTotal("10 gil");
    assert.equal(await text(tid("list-estimate-status")), "Known subtotal only · Price lookup needed: 1 · Unpriced units: 1.");
    await capture("partial-coverage");
    failBulk = false;
    // From here on a trip is being followed, so the editor stops repricing on
    // its own and every later lookup below is the explicit Refresh prices.
    await page.click(tid("guest-shop-mode"));
    if (await page.$('[data-testid="shop-change-route"][aria-expanded="false"]')) await page.click('[data-testid="shop-change-route"]');
    await page.click((tid("shop-route-option") + '[data-route-cheapest="true"]'));
    await page.waitForSelector(tid("shop-build-reference"));
    assert.match(await text(tid("shop-build-reference")), /10 gil/);
    assert.equal(await text(tid("shop-build-coverage")), "Known subtotal only · Price lookup needed: 1 · Unpriced units: 1.");
    await page.click(tid("guest-build-mode"));
    await page.click(tid("device-prices-refresh")); await expectState(added, "no-supply");
    await addItem("Maple Log");
    await page.waitForSelector('input[aria-label="Needed for Maple Log"]');
    const laterItem = await page.$eval('input[aria-label="Needed for Maple Log"]', el => Number(el.closest("[data-item-id]").dataset.itemId));
    await expectState(laterItem, "not-requested"); await expectTotal("10 gil");
    await page.click(`${row(laterItem)} button[aria-label="Remove Maple Log"]`);
    await page.waitForFunction(id => !document.querySelector(`[data-item-id="${id}"]`), {}, laterItem);
    const originalFreshness = await text(tid("list-estimate-freshness")); assert(originalFreshness.includes(world.name));
    await chooseScope(secondWorld.name);
    assert((await text(tid("list-estimate-freshness"))).includes(world.name), "picker does not relabel old prices");
    failBulk = true; await page.click(tid("device-prices-refresh")); await page.waitForSelector(tid("device-prices-error"));
    await expectTotal("10 gil"); assert((await text(tid("list-estimate-freshness"))).includes(world.name));
    assert.equal(await text(tid("device-prices-error")), "Could not load prices. Try again.");
    assert.match(await text(tid("list-estimate-freshness")), /refresh failed/i);
    await capture("failed-refresh");
    failBulk = false; holdBulk = true; await page.click(tid("device-prices-refresh"));
    await chooseScope(world.name); await page.click(tid("device-prices-refresh"));
    await waitHeld(2); await respondBulk(held.pop()); await expectTotal("10 gil");
    await respondBulk(held.pop()); holdBulk = false;
    await page.waitForFunction(() => !document.querySelector('[data-testid="device-prices-refresh"]')?.disabled);
    assert((await text(tid("list-estimate-freshness"))).includes(world.name), "late old-scope response cannot replace newest scope");
    await chooseScope(secondWorld.name); await page.click(tid("device-prices-refresh")); await expectTotal("30 gil");
    assert((await text(tid("list-estimate-freshness"))).includes(secondWorld.name));
    // Offline editing remains immediate; no price response or session is needed.
    await page.evaluate(() => window.__priceSockets.forEach(socket => socket.close()));
    await page.setOfflineMode(true);
    await setNumber(`${row(item)} input[aria-label="Needed for Bronze Ingot"]`, 4); await expectTotal("120 gil"); await saved();
    await page.setOfflineMode(false); await load(deviceUrl);
    // A reopened list prices itself against the scope it saved.
    await expectState(item, "priced"); await expectTotal("120 gil");
    assert((await text(tid("device-price-controls"))).includes(secondWorld.name), "reopen restores selected scope");
    assert.equal(await page.$eval(`${row(item)} input[aria-label="Needed for Bronze Ingot"]`, el => el.value), "4");
    await page.click(`${row(item)} button[aria-label="Details for Bronze Ingot"]`);
    await setNumber(`${row(item)} input[aria-label="Owned for Bronze Ingot"]`, 4);
    await page.click(`${row(added)} button[aria-label="Details for Iron Ingot"]`);
    await setNumber(`${row(added)} input[aria-label="Owned for Iron Ingot"]`, 1);
    await expectTotal("0 gil"); await expectState(item, "acquired");
    assert.match(await text(tid("list-estimate-status")), /Nothing left to buy/);
    await page.screenshot({ path: path.join(artifacts, "desktop.png"), fullPage: true });
    await page.setViewport({ width: 390, height: 844 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "mobile price controls do not overflow");
    await page.screenshot({ path: path.join(artifacts, "mobile.png"), fullPage: true });
    await page.setViewport({ width: 1280, height: 900 });
    console.log("PASS device: eager Build lookup, failed new-row lookup, no-supply coverage, failed/late scope responses, offline edits, reopen scope, all-owned zero");

    const login = await page.goto(`${base}/test/login?user_id=990000001471&username=DeviceBuildPricesQA&redirect=/list`, { waitUntil: "domcontentloaded" }); assert(login.ok());
    const name = `Shared pricing ${Date.now()}`;
    await api("POST", "/api/v1/list/create", { name, wdr_filter: { World: world.id } });
    accountId = (await api("GET", "/api/v1/list")).find(entry => entry.list.name === name).list.id;
    await api("POST", `/api/v1/list/${accountId}/add/item`, { id: 0, list_id: accountId, item_id: item, hq: null, quantity: 2, acquired: 0 });
    cookie = (await page.cookies()).map(entry => `${entry.name}=${entry.value}`).join("; ");
    const accountUrl = `${base}/list/${accountId}?labs=lists-sync`;
    const holdInitial = await page.evaluateOnNewDocument(id => { window.__holdInitialPriceDoc = id; window.__heldInitialPriceDocs = []; }, accountId);
    await load(accountUrl);
    await page.waitForFunction(() => window.__heldInitialPriceDocs?.length > 0);
    const restUntil = Date.now() + 15000;
    while (!accountResponses && Date.now() < restUntil) await new Promise(resolve => setTimeout(resolve, 25));
    assert(accountResponses > 0, "REST pricing proceeds while native document handshake is held");
    const initialStates = await page.evaluate(() => new Promise(resolve => {
      const samples = [];
      function frame() {
        samples.push({ total: document.querySelector('[data-testid="list-estimate-total"]')?.textContent.trim(), status: document.querySelector('[data-testid="list-estimate-status"]')?.textContent });
        if (samples.length === 8) resolve(samples); else requestAnimationFrame(frame);
      }
      requestAnimationFrame(frame);
    }));
    assert(initialStates.every(state => state.total !== "0 gil" && !state.status?.includes("Nothing left to buy")), "permission-known empty receiver must not claim that a nonempty list is free");
    await page.click(tid("guest-shop-mode"));
    assert.equal(await page.$(tid("shop-cart-summary")), null, "Shop must remain unavailable while only REST permission is known");
    await page.removeScriptToEvaluateOnNewDocument(holdInitial.identifier);
    await page.evaluate(() => {
      window.__holdInitialPriceDoc = null;
      for (const { socket, data } of window.__heldInitialPriceDocs.splice(0)) socket.dispatchEvent(new MessageEvent("message", { data }));
    });
    await page.waitForSelector((tid("shop-route-option") + '[data-route-cheapest="true"]'));
    if (await page.$('[data-testid="shop-change-route"][aria-expanded="false"]')) await page.click('[data-testid="shop-change-route"]');
    await page.click((tid("shop-route-option") + '[data-route-cheapest="true"]'));
    await page.waitForSelector(tid("shop-stack-bought"));
    assert.match(await text(tid("shop-build-reference")), /20 gil/, "readable snapshot mounts the shared-price Shop workspace");
    await page.click(tid("guest-build-mode"));
    await expectTotal("20 gil");

    // Local row edits must be reflected in Shop even while REST refresh fails.
    failAccount = true;
    await setNumber(`${row(item)} input[aria-label="Needed for Bronze Ingot"]`, 4); await expectTotal("40 gil");
    await addItem("Iron Ingot"); await expectState(added, "not-requested");
    await page.click(tid("guest-shop-mode"));
    if (await page.$('[data-testid="shop-change-route"][aria-expanded="false"]')) await page.click('[data-testid="shop-change-route"]');
    await page.click((tid("shop-route-option") + '[data-route-cheapest="true"]'));
    await page.waitForSelector(tid("shop-build-reference"));
    assert.match(await text(tid("shop-build-reference")), /40 gil/);
    assert.equal(await text(tid("shop-build-coverage")), "Known subtotal only · Price lookup needed: 1 · Unpriced units: 1.");
    failAccount = false;
    // Bounded client availability fault: keep the same real document and trip,
    // temporarily expose no permission in the REST reply, then restore the
    // authentic reply. Target edits produce real native revalidation broadcasts.
    // This is not a server revocation/purge simulation.
    const referenceBeforeCycle = await text(tid("shop-build-reference"));
    const stack = await page.$eval(tid("shop-stack"), node => node.dataset.shopKey);
    const stackRoot = '[data-shop-key="' + stack + '"]';
    const quantityInput = stackRoot + " " + tid("shop-stack-quantity");
    await replace(quantityInput, "2");
    assert.notEqual(await page.$eval(quantityInput, node => node.value), await page.$eval(quantityInput, node => node.dataset.handoffCommitted), "cycle starts with an unfinished draft");
    const tripBeforeCycle = await page.$$eval("[data-shop-key]", nodes => nodes.map(node => node.dataset.shopKey));
    const worldBeforeCycle = await text(tid("shop-stop-title"));
    const rowBeforeCycle = (await api("GET", "/api/v1/list/" + accountId))[1].find(row => row.item_id === item);
    await page.evaluate(selector => { window.__priceCycleInput = document.querySelector(selector); window.__priceCycleNext = document.querySelector('[data-testid="shop-next-world"]'); window.__priceCycleUpdateCount = window.__priceDocUpdates.length; }, quantityInput);
    unavailableAccount = true;
    await api("POST", "/api/v1/list/item/edit", { ...rowBeforeCycle, target_price: (rowBeforeCycle.target_price || 0) + 1 });
    await page.waitForFunction(() => !document.querySelector('[data-testid="shop-cart-summary"]'));
    assert.equal(await page.$(quantityInput), null, "unavailable purchase controls leave the active document");
    await page.evaluate(() => window.__priceCycleNext?.click());
    assert.equal(await page.evaluate(() => window.__priceDocUpdates.length), await page.evaluate(() => window.__priceCycleUpdateCount), "unavailable controls send no purchase updates");
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    unavailableAccount = false;
    await api("POST", "/api/v1/list/item/edit", { ...rowBeforeCycle, target_price: (rowBeforeCycle.target_price || 0) + 2 });
    await page.waitForSelector(quantityInput);
    assert.equal(await page.$eval((tid("shop-route-option") + '[data-route-cheapest="true"]'), node => node.getAttribute("aria-pressed")), "true", "same-document availability recovery retains the selected trip");
    assert.equal(await text(tid("shop-build-reference")), referenceBeforeCycle, "same-document recovery retains the frozen Build reference");
    assert.deepEqual(await page.$$eval("[data-shop-key]", nodes => nodes.map(node => node.dataset.shopKey)), tripBeforeCycle, "availability recovery retains frozen offer identities");
    assert.equal(await text(tid("shop-stop-title")), worldBeforeCycle, "availability recovery retains the active stop");
    const restoredDraft = await page.$eval(quantityInput, node => ({ value: node.value, valid: node.checkValidity(), sameNode: node === window.__priceCycleInput }));
    console.log("Availability restored draft/node", restoredDraft);
    assert.equal(restoredDraft.sameNode, false, "the held unavailable state must exercise an actual control remount");
    assert.equal(restoredDraft.value, await page.$eval(quantityInput, node => node.dataset.handoffCommitted), "new editor starts at its current valid bound");
    assert(restoredDraft.valid, "restored draft must remain within current bounds");
    assert.equal((await api("GET", "/api/v1/list/" + accountId))[1].find(row => row.item_id === item).acquired, rowBeforeCycle.acquired, "availability cycle does not record a purchase");
    await page.focus(quantityInput);
    const stackRemainder = Number(await page.$eval(quantityInput, node => node.max));
    await api("POST", "/api/v1/list/item/edit", { ...rowBeforeCycle, acquired: rowBeforeCycle.acquired + stackRemainder });
    await page.waitForFunction(selector => document.querySelector(selector)?.disabled, {}, quantityInput);
    await page.waitForFunction(selector => document.querySelector(selector)?.querySelector("button") === document.activeElement, {}, stackRoot);
    await api("POST", "/api/v1/list/item/edit", rowBeforeCycle);
    await page.waitForFunction(selector => !document.querySelector(selector)?.disabled, {}, quantityInput);
    console.log("PASS same-document Shop availability remount retains trip, safely remounts controls, and reattaches focus observer for a real remote completion");
    // Warm a second document, then navigate directly between dynamic list
    // routes while its next price request fails. No full reload can mask state
    // retained by a reused route component.
    const secondName = name + " second";
    await api("POST", "/api/v1/list/create", { name: secondName, wdr_filter: { World: secondWorld.id } });
    secondAccountId = (await api("GET", "/api/v1/list")).find(entry => entry.list.name === secondName).list.id;
    await api("POST", `/api/v1/list/${secondAccountId}/add/item`, { id: 0, list_id: secondAccountId, item_id: item, hq: null, quantity: 2, acquired: 0 });
    await load(`${base}/list/${secondAccountId}?labs=lists-sync`); await expectTotal("60 gil");
    await load(accountUrl); await expectTotal("40 gil");
    failAccount = true; holdAccount = true;
    await page.evaluate(id => {
      window.__priceRouteSamples = [];
      window.__priceRouteSampling = true;
      const sample = () => {
        if (location.pathname === `/list/${id}`) window.__priceRouteSamples.push(document.querySelector('[data-testid="list-estimate-total"]')?.textContent.trim() ?? null);
        if (window.__priceRouteSampling) requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);
      window.__priceRouteToken = "same-document";
      const link = document.createElement("a"); link.href = `/list/${id}?labs=lists-sync`; link.textContent = "Other list";
      document.body.append(link); link.click(); link.remove();
    }, secondAccountId);
    await page.waitForFunction(id => location.pathname === `/list/${id}`, {}, secondAccountId);
    const heldUntil = Date.now() + 15000;
    while (!heldAccount.length && Date.now() < heldUntil) await new Promise(resolve => setTimeout(resolve, 25));
    assert(heldAccount.length > 0, "B price response must actually be pending");
    await page.waitForFunction(() => window.__priceRouteSamples.length >= 5);
    holdAccount = false;
    for (const request of heldAccount.splice(0)) await request.respond({ status: 503, body: "held B pricing unavailable" });
    await page.waitForFunction(() => document.querySelector('[data-testid="list-estimate-status"]')?.textContent.includes("Prices unavailable"));
    const samples = await page.evaluate(() => { window.__priceRouteSampling = false; return window.__priceRouteSamples; });
    assert(!samples.includes("0 gil"), "an old handle cannot label B's loading document as a known empty cart");
    await expectTotal("—"); await expectState(item, "not-requested");
    assert.equal(await page.evaluate(() => window.__priceRouteToken), "same-document", "cross-list check must use client navigation");
    assert.equal(await page.$(tid("list-estimate-freshness")), null, "failed B cannot expose A timestamp or scope");
    failAccount = false;
    console.log("PASS cross-list failed price request keeps identity, scope and coverage fenced; checking incompatible companion");
    await runShopAvailability(page, { url: accountUrl, listId: accountId, readProjection: () => api("GET", `/api/v1/list/${accountId}`) });
    assert.deepEqual(errors, [], "application and fixture errors");
    console.log("PASS account: live document pricing and per-item coverage survive failed REST; incompatible document closes active companion");
  } catch (error) {
    console.error("Price state", await page.evaluate(() => ({ url: location.href, text: document.body.innerText.slice(-7000) })).catch(String), errors);
    testError = error;
  } finally {
    failAccount = false;
    await page.setOfflineMode(false).catch(() => {});
    for (const request of [...held, ...heldAccount]) if (!request.isInterceptResolutionHandled()) await request.abort().catch(() => {});
    const cleanup = await Promise.allSettled([accountId, secondAccountId].filter(Boolean).map(id => api("DELETE", `/api/v1/list/${id}/delete`)));
    cleanup.push(...await Promise.allSettled([browser.close()]));
    const failures = [testError, ...cleanup.filter(result => result.status === "rejected").map(result => result.reason)].filter(Boolean);
    if (failures.length) throw new AggregateError(failures, "Device pricing validation or owned fixture cleanup failed");
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
