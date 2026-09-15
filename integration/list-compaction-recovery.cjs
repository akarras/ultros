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

async function saved(page) {
  await page.waitForFunction(() => document.querySelector('[data-testid="account-list-save-state"]')?.textContent.includes("Saved on this device"), { timeout: 60000 });
}

async function open(context, id, errors) {
  const page = await context.newPage();
  await page.setCacheEnabled(false);
  page.on("pageerror", error => errors.push(String(error.stack || error)));
  page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
  await page.goto(`${base}/list/${id}`, { waitUntil: "domcontentloaded", timeout: 60000 });
  await page.waitForSelector(needed, { timeout: 60000 });
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
      await page.setCacheEnabled(false);
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
    await a.setOfflineMode(true);
    await edit(a, needed, 15);
    await edit(a, owned, 1);
    const before = (await api(control, "GET", `/api/v1/list/${id}/listings`))[1][0][0];
    assert.equal(before.quantity, 10, "A must really be offline");
    assert.equal(before.acquired, 0);
    await a.close(); // Recovery must come from the durable snapshot, not a live handle.
    await edit(b, owned, 4);
    await serverRow(control, id, { quantity: 10, acquired: 4 });
    await api(control, "POST", `/test/list/${id}/compact`);
    a = await open(contexts[0], id, errors);
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
    await a.setOfflineMode(true);
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
    await a.click('[data-testid="list-recovery-retry"]');
    await a.waitForFunction(() => !document.querySelector('[data-testid="list-recovery-retry"]'));
    await serverRow(control, conflictId, { quantity: 20 });
    console.log("PASS: conflict review preserves both copies and edited retry resolves it");

    await a.setOfflineMode(true);
    await edit(a, needed, 25);
    await a.close();
    await edit(remote, needed, 30);
    await serverRow(control, conflictId, { quantity: 30 });
    await api(control, "POST", `/test/list/${conflictId}/compact`);
    a = await open(contexts[0], conflictId, errors);
    await a.waitForSelector('[data-testid="list-recovery-use-server"]', { timeout: 60000 });
    await a.click('[data-testid="list-recovery-use-server"]');
    await a.waitForFunction(selector => document.querySelector(selector)?.value === "30", {}, needed);
    await saved(a);
    await a.reload({ waitUntil: "domcontentloaded" });
    await a.waitForFunction(selector => document.querySelector(selector)?.value === "30", { timeout: 60000 }, needed);
    await serverRow(control, conflictId, { quantity: 30 });
    console.log("PASS: explicit server choice survives reload without replaying discarded edits");
    assert.deepEqual(errors, [], "browser application errors");
  } finally {
    for (const id of lists) await api(control, "DELETE", `/api/v1/list/${id}/delete`).catch(console.error);
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
