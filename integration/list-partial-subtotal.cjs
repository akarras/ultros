// Real account editor and lookup lifecycle with deterministic available stock.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const puppeteer = require("puppeteer");
const base = new URL(process.env.BASE_URL || "http://127.0.0.1:8080").origin;

async function main() {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  page.setDefaultTimeout(60000);
  page.setDefaultNavigationTimeout(60000);
  await page.setCacheEnabled(false);
  await page.setViewport({ width: 1280, height: 900 });
  const errors = [];
  page.on("pageerror", error => {
    if (!String(error.stack).includes("pagead2.googlesyndication.com")) errors.push(String(error.stack || error));
  });
  page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
  let listId;
  let stock = 2;
  let lookups = 0;
  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, { method, headers: { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
    if (!response.ok) throw new Error(`${method} ${route}: ${response.status}`);
    const text = await response.text();
    return text ? JSON.parse(text) : null;
  }, { method, route, body });
  const state = () => page.evaluate(() => ({
    total: document.querySelector('[data-testid="list-estimate-total"]')?.textContent,
    incomplete: document.querySelector('[data-testid="list-estimate-total"]')?.dataset.incomplete,
    status: document.querySelector('[data-testid="list-estimate-status"]')?.textContent,
    row: document.querySelector('[data-testid="cart-line-estimate"]')?.textContent,
  }));
  async function expectSummary(total, incomplete, status) {
    await page.waitForFunction(({ total, incomplete, status }) => {
      const amount = document.querySelector('[data-testid="list-estimate-total"]');
      return amount?.textContent === total && amount.dataset.incomplete === String(incomplete) &&
        document.querySelector('[data-testid="list-estimate-status"]')?.textContent.includes(status);
    }, {}, { total, incomplete, status });
  }
  try {
    const login = await page.goto(`${base}/test/login?user_id=990000000814&username=PartialSubtotalQA&redirect=/list`, { waitUntil: "domcontentloaded" });
    assert(login.ok(), "test-auth must be enabled");
    const worlds = await api("GET", "/api/v1/world_data");
    const world = worlds.regions[0].datacenters[0].worlds[0];
    await page.setCookie(...[
      ["LABS", "lists-sync"], ["HIDE_ADS", "true"], ["HOME_WORLD", world.name], ["i18n_pref_locale", "en"],
    ].map(([name, value]) => ({ name, value, url: base, path: "/" })));
    const name = `Partial subtotal ${Date.now()}`;
    await api("POST", "/api/v1/list/create", { name, wdr_filter: { World: world.id } });
    listId = (await api("GET", "/api/v1/list")).find(entry => entry.list.name === name).list.id;
    await api("POST", `/api/v1/list/${listId}/add/item`, { id: 0, list_id: listId, item_id: 5056, hq: null, quantity: 5, acquired: 0 });
    const cookie = (await page.cookies()).map(entry => `${entry.name}=${entry.value}`).join("; ");
    await page.setRequestInterception(true);
    page.on("request", async request => {
      const url = new URL(request.url());
      if (url.origin === base && url.pathname === `/api/v1/list/${listId}/listings` && request.method() === "GET") {
        try {
          const response = await fetch(url, { headers: { cookie } });
          assert(response.ok, `fixture row fetch: ${response.status}`);
          const body = await response.json();
          const offers = stock ? [{ id: -1470001, world_id: world.id, item_id: 5056, retainer_id: 1, price_per_unit: 10, quantity: stock, hq: false, timestamp: "2026-09-15T00:00:00" }] : [];
          body[1] = body[1].map(([row]) => [row, row.item_id === 5056 ? offers : []]);
          lookups += 1;
          await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
        } catch (error) { errors.push(String(error)); await request.abort(); }
      } else if (/(^|\.)(googlesyndication\.com|doubleclick\.net|googleadservices\.com)$/.test(url.hostname)) {
        await request.abort();
      } else await request.continue();
    });
    await page.goto(`${base}/list/${listId}`, { waitUntil: "domcontentloaded" });
    await expectSummary("20 gil", true, "3 units still unpriced");
    assert(lookups > 0);
    assert((await state()).status.includes("Known subtotal only"));
    assert((await state()).status.includes("0 of 1 items fully priced"));
    assert((await state()).row.includes("20 gil"));
    const artifacts = path.join(__dirname, "artifacts", "list-partial-subtotal");
    fs.mkdirSync(artifacts, { recursive: true });
    await page.screenshot({ path: path.join(artifacts, "desktop.png"), fullPage: true });
    await page.setViewport({ width: 390, height: 844 });
    await page.reload({ waitUntil: "domcontentloaded" });
    await expectSummary("20 gil", true, "3 units still unpriced");
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "mobile subtotal must not overflow");
    await page.screenshot({ path: path.join(artifacts, "mobile.png"), fullPage: true });
    stock = 0;
    await page.reload({ waitUntil: "domcontentloaded" });
    await expectSummary("—", true, "No listed prices");
    stock = 5;
    await page.reload({ waitUntil: "domcontentloaded" });
    await expectSummary("50 gil", false, "Cheapest listed units");
    assert.deepEqual(errors, [], "application and fixture errors");
    console.log("PASS: partial subtotal 20 gil/3 missing survives desktop/mobile reload; zero and complete supply stay distinct");
  } catch (error) {
    console.error("Subtotal state", await state().catch(String), "lookups", lookups, "errors", errors);
    throw error;
  } finally {
    if (listId) await api("DELETE", `/api/v1/list/${listId}/delete`).catch(console.error);
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
