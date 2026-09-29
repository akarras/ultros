// Fresh test-auth server required. Real Loro fixture bytes, hydrated restore,
// IndexedDB and account storage; every account list is created/deleted here.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const puppeteer = require("puppeteer");
const fixtures = require("./fixtures/list-compatibility.json");
const base = new URL(process.env.BASE_URL || "http://127.0.0.1:8080").origin;
const artifacts = path.join(__dirname, "artifacts", "list-document-compatibility");
const user = 990000000877;
const testId = id => `[data-testid="${id}"]`;

async function api(page, method, route, body) {
  return page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, { method, headers: { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
    if (!response.ok) throw new Error(`${method} ${route}: ${response.status}`);
    const text = await response.text();
    return text ? JSON.parse(text) : null;
  }, { method, route, body });
}

async function records(page) {
  return page.evaluate(() => new Promise((resolve, reject) => {
    const request = indexedDB.open("ultros-device-lists-v1");
    request.onerror = () => reject(request.error);
    request.onsuccess = () => {
      const db = request.result;
      const transaction = db.transaction("lists", "readonly");
      const get = transaction.objectStore("lists").getAll();
      get.onsuccess = () => resolve(get.result.map(record => ({ ...record, snapshot: [...record.snapshot] })));
      transaction.oncomplete = () => db.close();
    };
  }));
}

async function prepare(page, errors) {
  await page.setViewport({ width: 1280, height: 900 });
  page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
  page.setDefaultTimeout(60000);
  page.setDefaultNavigationTimeout(60000);
  page.on("pageerror", error => { if (!String(error.stack).includes("googlesyndication")) errors.push(String(error.stack || error)); });
  page.on("console", message => { if (["warn", "error"].includes(message.type()) && /list snapshot|list schema|invalid list document/.test(message.text())) console.log(`Browser ${message.type()}: ${message.text()}`); });
  await page.setCookie({ name: "LABS", value: "lists-sync", url: base }, { name: "HIDE_ADS", value: "true", url: base });
  await page.setRequestInterception(true);
  page.on("request", request => /(^|\.)(googlesyndication\.com|doubleclick\.net|googleadservices\.com)$/.test(new URL(request.url()).hostname) ? request.abort() : request.continue());
  await page.evaluateOnNewDocument(() => {
    window.compatibilityHydrated = false;
    window.addEventListener("ultros:hydrated", () => { window.compatibilityHydrated = true; });
    const create = URL.createObjectURL;
    URL.createObjectURL = function (blob) { window.compatibilityExport = blob; return create.call(this, blob); };
    document.addEventListener("click", event => {
      if (event.target.closest?.('a[download="ultros-list-recovery.json"]')) event.preventDefault();
    }, true);
    const send = WebSocket.prototype.send;
    window.compatibilityUpdates = [];
    WebSocket.prototype.send = function (data) {
      if (typeof data === "string" && data.includes('"ListDocUpdate"')) window.compatibilityUpdates.push(data);
      return send.call(this, data);
    };
  });
}

async function restore(page, bytes) {
  const backup = JSON.stringify({ format: "ultros-device-list", version: 1, name: "Compatibility fixture", snapshot: Buffer.from(bytes).toString("base64") });
  await page.waitForSelector(testId("list-restore-open"));
  await page.click(testId("list-restore-open"));
  await page.waitForSelector(testId("device-list-backup"), { visible: true });
  await page.$eval(testId("device-list-backup"), (input, text) => {
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }, backup);
  await page.click(testId("device-list-restore"));
  return backup;
}

async function main() {
  fs.mkdirSync(artifacts, { recursive: true });
  const browser = await puppeteer.launch({ headless: true });
  const errors = [];
  let control, listId, testError;
  try {
    const guest = await browser.newPage();
    await prepare(guest, errors);
    await guest.goto(`${base}/list?lang=en`, { waitUntil: "domcontentloaded" });
    await restore(guest, fixtures.supported);
    await guest.waitForSelector('input[aria-label^="Needed for "]');
    await guest.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    assert.equal(await guest.$eval('input[aria-label^="Needed for "]', input => input.value), "5");
    const deviceUrl = guest.url();
    const before = await records(guest);
    for (const kind of ["future", "malformed"]) {
      await guest.goto(`${base}/list?lang=en`, { waitUntil: "domcontentloaded" });
      const backup = await restore(guest, fixtures[kind]);
      await guest.waitForFunction(() => document.body.textContent.includes("cannot safely read this list"));
      assert.equal(new URL(guest.url()).pathname, "/list");
      assert.equal(await guest.$eval(testId("device-list-backup"), input => input.value), backup);
      assert.deepEqual(await records(guest), before, `${kind} restore must leave every device record unchanged`);
    }
    await guest.screenshot({ path: path.join(artifacts, "guest-restore-rejected.png"), fullPage: true });
    await guest.goto(deviceUrl, { waitUntil: "domcontentloaded" });
    await guest.waitForSelector('input[aria-label^="Needed for "]');
    assert.equal(await guest.$eval('input[aria-label^="Needed for "]', input => input.value), "5");
    console.log("PASS supported restore and future/malformed backup rejection without record changes");

    // Incognito Cache Storage cannot hold the large debug WASM. This browser
    // already has a disposable regular profile; guest checks are complete.
    await guest.close();
    const context = browser.defaultBrowserContext();
    control = await context.newPage();
    await prepare(control, errors);
    const response = await control.goto(`${base}/test/login?user_id=${user}&username=SchemaCompatibilityQA&redirect=/list`, { waitUntil: "domcontentloaded" });
    assert(response.ok(), "fresh test-auth build required");
    await control.waitForFunction(() => window.compatibilityHydrated);
    const worlds = await api(control, "GET", "/api/v1/world_data");
    const name = `Schema compatibility ${Date.now()}`;
    await api(control, "POST", "/api/v1/list/create", { name, wdr_filter: { World: worlds.regions[0].datacenters[0].worlds[0].id } });
    listId = (await api(control, "GET", "/api/v1/list")).find(entry => entry.list.name === name).list.id;
    await api(control, "POST", `/api/v1/list/${listId}/add/item`, { id: 0, item_id: 5056, list_id: listId, hq: null, quantity: 5, acquired: 0 });
    const page = await context.newPage();
    await prepare(page, errors);
    const key = `ultros.listdoc.v1.${user}.${listId}`;
    const projection = result => [result[0], result[1].map(row => row[0])];
    const serverBefore = projection(await api(control, "GET", `/api/v1/list/${listId}/listings`));
    for (const kind of ["future", "malformed", "damaged"]) {
      const original = Buffer.from(kind === "damaged" ? "not a Loro document" : fixtures[kind]).toString("base64");
      await control.evaluate(({ key, original }) => localStorage.setItem(key, original), { key, original });
      await page.goto(`${base}/list/${listId}?lang=en`, { waitUntil: "domcontentloaded" });
      await page.waitForFunction(() => window.compatibilityHydrated);
      await page.waitForSelector(testId("list-compatibility-export"));
      await page.evaluate(() => { window.compatibilityExport = undefined; });
      await page.$eval(testId("list-compatibility-export"), button => button.scrollIntoView({ block: "center" }));
      await page.locator(testId("list-compatibility-export")).click();
      await page.waitForFunction(() => window.compatibilityExport instanceof Blob);
      const backup = await page.evaluate(async () => JSON.parse(await window.compatibilityExport.text()));
      assert.equal(backup.snapshot, original, `${kind}: export must contain the original bytes`);
      // Allow the save debounce and handshake callbacks to run before checking.
      await new Promise(resolve => setTimeout(resolve, 900));
      assert.equal(await page.$eval(testId("realtime-status-indicator"), badge => badge.dataset.status), "offline", "incompatible data must not appear to be connecting or live");
      assert.equal(await page.evaluate(key => localStorage.getItem(key), key), original);
      assert.deepEqual(await page.evaluate(() => window.compatibilityUpdates), []);
      assert.deepEqual(projection(await api(control, "GET", `/api/v1/list/${listId}/listings`)), serverBefore);
      await page.reload({ waitUntil: "domcontentloaded" });
      await page.waitForSelector(testId("list-compatibility-export"));
      assert.equal(await page.$eval(testId("realtime-status-indicator"), badge => badge.dataset.status), "offline");
      assert.equal(await page.evaluate(key => localStorage.getItem(key), key), original);
      console.log(`PASS ${kind} account open: recovery export, reload, unchanged cache and server projection`);
    }
    await page.screenshot({ path: path.join(artifacts, "account-recovery-desktop.png"), fullPage: true });
    await page.setViewport({ width: 390, height: 844 });
    await page.screenshot({ path: path.join(artifacts, "account-recovery-mobile.png"), fullPage: true });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
    assert.deepEqual(errors, [], "no browser panics");
  } catch (error) {
    for (const [index, page] of (await browser.pages()).entries()) {
      if (!page.url().startsWith(base)) continue;
      console.error("Failure page", page.url(), await page.evaluate(() => ({ body: document.body.innerText.slice(-6000), recovery: document.querySelector('[data-testid="list-recovery"]')?.textContent, save: document.querySelector('[data-testid="account-list-save-state"]')?.textContent })).catch(String));
      await Promise.race([page.screenshot({ path: path.join(artifacts, `failure-${index}.png`), fullPage: true }), new Promise((_, reject) => setTimeout(() => reject(new Error("Diagnostic screenshot timed out")), 10000))]).catch(() => {});
    }
    console.error("Browser errors", errors);
    testError = error;
  } finally {
    const cleanup = await Promise.allSettled(control && listId ? [api(control, "DELETE", `/api/v1/list/${listId}/delete`)] : []);
    cleanup.push(...await Promise.allSettled([browser.close()]));
    const failures = [testError, ...cleanup.filter(result => result.status === "rejected").map(result => result.reason)].filter(Boolean);
    if (failures.length) throw new AggregateError(failures, "Compatibility validation or owned fixture cleanup failed");
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
