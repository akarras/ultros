// Build feedback acceptance against a fresh test-auth server. Real keyboard
// input and real documents; this probe does not depend on market prices.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const puppeteer = require("puppeteer");
const base = process.env.BASE_URL || "http://127.0.0.1:8080";
const user = 990000001439;
const artifactDir = path.join(__dirname, "artifacts", "list-cart-feedback");
const cart = '[data-testid="list-cart"]';
const search = 'input[aria-label="Add an item"]';
const addQuantity = 'input[aria-label="Quantity to add"]';
const needed = 'input[aria-label="Needed for Bronze Ingot"]';
const owned = 'input[aria-label="Owned for Bronze Ingot"]';
const targetPrice = 'input[aria-label="Target price for Bronze Ingot"]';

async function withCleanup(action, cleanup, message) {
  const failures = [];
  try { await action(); } catch (error) { failures.push(error); }
  try { await cleanup(); } catch (error) { failures.push(error); }
  if (failures.length === 1) throw failures[0];
  if (failures.length) throw new AggregateError(failures, message);
}

async function main() {
  fs.mkdirSync(artifactDir, { recursive: true });
  const browser = await puppeteer.launch({ headless: true });
  await withCleanup(() => run(browser), () => browser.close(), "Cart feedback failed and browser cleanup failed");
}

async function run(browser) {
  const page = await browser.newPage();
  page.setDefaultTimeout(90000);
  page.setDefaultNavigationTimeout(90000);
  const errors = [];
  const lists = [];
  const createdNames = new Set();
  async function configure(editor) {
    editor.setDefaultTimeout(90000);
    editor.setDefaultNavigationTimeout(90000);
    editor.on("pageerror", error => errors.push(String(error.stack || error)));
    await editor.setRequestInterception(true);
    editor.on("request", request => {
      const url = new URL(request.url());
      const response = url.hostname === "pagead2.googlesyndication.com" && url.pathname.endsWith("/adsbygoogle.js")
        ? request.respond({ status: 200, contentType: "application/javascript", body: "/* External ad loader disabled for deterministic app acceptance. */" })
        : request.continue();
      response.catch(error => errors.push(String(error)));
    });
    editor.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
    await editor.evaluateOnNewDocument(() => {
      window.__cartFeedbackHydrated = false;
      window.addEventListener("ultros:hydrated", () => { window.__cartFeedbackHydrated = true; });
    });
  }
  await configure(page);
  await page.setCookie(...[["LABS", "lists-sync"], ["HIDE_ADS", "true"], ["i18n_pref_locale", "en"]]
    .map(([name, value]) => ({ name, value, url: base, path: "/" })));
  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, { method, headers: { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body) });
    if (!response.ok) throw new Error(`${method} ${route}: ${response.status}`);
    const text = await response.text();
    return text ? JSON.parse(text) : null;
  }, { method, route, body });
  async function load(route) {
    await page.bringToFront();
    await page.goto(`${base}${route}`, { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__cartFeedbackHydrated);
  }
  async function replace(selector, value, editor = page) {
    await editor.bringToFront();
    await editor.focus(selector);
    await editor.keyboard.down("Control");
    await editor.keyboard.press("A");
    await editor.keyboard.up("Control");
    await editor.keyboard.press("Backspace");
    if (value) await editor.keyboard.type(value);
  }
  async function waitValue(selector, value, editor = page) {
    await editor.bringToFront();
    await editor.waitForFunction((selector, value) =>
      document.querySelector(selector)?.value === value, {}, selector, value);
  }
  async function screenshot(name) {
    await page.bringToFront();
    await page.screenshot({ path: path.join(artifactDir, `${name}.png`), fullPage: true });
  }
  async function assertInvalid(selector, committed, message = /Enter a whole number/) {
    await page.bringToFront();
    await page.waitForFunction(selector => document.querySelector(selector)?.getAttribute("aria-invalid") === "true", {}, selector);
    const state = await page.$eval(selector, input => ({
      message: document.getElementById(input.getAttribute("aria-describedby"))?.textContent,
      role: document.getElementById(input.getAttribute("aria-describedby"))?.getAttribute("role"),
      committed: input.dataset.committed,
    }));
    assert.match(state.message, message);
    assert.equal(state.role, "alert", "invalid field describes the associated announced error");
    if (committed !== undefined) assert.equal(state.committed, committed, "invalid draft never mutates the document");
  }
  async function exercise(kind, viewport) {
    const prefix = `${kind}-${viewport}`;
    await page.waitForSelector(cart);
    // The composer mounts once the document reports it is writable, which an
    // account list resolves after its access check.
    await page.waitForSelector(addQuantity);
    await replace(addQuantity, "0");
    await assertInvalid(addQuantity);
    await replace(search, "Bronze Ingot");
    await page.waitForSelector('button[aria-label="Add Bronze Ingot"]');
    assert(await page.$eval('button[aria-label="Add Bronze Ingot"]', button => button.disabled));
    await page.keyboard.press("Enter");
    assert.equal(await page.$(needed), null, "invalid keyboard add does not create a row");
    await screenshot(`${prefix}-invalid-add`);
    await replace(addQuantity, "2147483648");
    await assertInvalid(addQuantity, undefined, /number is too large/);
    await page.focus(search);
    await page.keyboard.press("Enter");
    assert.equal(await page.$(needed), null, "overflowing keyboard add does not create a row");
    await replace(addQuantity, "3");
    await page.focus(search);
    await page.keyboard.press("Enter");
    await waitValue(needed, "3");
    if (kind === "device") {
      await page.waitForFunction(selector => document.querySelector(selector)?.closest("li").classList.contains("ring-2"), {}, needed);
      await page.waitForFunction(selector => !document.querySelector(selector)?.closest("li").classList.contains("ring-2"), {}, needed);
    }
    await page.focus(search);
    await page.keyboard.press("Enter");
    await waitValue(needed, "6");
    assert.equal((await page.$$(needed)).length, 1, "duplicate item and quality increases the same row");
    if (kind === "device") {
      await page.waitForFunction(selector => document.querySelector(selector)?.closest("li").classList.contains("ring-2"), {}, needed);
      await screenshot(`${prefix}-duplicate-highlight`);
    }
    await page.keyboard.press("Escape");
    for (const invalid of ["0", "-1", "1.5", "2147483648", ""]) {
      await replace(needed, invalid);
      await page.keyboard.press("Enter");
      await assertInvalid(needed, "6", invalid === "2147483648" ? /number is too large/ : /Enter a whole number/);
      await page.focus(needed);
      await page.keyboard.press("Escape");
      await waitValue(needed, "6");
      assert.equal(await page.$eval(needed, input => input.getAttribute("aria-invalid")), "false");
    }
    await replace(needed, "1.5");
    await page.keyboard.press("Tab");
    await assertInvalid(needed, "6");
    await screenshot(`${prefix}-invalid-row`);
    await replace(needed, "7");
    await page.keyboard.press("Enter");
    await page.waitForFunction(selector => document.querySelector(selector)?.dataset.committed === "7", {}, needed);
    // A real second editor updates the same field while this editor displays
    // an invalid draft. The incoming committed value replaces it and clears
    // validation, including when the invalid input still has focus.
    const peer = await browser.newPage();
    await withCleanup(async () => {
      await configure(peer);
      await peer.bringToFront();
      await peer.goto(page.url(), { waitUntil: "domcontentloaded" });
      await peer.waitForFunction(() => window.__cartFeedbackHydrated);
      await peer.waitForFunction(selector => document.querySelector(selector)?.dataset.committed === "7", {}, needed);
      await replace(needed, "1.5");
      await page.keyboard.press("Enter");
      await assertInvalid(needed, "7");
      await page.focus(needed);
      await replace(needed, "13", peer);
      await peer.keyboard.press("Enter");
      await waitValue(needed, "13");
      await page.waitForFunction(selector => {
        const input = document.querySelector(selector);
        return input?.dataset.committed === "13" && input.getAttribute("aria-invalid") === "false" && !input.hasAttribute("aria-describedby");
      }, {}, needed);
      await screenshot(`${prefix}-remote-correction`);
      await replace(needed, "7", peer);
      await peer.keyboard.press("Enter");
      await waitValue(needed, "7");
    }, () => peer.close(), "Second-editor feedback failed and editor cleanup failed");
    // The number itself must fit, not merely the outer cart. Reserve the
    // browser number spinner's width and measure four ordinary digits.
    await replace(needed, "1234");
    await page.keyboard.press("Enter");
    await page.waitForFunction(selector => document.querySelector(selector)?.dataset.committed === "1234", {}, needed);
    const numericGeometry = await page.$eval(needed, input => {
      const style = getComputedStyle(input);
      const context = document.createElement("canvas").getContext("2d");
      context.font = `${style.fontSize} ${style.fontFamily}`;
      return {
        width: input.getBoundingClientRect().width,
        available: input.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) - 18,
        textWidth: context.measureText(input.value).width,
        pageFits: document.documentElement.scrollWidth <= innerWidth + 1,
      };
    });
    assert(numericGeometry.available >= numericGeometry.textWidth, `ordinary quantity must fit: ${JSON.stringify(numericGeometry)}`);
    assert(numericGeometry.pageFits, "four-digit quantity does not overflow the viewport");
    await page.$eval(needed, input => input.scrollIntoView({ block: "center", behavior: "instant" }));
    await screenshot(`${prefix}-four-digit-quantity`);
    await replace(needed, "7");
    await page.keyboard.press("Enter");
    await page.waitForFunction(selector => document.querySelector(selector)?.dataset.committed === "7", {}, needed);
    await page.click('button[aria-label="Details for Bronze Ingot"]');
    await replace(owned, "-1");
    await page.keyboard.press("Enter");
    await assertInvalid(owned, "0");
    await replace(owned, "2147483648");
    await page.keyboard.press("Enter");
    await assertInvalid(owned, "0", /number is too large/);
    await replace(owned, "0");
    await page.keyboard.press("Enter");
    await replace(targetPrice, "9223372036854775808");
    await page.keyboard.press("Enter");
    await assertInvalid(targetPrice, "", /number is too large/);
    await replace(targetPrice, "12");
    await page.keyboard.press("Enter");
    await page.waitForFunction(selector => document.querySelector(selector)?.dataset.committed === "12", {}, targetPrice);
    await replace(targetPrice, "");
    await page.keyboard.press("Enter");
    await page.waitForFunction(selector => document.querySelector(selector)?.dataset.committed === "" && document.querySelector(selector)?.getAttribute("aria-invalid") === "false", {}, targetPrice);
    const filter = `${cart} input[type="search"]`;
    await replace(filter, "no-match-feedback-fixture");
    await page.waitForSelector('[data-testid="cart-no-matches"]');
    assert.equal((await page.$$('[data-testid="cart-rows"] > li')).length, 0);
    await screenshot(`${prefix}-no-matches`);
    await page.focus('[data-testid="cart-no-matches"] button');
    await page.keyboard.press("Enter");
    await waitValue(needed, "7");
    await page.waitForSelector('[data-testid="cart-no-matches"]', { hidden: true });
    assert.equal(await page.$eval(filter, input => input.value), "");
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "feedback does not overflow the viewport");
    if (kind === "account") {
      // Clearing filters must also undo the host's hide-acquired URL filter.
      await replace(owned, "7");
      await page.keyboard.press("Enter");
      await page.waitForFunction(selector => document.querySelector(selector)?.dataset.committed === "7", {}, owned);
      await load(`${new URL(page.url()).pathname}?hide-acquired=true`);
      await page.waitForSelector('[data-testid="cart-no-matches"]');
      await page.click('[data-testid="cart-no-matches"] button');
      await waitValue(needed, "7");
      assert(!new URL(page.url()).searchParams.has("hide-acquired"));
    }
    console.log(`PASS: ${prefix} numeric validation, correction, duplicate add, keyboard filter recovery`);
  }
  await withCleanup(async () => {
    for (const [viewport, width, height] of [["desktop", 1280, 900], ["mobile", 390, 844]]) {
      await page.setViewport({ width, height });
      await load("/list");
      await page.click('[data-testid="list-new"]');
      await replace('[data-testid="device-list-name"]', `Cart feedback ${viewport} ${Date.now()}`);
      await page.click('[data-testid="device-list-create"]');
      await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
      await exercise("device", viewport);
    }
    await load(`/test/login?user_id=${user}&username=CartFeedbackQA&redirect=/list`);
    const worlds = await api("GET", "/api/v1/world_data");
    const world = worlds.regions[0].datacenters[0].worlds[0];
    for (const [viewport, width, height] of [["desktop", 1280, 900], ["mobile", 390, 844]]) {
      await page.setViewport({ width, height });
      const name = `Cart feedback ${viewport} ${Date.now()}`;
      createdNames.add(name);
      await api("POST", "/api/v1/list/create", { name, wdr_filter: { World: world.id } });
      const id = (await api("GET", "/api/v1/list")).find(entry => entry.list.name === name).list.id;
      lists.push(id);
      await load(`/list/${id}`);
      await exercise("account", viewport);
    }
  }, async () => {
    const failures = [];
    // Inventory also finds a successful create whose response or subsequent
    // lookup failed. Only this run's exact names and recorded IDs are owned.
    if (createdNames.size) {
      try {
        const inventory = await api("GET", "/api/v1/list");
        for (const entry of inventory) {
          if (createdNames.has(entry.list.name) && !lists.includes(entry.list.id)) lists.push(entry.list.id);
        }
      } catch (error) { failures.push(new Error("Failed to inventory cart feedback fixtures", { cause: error })); }
    }
    const cleanup = await Promise.allSettled(lists.map(id => api("DELETE", `/api/v1/list/${id}/delete`)));
    failures.push(...cleanup.flatMap((result, index) => result.status === "rejected"
      ? [new Error(`Failed to delete fixture list ${lists[index]}`, { cause: result.reason })] : []));
    if (createdNames.size) {
      try {
        const inventory = await api("GET", "/api/v1/list");
        assert(!inventory.some(entry => lists.includes(entry.list.id) || createdNames.has(entry.list.name)), "cart feedback fixtures are absent after cleanup");
      } catch (error) { failures.push(new Error("Failed to verify cart feedback fixture cleanup", { cause: error })); }
    }
    try { assert.deepEqual(errors, [], "application and request errors, including cleanup"); }
    catch (error) { failures.push(error); }
    if (failures.length) throw new AggregateError(failures, "Cart feedback fixture cleanup failed");
  }, "Cart feedback assertions and cleanup failed");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
