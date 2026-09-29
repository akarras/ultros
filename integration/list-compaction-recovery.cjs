// Real browser peers, durable reload and the production server compaction path.
// Requires a fresh test-auth build. All created lists belong to this fixture.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const puppeteer = require("puppeteer");
const base = process.env.BASE_URL || "http://127.0.0.1:8080";
const user = 990000000812;
const needed = 'input[aria-label^="Needed for "]';
const owned = 'input[aria-label^="Owned for "]';
const artifacts = path.join(__dirname, "artifacts", "list-compaction-recovery");

async function api(page, method, route, body) {
  return page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, { method, headers: { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
    if (!response.ok) throw new Error(`${method} ${route}: ${response.status}`);
    const text = await response.text();
    return text ? JSON.parse(text) : null;
  }, { method, route, body });
}

async function prepare(page) {
  page.setDefaultNavigationTimeout(60000);
  page.compactionRequests = new Map();
  page.on("request", request => page.compactionRequests.set(request, `${request.resourceType()} ${request.url()}`));
  page.on("requestfinished", request => page.compactionRequests.delete(request));
  page.on("requestfailed", request => page.compactionRequests.delete(request));
  await page.setCacheEnabled(false);
  await page.evaluateOnNewDocument(() => {
    const NativeSocket = window.WebSocket;
    window.compactionSockets = new Set();
    window.WebSocket = class extends NativeSocket {
      constructor(...args) {
        super(...args);
        window.compactionSockets.add(this);
        this.addEventListener("close", () => window.compactionSockets.delete(this));
      }
    };
  });
  // Third-party ad frames can keep navigation pending and do not exercise
  // list recovery. Keep all first-party requests and their errors intact.
  await page.setRequestInterception(true);
  page.on("request", request => {
    const host = new URL(request.url()).hostname;
    if (/(^|\.)(googlesyndication\.com|doubleclick\.net|googleadservices\.com)$/.test(host)) request.abort();
    else request.continue();
  });
}

async function disconnect(page) {
  // Chromium's offline emulation need not close already-open WebSockets.
  // Finish the close handshake while online: switching offline first can
  // leave it waiting forever. Then disable reconnects before editing.
  await page.evaluate(async () => {
    await Promise.all([...window.compactionSockets].map(socket => new Promise(resolve => {
      if (socket.readyState === WebSocket.CLOSED) return resolve();
      socket.addEventListener("close", resolve, { once: true });
      socket.close();
    })));
  });
  await page.setOfflineMode(true);
}

async function saved(page) {
  await page.waitForFunction(() => document.querySelector('[data-testid="account-list-save-state"]')?.textContent.includes("Saved on this device"), { timeout: 60000 });
}

async function open(context, id, errors, beforeSaved) {
  const page = await context.newPage();
  await prepare(page);
  page.on("pageerror", error => { if (!String(error.stack).includes("pagead2.googlesyndication.com")) errors.push(String(error.stack || error)); });
  page.on("console", message => { if (["warn", "error"].includes(message.type()) && message.text().includes("list ")) console.log(`Browser ${message.type()}: ${message.text()}`); });
  page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
  await page.goto(`${base}/list/${id}`, { waitUntil: "domcontentloaded", timeout: 60000 });
  await page.waitForSelector(needed, { timeout: 60000 });
  if (beforeSaved) await beforeSaved(page);
  await saved(page);
  return page;
}

async function edit(page, selector, value) {
  if (selector === owned) {
    await page.evaluate(() => document.querySelector('button[aria-label^="Details for "][aria-expanded="false"]')?.click());
  }
  await page.waitForSelector(selector);
  await page.evaluate(({ selector, value }) => {
    const input = document.querySelector(selector);
    input.value = String(value);
    input.dispatchEvent(new Event("change", { bubbles: true }));
  }, { selector, value });
  await saved(page);
}

async function serverRow(page, id, expected) {
  const deadline = Date.now() + 60000;
  let row;
  do {
    row = (await api(page, "GET", `/api/v1/list/${id}/listings`))[1][0][0];
    if (Object.entries(expected).every(([key, value]) => row[key] === value)) return row;
    await new Promise(resolve => setTimeout(resolve, 150));
  } while (Date.now() < deadline);
  assert.fail(`Server did not reach ${JSON.stringify(expected)}: ${JSON.stringify(row)}`);
}

async function main() {
  fs.mkdirSync(artifacts, { recursive: true });
  const browser = await puppeteer.launch({ headless: true });
  const errors = [];
  const lists = [];
  let control;
  try {
    const contexts = await Promise.all([browser.createBrowserContext(), browser.createBrowserContext()]);
    const controls = [];
    for (const context of contexts) {
      const page = await context.newPage();
      await prepare(page);
      const response = await page.goto(`${base}/test/login?user_id=${user}&username=CompactionRecoveryQA&redirect=/list`);
      assert(response.ok(), "test-auth must be enabled");
      await page.setCookie({ name: "LABS", value: "lists-sync", url: base, path: "/" });
      controls.push(page);
    }
    control = controls[1];
    const worlds = await api(control, "GET", "/api/v1/world_data");
    async function fixture(label) {
      const name = `Compaction ${label} ${Date.now()}`;
      await api(control, "POST", "/api/v1/list/create", { name, wdr_filter: { World: worlds.regions[0].datacenters[0].worlds[0].id } });
      const id = (await api(control, "GET", "/api/v1/list")).find(entry => entry.list.name === name).list.id;
      lists.push(id);
      await api(control, "POST", `/api/v1/list/${id}/add/item`, { id: 0, item_id: 5, list_id: id, hq: null, quantity: 10, acquired: 0 });
      return id;
    }

    const id = await fixture("disjoint");
    let a = await open(contexts[0], id, errors);
    const b = await open(contexts[1], id, errors);
    await disconnect(a);
    await edit(a, needed, 15);
    await edit(a, owned, 1);
    const before = (await api(control, "GET", `/api/v1/list/${id}/listings`))[1][0][0];
    assert.equal(before.quantity, 10, "A must really be offline");
    assert.equal(before.acquired, 0);
    await a.close(); // Recovery must come from the durable snapshot, not a live handle.
    await edit(b, owned, 4);
    await serverRow(control, id, { quantity: 10, acquired: 4 });
    await api(control, "POST", `/test/list/${id}/compact`);
    await controls[0].evaluate(user => {
      navigator.locks.request(`ultros-account-lists:${user}`, () => new Promise(resolve => { window.releaseCompactionLock = resolve; }));
    }, user);
    await controls[0].waitForFunction(() => typeof window.releaseCompactionLock === "function");
    a = await open(contexts[0], id, errors, async page => {
      await page.waitForFunction(() => document.querySelector('[data-testid="list-recovery"]')?.textContent.includes("Undo and Redo history has been reset"));
      await serverRow(control, id, { quantity: 10, acquired: 4 });
      await controls[0].evaluate(() => window.releaseCompactionLock());
    });
    await serverRow(control, id, { quantity: 15, acquired: 5 });
    await a.waitForFunction(() => document.querySelector('[data-testid="list-recovery"]')?.textContent.includes("Undo and Redo history has been reset"));
    await b.waitForFunction(selector => document.querySelector(selector)?.value === "15", {}, needed);
    await b.waitForFunction(selector => document.querySelector(selector)?.value === "5", {}, owned);
    await saved(a);
    await a.reload({ waitUntil: "domcontentloaded" });
    await a.waitForFunction(selector => document.querySelector(selector)?.value === "15", { timeout: 60000 }, needed);
    await serverRow(control, id, { quantity: 15, acquired: 5 });
    console.log("PASS: compaction + reload preserve remote purchases and replay offline delta once");
    await a.close();
    await b.close();

    const conflictId = await fixture("conflict");
    a = await open(contexts[0], conflictId, errors);
    const remote = await open(contexts[1], conflictId, errors);
    await disconnect(a);
    await edit(a, needed, 15);
    await a.close();
    await edit(remote, needed, 20);
    await serverRow(control, conflictId, { quantity: 20 });
    await api(control, "POST", `/test/list/${conflictId}/compact`);
    a = await open(contexts[0], conflictId, errors);
    await a.waitForSelector('[data-testid="list-recovery-retry"]', { timeout: 60000 });
    const review = await a.$eval('[data-testid="list-recovery"]', el => el.textContent);
    assert.match(review, /15 needed/);
    assert.match(review, /20 needed/);
    assert.equal(await a.$eval(needed, el => el.value), "15", "conflict preserves local values");
    await serverRow(control, conflictId, { quantity: 20 });
    await a.setViewport({ width: 1280, height: 900 });
    await a.screenshot({ path: path.join(artifacts, "review-desktop.png"), fullPage: true });
    await a.setViewport({ width: 390, height: 844 });
    await a.screenshot({ path: path.join(artifacts, "review-mobile.png"), fullPage: true });
    assert(await a.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "review must not overflow mobile viewport");
    await edit(a, needed, 20);
    await a.waitForFunction(() => !document.querySelector('[data-testid="list-recovery"] tbody')?.textContent.includes("15 needed"));
    await saved(a);
    await a.$eval('[data-testid="list-recovery-retry"]', button => button.scrollIntoView({ block: "center" }));
    await a.locator('[data-testid="list-recovery-retry"]').click();
    await a.waitForFunction(() => !document.querySelector('[data-testid="list-recovery-retry"]')).catch(async error => {
      console.error("Conflict retry state", await a.evaluate(selector => ({
        value: document.querySelector(selector)?.value,
        review: document.querySelector('[data-testid="list-recovery"]')?.textContent,
        save: document.querySelector('[data-testid="account-list-save-state"]')?.textContent,
      }), needed));
      console.error("Application errors", errors);
      throw error;
    });
    await serverRow(control, conflictId, { quantity: 20 });
    console.log("PASS: conflict review preserves both copies and edited retry resolves it");

    await disconnect(a);
    await edit(a, needed, 25);
    await a.close();
    await edit(remote, needed, 30);
    await serverRow(control, conflictId, { quantity: 30 });
    await api(control, "POST", `/test/list/${conflictId}/compact`);
    a = await open(contexts[0], conflictId, errors);
    await a.waitForSelector('[data-testid="list-recovery-use-server"]', { timeout: 60000 });
    await a.locator('[data-testid="list-recovery-use-server"]').click();
    await a.waitForFunction(selector => document.querySelector(selector)?.value === "30", {}, needed);
    await saved(a);
    await a.reload({ waitUntil: "domcontentloaded" });
    await a.waitForFunction(selector => document.querySelector(selector)?.value === "30", { timeout: 60000 }, needed);
    await serverRow(control, conflictId, { quantity: 30 });
    console.log("PASS: explicit server choice survives reload without replaying discarded edits");
    await a.close();
    await remote.close();

    const quotaId = await fixture("quota");
    a = await open(contexts[0], quotaId, errors);
    const quotaPeer = await open(contexts[1], quotaId, errors);
    await disconnect(a);
    await edit(a, owned, 1);
    await a.close();
    await edit(quotaPeer, owned, 4);
    await serverRow(control, quotaId, { acquired: 4 });
    await api(control, "POST", `/test/list/${quotaId}/compact`);
    const denied = await contexts[0].newPage();
    await prepare(denied);
    denied.on("pageerror", error => { if (!String(error.stack).includes("pagead2.googlesyndication.com")) errors.push(String(error.stack || error)); });
    denied.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
    await denied.evaluateOnNewDocument(() => {
      const original = Storage.prototype.setItem;
      Storage.prototype.setItem = function(key, value) {
        if (String(key).startsWith("ultros.listdoc.v1.")) throw new DOMException("Recovery quota fixture", "QuotaExceededError");
        return original.call(this, key, value);
      };
    });
    await denied.goto(`${base}/list/${quotaId}`, { waitUntil: "domcontentloaded", timeout: 60000 });
    await denied.waitForFunction(() =>
      document.querySelector('[data-testid="list-recovery"]')?.textContent.includes("Undo and Redo history has been reset") &&
      document.querySelector('[data-testid="account-list-save-state"]')?.textContent.includes("not saved on this device"),
      { timeout: 60000 });
    await serverRow(control, quotaId, { acquired: 4 });
    await denied.evaluate(() => {
      window.quotaNavigationMarker = "kept";
      [...document.querySelectorAll("a")].find(link => link.getAttribute("href") === "/list").click();
    });
    await denied.waitForFunction(() => location.pathname === "/list");
    await denied.evaluate(id => {
      const link = document.createElement("a");
      link.href = `/list/${id}`;
      document.body.appendChild(link);
      link.click();
      link.remove();
    }, quotaId);
    await denied.waitForFunction(id => location.pathname === `/list/${id}`, {}, quotaId);
    assert.equal(await denied.evaluate(() => window.quotaNavigationMarker), "kept", "must retain the same runtime across SPA navigation");
    await denied.waitForSelector(needed, { timeout: 60000 });
    await denied.waitForFunction(() => document.querySelector('[data-testid="account-list-save-state"]')?.textContent.includes("not saved on this device"));
    await denied.evaluate(() => document.querySelector('button[aria-label^="Details for "][aria-expanded="false"]')?.click());
    await denied.waitForFunction(selector => document.querySelector(selector)?.value === "5", {}, owned);
    await serverRow(control, quotaId, { acquired: 4 });
    await denied.close(); // Lose the in-memory replacement; retain the older disk copy.
    a = await open(contexts[0], quotaId, errors);
    await serverRow(control, quotaId, { acquired: 5 });
    await saved(a);
    await a.reload({ waitUntil: "domcontentloaded" });
    await a.waitForSelector(needed, { timeout: 60000 });
    await serverRow(control, quotaId, { acquired: 5 });
    console.log("PASS: failed replacement save cannot duplicate offline purchases after reload");
    assert.deepEqual(errors, [], "browser application errors");
  } catch (error) {
    for (const page of await browser.pages()) {
      if (!page.url().startsWith(`${base}/list/`)) continue;
      console.error("Failure page", page.url(), [...(page.compactionRequests?.values() || [])]);
      console.error(await page.evaluate(() => ({ ready: document.readyState, title: document.title, bodyLength: document.body?.textContent.length, recovery: document.querySelector('[data-testid="list-recovery"]')?.textContent, save: document.querySelector('[data-testid="account-list-save-state"]')?.textContent })).catch(String));
    }
    throw error;
  } finally {
    for (const id of lists) await api(control, "DELETE", `/api/v1/list/${id}/delete`).catch(console.error);
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
