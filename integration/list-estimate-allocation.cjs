// Deterministic Build pricing through the real account editor. Only listing
// offers are replaced; authentication, rows, edits and synchronization are real.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const puppeteer = require("puppeteer");
const base = process.env.BASE_URL || "http://127.0.0.1:8080";
const user = 990000000813;
const item = 5056;

async function main() {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  page.setDefaultTimeout(60000);
  page.setDefaultNavigationTimeout(60000);
  await page.setCacheEnabled(false);
  await page.setViewport({ width: 1280, height: 900 });
  const errors = [];
  let lookups = 0;
  page.on("pageerror", error => {
    if (!String(error.stack).includes("pagead2.googlesyndication.com")) errors.push(String(error.stack || error));
  });
  page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
  let listId;
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
    rows: [...document.querySelectorAll('[data-testid="cart-rows"] > li')].map(row => ({
      quality: row.querySelector("select")?.value,
      cost: row.querySelector('[data-testid="cart-line-estimate"]')?.textContent,
      detail: row.querySelector('[data-testid="cart-pricing-detail"]')?.textContent,
      units: [...row.querySelectorAll("tbody tr")].map(tr => Number(tr.cells[2].textContent)),
    })),
  }));
  async function waitCosts(expected, incomplete) {
    await page.waitForFunction(({ expected, incomplete }) => {
      const total = document.querySelector('[data-testid="list-estimate-total"]');
      return total?.textContent === "150 gil" && total.dataset.incomplete === String(incomplete) &&
        Object.entries(expected).every(([quality, cost]) => {
          const row = [...document.querySelectorAll('[data-testid="cart-rows"] > li')].find(row => row.querySelector("select")?.value === quality);
          return row?.querySelector('[data-testid="cart-line-estimate"]')?.textContent.includes(`${cost} gil`);
        });
    }, {}, { expected, incomplete });
  }
  try {
    const login = await page.goto(`${base}/test/login?user_id=${user}&username=EstimateAllocationQA&redirect=/list`, { waitUntil: "domcontentloaded" });
    assert(login.ok(), "test-auth must be enabled");
    const worlds = await api("GET", "/api/v1/world_data");
    const world = worlds.regions[0].datacenters[0].worlds[0];
    await page.setCookie(...[
      ["LABS", "lists-sync"], ["HIDE_ADS", "true"], ["HOME_WORLD", world.name], ["i18n_pref_locale", "en"],
    ].map(([name, value]) => ({ name, value, url: base, path: "/" })));
    const name = `Shared estimate supply ${Date.now()}`;
    await api("POST", "/api/v1/list/create", { name, wdr_filter: { World: world.id } });
    listId = (await api("GET", "/api/v1/list")).find(entry => entry.list.name === name).list.id;
    for (const hq of [null, true, false]) {
      await api("POST", `/api/v1/list/${listId}/add/item`, { id: 0, list_id: listId, item_id: item, hq, quantity: hq === null ? 4 : 2, acquired: 0 });
    }
    const cookie = (await page.cookies()).map(entry => `${entry.name}=${entry.value}`).join("; ");
    const offers = [
      { id: -1469001, world_id: world.id, item_id: item, retainer_id: 1, price_per_unit: 10, quantity: 3, hq: true, timestamp: "2026-09-15T00:00:00" },
      { id: -1469002, world_id: world.id, item_id: item, retainer_id: 2, price_per_unit: 30, quantity: 4, hq: false, timestamp: "2026-09-15T00:00:00" },
    ];
    await page.setRequestInterception(true);
    page.on("request", async request => {
      const url = new URL(request.url());
      if (url.origin === base && url.pathname === `/api/v1/list/${listId}/listings` && request.method() === "GET") {
        try {
          // Fetch the current real rows outside the intercepted browser, then
          // supply the same physical offers to every quality row, as the API does.
          const response = await fetch(url, { headers: { cookie } });
          assert(response.ok, `fixture row fetch: ${response.status}`);
          const body = await response.json();
          body[1] = body[1].map(([row]) => [row, row.item_id === item ? offers : []]);
          lookups += 1;
          await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
        } catch (error) { errors.push(String(error)); await request.abort(); }
      } else if (/(^|\.)(googlesyndication\.com|doubleclick\.net|googleadservices\.com)$/.test(url.hostname)) {
        await request.abort();
      } else await request.continue();
    });
    await page.goto(`${base}/list/${listId}`, { waitUntil: "domcontentloaded" });
    await waitCosts({ any: 70, hq: 20, nq: 60 }, true);
    assert(lookups > 0, "deterministic offers must have reached the editor");
    await page.evaluate(() => {
      const row = [...document.querySelectorAll('[data-testid="cart-rows"] > li')].find(row => row.querySelector("select")?.value === "any");
      row.querySelector('button[aria-label^="Details for "]').click();
    });
    await page.waitForFunction(() => document.querySelector('[data-testid="cart-pricing-detail"]')?.textContent.includes("covers 3 of 4"));
    assert.deepEqual((await state()).rows.find(row => row.quality === "any").units, [1, 2]);
    const artifacts = path.join(__dirname, "artifacts", "list-estimate-allocation");
    fs.mkdirSync(artifacts, { recursive: true });
    await page.screenshot({ path: path.join(artifacts, "desktop.png"), fullPage: true });
    await page.$eval('button[aria-label="Sort by Est. cost"]', button => button.scrollIntoView({ block: "center", behavior: "instant" }));
    await page.click('button[aria-label="Sort by Est. cost"]');
    await page.waitForFunction(() => [...document.querySelectorAll('[data-testid="cart-rows"] > li select')].map(select => select.value).join() === "hq,nq,any");
    await page.evaluate(() => {
      const row = [...document.querySelectorAll('[data-testid="cart-rows"] > li')].find(row => row.querySelector("select")?.value === "hq");
      const input = row.querySelector('input[aria-label^="Needed for "]');
      input.value = "1";
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await waitCosts({ any: 80, hq: 10, nq: 60 }, false);
    await page.waitForFunction(() => document.querySelector('[data-testid="cart-pricing-detail"]')?.textContent.includes("covers 4 of 4"));
    assert.deepEqual((await state()).rows.find(row => row.quality === "any").units, [2, 2], "details must update the units of the same listing id");
    await page.waitForFunction(() => document.querySelector('[data-testid="account-list-save-state"]')?.textContent.includes("Saved on this device"));
    await page.reload({ waitUntil: "domcontentloaded" });
    await waitCosts({ any: 80, hq: 10, nq: 60 }, false);
    await page.setViewport({ width: 390, height: 844 });
    await page.select('select[aria-label="Sort by"]', "price-desc");
    await page.waitForFunction(() => [...document.querySelectorAll('[data-testid="cart-rows"] > li select')].map(select => select.value).join() === "any,nq,hq");
    await page.evaluate(() => {
      const row = [...document.querySelectorAll('[data-testid="cart-rows"] > li')].find(row => row.querySelector("select")?.value === "any");
      row.querySelector('button[aria-label^="Details for "]').click();
    });
    await page.waitForFunction(() => document.querySelector('[data-testid="cart-pricing-detail"]')?.textContent.includes("covers 4 of 4"));
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "mobile cart must not overflow");
    await page.screenshot({ path: path.join(artifacts, "mobile.png"), fullPage: true });
    assert.deepEqual(errors, [], "application and fixture errors");
    console.log("PASS: one supply allocation drives row costs, totals, details, sorting, edits and reload on desktop/mobile");
  } catch (error) {
    console.error("Allocation state", await state().catch(String));
    console.error("Fixture lookups", lookups, "errors", errors);
    throw error;
  } finally {
    if (listId) await api("DELETE", `/api/v1/list/${listId}/delete`).catch(console.error);
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
