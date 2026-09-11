#!/usr/bin/env node
"use strict";

// Anonymous Lists 2.0 browser contract. Run against a freshly built server:
// BASE_URL=http://127.0.0.1:8080 npm --prefix integration run test:lists-v2
// No test-auth, Discord account, or market listings are required. Catalog
// data must be installed. A fresh browser context proves there is no session.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const https = require("node:https");
const path = require("node:path");
const { capture } = require("./capture.cjs");

const testId = id => `[data-testid="${id}"]`;

async function main() {
  const { default: puppeteer } = await import("puppeteer");
  const upstream = new URL(process.env.BASE_URL || "http://127.0.0.1:8080");
  const transport = upstream.protocol === "https:" ? https : http;
  let disconnected = false;
  const sockets = new Set();
  // Page.setOfflineMode does not reliably disconnect the service-worker
  // target. A private proxy lets us sever every fetch from this test's origin
  // without stopping the real app or interfering with another test session.
  const proxy = http.createServer((request, response) => {
    if (disconnected) { request.socket.destroy(); return; }
    const forwarded = transport.request(new URL(request.url, upstream), {
      method: request.method,
      headers: { ...request.headers, host: upstream.host },
    }, incoming => {
      if (incoming.statusCode >= 500) console.error("[proxy]", incoming.statusCode, request.url);
      response.writeHead(incoming.statusCode, incoming.headers);
      incoming.pipe(response);
    });
    forwarded.on("error", () => { response.destroy(); });
    request.pipe(forwarded);
  });
  proxy.on("connection", socket => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  proxy.on("upgrade", (request, socket, head) => {
    if (disconnected) { socket.destroy(); return; }
    const forwarded = transport.request(new URL(request.url, upstream), {
      headers: { ...request.headers, host: upstream.host },
    });
    forwarded.on("upgrade", (incoming, upstreamSocket, upstreamHead) => {
      socket.write(`HTTP/1.1 ${incoming.statusCode} ${incoming.statusMessage}\r\n`);
      for (let i = 0; i < incoming.rawHeaders.length; i += 2) {
        socket.write(`${incoming.rawHeaders[i]}: ${incoming.rawHeaders[i + 1]}\r\n`);
      }
      socket.write("\r\n");
      if (upstreamHead.length) socket.write(upstreamHead);
      if (head.length) upstreamSocket.write(head);
      socket.pipe(upstreamSocket).pipe(socket);
      socket.on("error", () => upstreamSocket.destroy());
      upstreamSocket.on("error", () => socket.destroy());
      socket.on("close", () => upstreamSocket.destroy());
      upstreamSocket.on("close", () => socket.destroy());
    });
    forwarded.on("error", () => socket.destroy());
    forwarded.end();
  });
  await new Promise(resolve => proxy.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${proxy.address().port}`;
  const timeout = Number(process.env.TIMEOUT_MS || 60000);
  const browser = await puppeteer.launch({
    headless: process.env.HEADLESS !== "false", args: ["--no-sandbox"],
  }).catch(error => { proxy.close(); throw error; });
  // Puppeteer gives this fresh browser a disposable profile. Use its regular
  // context: incognito Cache Storage has a smaller in-memory budget that cannot
  // hold the large debug WASM build, unlike the player's normal browser.
  const context = browser.defaultBrowserContext();
  const page = await context.newPage();
  page.setDefaultTimeout(timeout);
  await page.setViewport({ width: 1280, height: 900 });
  await page.setCookie(
    { name: "LABS", value: "lists-sync", url: base, path: "/" },
    { name: "HIDE_ADS", value: "true", url: base, path: "/" },
    { name: "i18n_pref_locale", value: "en", url: base, path: "/" },
  );
  await page.evaluateOnNewDocument(() => {
    window.__listsV2Hydrated = false;
    window.addEventListener("ultros:hydrated", () => { window.__listsV2Hydrated = true; });
    // Exercise the portable fallback deterministically; real PiP visibility
    // over FFXIV and exclusive fullscreen are a separate manual test.
    Object.defineProperty(window, "documentPictureInPicture", { value: undefined, configurable: true });
  });
  const errors = [];
  const accountWrites = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("request", request => {
    const pathname = new URL(request.url()).pathname;
    if (request.method() !== "GET" && /^\/api\/v1\/list(?:\/|$)/.test(pathname)) {
      accountWrites.push(`${request.method()} ${pathname}`);
    }
  });

  async function load(route) {
    const response = await page.goto(new URL(route, base).href, { waitUntil: "domcontentloaded" });
    assert(response?.ok(), `navigation ${route}: ${response?.status()}`);
    await page.waitForFunction(() => window.__listsV2Hydrated);
  }
  async function replace(selector, value) {
    await page.waitForSelector(selector, { visible: true });
    // Triple-click never selects a number input, and a right-aligned cell
    // puts the caret before the digits; select explicitly instead.
    await page.click(selector);
    await page.$eval(selector, input => input.select());
    await page.keyboard.press("Backspace");
    await page.type(selector, String(value));
  }
  async function saved() {
    await page.waitForFunction(selector =>
      document.querySelector(selector)?.textContent.includes("Saved on this device"),
    {}, testId("device-list-status"));
  }
  async function offlineReady() {
    await page.waitForFunction(() => typeof window.__ULTROS_GUEST_OFFLINE_READY__ === "boolean");
    assert.equal(await page.evaluate(() => window.__ULTROS_GUEST_OFFLINE_READY__), true,
      "guest offline preparation succeeds");
  }

  try {
    await load("/?lang=en");
    const documentToken = `guest-entry-${Date.now()}`;
    await page.evaluate(token => { window.__guestEntryDocument = token; }, documentToken);
    await page.waitForFunction(() => Array.from(document.querySelectorAll("a[href]"))
      .some(link => new URL(link.href).pathname === "/list"));
    await page.evaluate(() => Array.from(document.querySelectorAll("a[href]"))
      .find(link => new URL(link.href).pathname === "/list").click());
    await page.waitForSelector(testId("device-list-create"));
    assert.equal(await page.evaluate(() => window.__guestEntryDocument), documentToken,
      "homepage entry opens the guest directory through client-side navigation");
    const name = `Browser guest project ${Date.now()}`;
    await replace(testId("device-list-name"), name);
    await page.click(testId("device-list-create"));
    await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    const deviceUrl = page.url();
    await saved();
    console.log("[step] guest list created; waiting for offline preparation after SPA entry");
    await offlineReady();
    assert.equal(await page.evaluate(() => window.__guestEntryDocument), documentToken,
      "offline resources prepare after SPA guest entry without a document reload");
    assert(!/oauth|login/.test(new URL(deviceUrl).pathname), "guest creation never redirects to authentication");
    console.log("[ok] anonymous player creates a durable device list before authentication");

    const search = 'input[aria-label="Add an item"]';
    const needed = 'input[aria-label="Needed for Bronze Ingot"]';
    const owned = 'input[aria-label="Owned for Bronze Ingot"]';
    const target = 'input[aria-label="Target price for Bronze Ingot"]';
    const quality = 'select[aria-label="Quality for Bronze Ingot"]';
    const details = 'button[aria-label="Details for Bronze Ingot"]';
    // Owned and target price sit behind the row's details toggle in the
    // compact cart; open it (idempotently) before touching either field.
    const openDetails = async (target = page) => {
      await target.waitForSelector(details, { visible: true });
      if (await target.$eval(details, button => button.getAttribute("aria-expanded")) !== "true") {
        await target.click(details);
      }
      await target.waitForSelector(owned, { visible: true });
    };
    const waitValue = (selector, expected) => page.waitForFunction(
      (selector, expected) => document.querySelector(selector)?.value === expected,
      {}, selector, String(expected));
    await replace('input[aria-label="Quantity to add"]', "3");
    await replace(search, "Bronze Ingot");
    await page.waitForSelector('button[aria-label="Add Bronze Ingot"]');
    await page.keyboard.press("Enter");
    await waitValue(needed, 3);
    assert.equal(await page.$eval(search, element => document.activeElement === element), true,
      "Enter adds an item and leaves catalog search ready for the next entry");
    await page.keyboard.press("Enter");
    await waitValue(needed, 6);
    assert.equal(await page.$$('input[aria-label="Needed for Bronze Ingot"]').then(rows => rows.length), 1,
      "duplicate additions increase a single row's need");
    await page.keyboard.press("Escape");
    assert.equal(await page.$('[aria-label="Catalog results"]'), null, "Escape dismisses catalog results");
    await page.waitForSelector(testId("list-estimate-total"));
    assert.match(await page.$eval(testId("list-estimate-status"), element => element.textContent), /prices|units|Nothing left/i,
      "the cart summary explains what its estimated total covers (or why there is none yet)");
    await page.select(quality, "nq");
    await page.waitForFunction(selector => document.querySelector(selector)?.value === "nq", {}, quality);
    await saved();
    assert.equal(await page.$(owned), null, "owned quantity is not a default column in the compact cart");
    console.log("[ok] compact cart shows the estimate summary and commits a quality change");
    await replace(needed, 9);
    await page.keyboard.press("Escape");
    await page.keyboard.press("Tab");
    await waitValue(needed, 6);
    await page.$eval(quality, element => { window.__qualityBeforeCommit = element; });
    await replace(needed, 8);
    await page.keyboard.press("Tab");
    await waitValue(needed, 8);
    await saved();
    assert.equal(await page.$eval(quality, element =>
      element === window.__qualityBeforeCommit && document.activeElement === element), true,
    "Tab commits Needed and preserves the existing Quality control and keyboard focus");
    await openDetails();
    await replace(owned, 2);
    await page.keyboard.press("Enter");
    await replace(target, 125);
    await page.keyboard.press("Enter");
    await saved();
    await load(deviceUrl);
    await waitValue(needed, 8);
    await openDetails();
    await waitValue(owned, 2);
    await waitValue(target, 125);
    console.log("[ok] inline add, duplicate quantities, focus, Escape and edited values survive reload");
    const shots = path.join(__dirname, "artifacts", "lists-v2");
    fs.mkdirSync(shots, { recursive: true });
    await capture(page, { path: path.join(shots, "cart-desktop.png"), fullPage: true }).catch(() => {});
    await page.setViewport({ width: 390, height: 844, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });
    // Changing isMobile/hasTouch reloads the page: wait for the cart itself
    // so the width assertion and the capture see rows, not the loading screen.
    await waitValue(needed, 8);
    await page.waitForFunction(() => document.documentElement.scrollWidth <= window.innerWidth + 1);
    await capture(page, { path: path.join(shots, "cart-mobile.png"), fullPage: true }).catch(() => {});
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), true,
      "the compact cart fits a 390px viewport without horizontal scroll");
    await page.setViewport({ width: 1280, height: 800 });
    // Toggling isMobile/hasTouch makes puppeteer reload the page, which
    // closes the details panel the cross-tab check reads Owned from.
    await waitValue(needed, 8);
    await openDetails();

    console.log("[step] opening a second guest editor for the cross-tab focus check");
    const secondEditor = await browser.newPage();
    secondEditor.setDefaultTimeout(timeout);
    try {
      await secondEditor.goto(deviceUrl, { waitUntil: "domcontentloaded" });
      await openDetails(secondEditor);
      console.log("[step] second guest editor ready");
      await page.bringToFront();
      await replace(needed, 12);
      // A background writer models an incoming edit without blurring the
      // primary input: switching tabs itself commits the draft by design.
      await secondEditor.$eval(owned, input => {
        input.value = "3";
        input.dispatchEvent(new Event("change", { bubbles: true }));
      });
      await waitValue(owned, 3);
      assert.equal(await page.$eval(needed, element =>
        element.value === "12" && document.activeElement === element), true,
      "another tab's Owned update preserves the active Needed draft and focus");
      await page.keyboard.press("Escape");
      await page.keyboard.press("Tab");
      await waitValue(needed, 8);
      await secondEditor.$eval(owned, input => {
        input.value = "2";
        input.dispatchEvent(new Event("change", { bubbles: true }));
      });
      await waitValue(owned, 2);
      await saved();
      console.log("[ok] remote row updates preserve an uncommitted keyboard draft");
    } catch (error) {
      console.error("Cross-tab focus check:", error);
      throw error;
    } finally {
      if (browser.connected) await secondEditor.close();
    }

    await offlineReady();
    disconnected = true;
    for (const socket of sockets) socket.destroy();
    await page.setOfflineMode(true);
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__ULTROS_OFFLINE_GUEST__ === true);
    await waitValue(needed, 8);
    await openDetails();
    await replace(owned, 3);
    await page.keyboard.press("Enter");
    await saved();
    await page.reload({ waitUntil: "domcontentloaded" });
    await waitValue(needed, 8);
    await openDetails();
    await waitValue(owned, 3);
    await replace(search, "Bronze Ingot");
    await page.waitForSelector('button[aria-label="Add Bronze Ingot"]');
    await page.keyboard.press("Escape");
    disconnected = false;
    await page.setOfflineMode(false);
    console.log("[ok] cold offline reload preserves editable rows and the cached item catalog");

    await page.click(testId("device-list-storage-toggle"));
    await page.click(testId("device-list-export"));
    await page.waitForFunction(selector => document.querySelector(selector)?.value.length > 0,
      {}, testId("device-list-backup"));
    const backup = await page.$eval(testId("device-list-backup"), input => input.value);
    assert.equal(JSON.parse(backup).format, "ultros-device-list");
    await load("/list?lang=en");
    await page.$eval(testId("device-list-backup"), element => { element.closest("details").open = true; });
    await replace(testId("device-list-backup"), backup);
    await page.click(testId("device-list-restore"));
    await page.waitForFunction(original => location.pathname.startsWith("/list/device/") && location.href !== original,
      {}, deviceUrl);
    await saved();
    await waitValue(needed, 8);
    await openDetails();
    await waitValue(owned, 3);
    await waitValue(target, 125);
    console.log("[ok] portable backup restores into a distinct device list");

    // ===== Build → Shop → Build handoff (#1437), with unknown prices =====
    // The restored list needs 8 Bronze Ingots and owns 3; no prices were
    // looked up, so every trip reports the 5 remaining units as missing.
    const visible = selector => page.$eval(selector, element => !element.closest(".hidden"));
    await page.click(testId("guest-shop-mode"));
    await page.waitForSelector(testId("shop-cart-summary"), { visible: true });
    assert.match(await page.$eval(testId("shop-cart-summary"), element => element.textContent),
      /1 items · 5 units left to buy · 0 priced/, "handoff counts remaining units of a partially acquired row");
    assert.equal(await visible(testId("shop-no-prices")), true, "unknown prices are called out before a trip exists");
    await page.click(testId("shop-cheapest"));
    await page.waitForSelector(testId("shop-totals"));
    assert.match(await page.$eval(testId("shop-totals"), element => element.textContent), /0 gil · 0 surplus · 5 missing/);
    await page.$eval(testId("shop-estimate"), details => { details.open = true; });
    assert.match(await page.$eval(testId("shop-estimate"), element => element.textContent),
      /Build estimate: 0 gil \(0 of 1 items priced/, "Shop explains the whole-stack total against the Build estimate");
    assert.equal(await visible(testId("shop-drift")), false, "a fresh trip reports no drift");
    await page.click(testId("guest-build-mode"));
    await page.waitForSelector(needed, { visible: true });
    assert.equal(await visible(testId("shop-totals")), false, "Shop stays mounted but hidden in Build");
    await replace(needed, 9);
    await page.keyboard.press("Enter");
    await waitValue(needed, 9);
    await saved();
    await page.click(testId("guest-shop-mode"));
    await page.waitForSelector(testId("shop-totals"), { visible: true });
    assert.match(await page.$eval(testId("shop-totals"), element => element.textContent), /5 missing/,
      "returning to Shop keeps the chosen trip as it was planned");
    await page.waitForFunction(selector => {
      const element = document.querySelector(selector);
      return element && !element.closest(".hidden") && /1 quantity change/.test(element.textContent);
    }, {}, testId("shop-drift"));
    await page.click(testId("shop-refresh"));
    await page.waitForSelector(testId("shop-review"));
    assert.match(await page.$eval(testId("shop-review-next"), element => element.textContent), /6 missing/);
    assert.match(await page.$eval(testId("shop-totals"), element => element.textContent), /5 missing/,
      "a refresh is reviewed before it replaces the trip");
    await page.click(testId("shop-review-keep"));
    await page.waitForFunction(selector => !document.querySelector(selector), {}, testId("shop-review"));
    assert.match(await page.$eval(testId("shop-totals"), element => element.textContent), /5 missing/);
    await page.click(testId("shop-refresh"));
    await page.waitForSelector(testId("shop-review-apply"));
    await page.click(testId("shop-review-apply"));
    await page.waitForFunction(selector => /6 missing/.test(document.querySelector(selector)?.textContent), {}, testId("shop-totals"));
    await page.waitForFunction(selector => !!document.querySelector(selector)?.closest(".hidden"), {}, testId("shop-drift"));
    console.log("[ok] Build edits keep the chosen trip; a refresh is reviewed before it replaces it");

    await page.click(testId("shop-cheapest"));
    await page.waitForSelector(testId("open-shopping-companion"));
    const popupPromise = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Shopping companion did not open")), timeout);
      page.once("popup", popup => { clearTimeout(timer); resolve(popup); });
    });
    await page.click(testId("open-shopping-companion"));
    const popup = await popupPromise;
    popup.setDefaultTimeout(timeout);
    await popup.waitForSelector("main h1");
    assert.match(await popup.$eval("body", body => body.innerText), /Keep the Ultros tab open/);
    assert.match(await popup.$eval("#notice", element => element.textContent), /always.on.top|normal|regular|standard/i,
      "fallback explains that a normal browser window is not always on top");
    // Use the router link rather than a document navigation: component
    // cleanup must close the companion even when pagehide never fires.
    await page.bringToFront();
    await page.$eval('a[href="/list?labs=lists-sync"]', link => link.click());
    await page.waitForFunction(() => location.pathname === "/list");
    const closedByDeadline = await Promise.race([
      popup.isClosed() ? Promise.resolve(true) : new Promise(resolve => popup.once("close", () => resolve(true))),
      new Promise(resolve => setTimeout(() => resolve(false), 5000)),
    ]);
    assert.equal(closedByDeadline, true, "route cleanup closes the companion without disposing the device list");
    console.log("[ok] guest Shop opens the compact fallback and closes it on client-side navigation");

    assert.deepEqual(accountWrites, [], "guest changes do not call authenticated list mutation endpoints");
    assert.deepEqual(errors, [], "no uncaught browser errors during guest navigation");
  } catch (error) {
    console.error("Offline diagnostics:", await page.evaluate(async () => ({
      ready: window.__ULTROS_GUEST_OFFLINE_READY__,
      offline: window.__ULTROS_OFFLINE_GUEST__,
      resources: performance.getEntriesByType("resource").map(entry => entry.name).filter(name => /pkg\/|guest-offline|rkyv/.test(name)),
      worker: navigator.serviceWorker?.controller?.scriptURL,
      caches: await caches.keys(),
    })).catch(() => null), errors);
    const artifacts = path.join(__dirname, "artifacts", "lists-v2");
    fs.mkdirSync(artifacts, { recursive: true });
    await capture(page, { path: path.join(artifacts, "failure.png"), fullPage: true }).catch(() => {});
    console.error("Guest page:", page.url(), await page.$eval("body", body => body.innerText).catch(() => "unavailable"));
    throw error;
  } finally {
    await browser.close();
    for (const socket of sockets) socket.destroy();
    proxy.closeAllConnections();
    await new Promise(resolve => proxy.close(resolve));
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
