// #1475: real account rows and edits, deterministic market offers, and the
// browser accessibility tree. Requires this branch's fresh test-auth build.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const puppeteer = require("puppeteer");
const base = new URL(process.env.BASE_URL || "http://127.0.0.1:53122").origin;
const cart = '[data-testid="list-cart"]';
const rows = '[data-testid="cart-rows"] > li';

async function main() {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  page.setDefaultTimeout(60000);
  page.setDefaultNavigationTimeout(Number(process.env.NAVIGATION_TIMEOUT_MS || 120000));
  await page.setViewport({ width: 1280, height: 900 });
  const errors = [];
  page.on("pageerror", error => {
    if (!String(error.stack).includes("googlesyndication.com")) errors.push(String(error.stack || error));
  });
  page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
  let listId;
  let lookups = 0;
  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, { method, headers: { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
    if (!response.ok) throw new Error(`${method} ${route}: ${response.status}`);
    const text = await response.text();
    return text ? JSON.parse(text) : null;
  }, { method, route, body });
  const order = () => page.$$eval(rows, entries => entries.map(row => row.querySelector("select").value));
  const waitOrder = expected => page.waitForFunction(({ rows, expected }) =>
    [...document.querySelectorAll(rows)].map(row => row.querySelector("select").value).join() === expected.join(),
  {}, { rows, expected });
  const rowSelector = async quality => page.$$eval(rows, (entries, quality) => {
    const row = entries.find(row => row.querySelector("select").value === quality);
    if (!row) throw new Error(`Missing ${quality} row`);
    return `[data-row-id="${row.dataset.rowId}"]`;
  }, quality);
  const saved = () => page.waitForFunction(() =>
    document.querySelector('[data-testid="account-list-save-state"]')?.textContent.includes("Saved on this device"));
  const button = column => `button[aria-label="Sort by ${column}"]`;
  const activate = async (column, key) => {
    await page.focus(button(column));
    await page.keyboard.press(key);
  };
  async function checkSortAX(column, description, pressed = true) {
    await page.waitForFunction(({ column, pressed }) =>
      document.querySelector(`button[aria-label="Sort by ${column}"]`)?.getAttribute("aria-pressed") === String(pressed),
    {}, { column, pressed });
    const client = await page.createCDPSession();
    try {
      const { nodes } = await client.send("Accessibility.getFullAXTree");
      const node = nodes.find(node => !node.ignored && node.role?.value === "button" && node.name?.value === `Sort by ${column}`);
      assert(node, `${column} is exposed as a button`);
      assert.equal(String(node.properties.find(property => property.name === "pressed")?.value.value), String(pressed));
      assert.equal(node.description?.value, description, "computed accessible description exposes direction");
      const { root } = await client.send("DOM.getDocument");
      const { nodeId } = await client.send("DOM.querySelector", { nodeId: root.nodeId, selector: '[data-testid="cart-rows"]' });
      const { node: domList } = await client.send("DOM.describeNode", { nodeId });
      const list = nodes.find(node => node.backendDOMNodeId === domList.backendNodeId);
      assert.equal(list?.role?.value, "list", "the actual cart is exposed as a native list");
      assert.equal(list.childIds.filter(id => nodes.find(child => child.nodeId === id)?.role?.value === "listitem").length, 3,
        "each cart row is associated with its list in the accessibility tree");
    } finally { await client.detach(); }
  }
  try {
    const login = await page.goto(`${base}/test/login?user_id=990000000875&username=ListSortQA&redirect=/list`, { waitUntil: "domcontentloaded" });
    assert(login.ok(), "test-auth must be enabled");
    const worlds = await api("GET", "/api/v1/world_data");
    const world = worlds.regions[0].datacenters[0].worlds[0];
    await page.setCookie(...[["LABS", "lists-sync"], ["HIDE_ADS", "true"], ["HOME_WORLD", world.name], ["i18n_pref_locale", "en"]].map(([name, value]) => ({ name, value, url: base, path: "/" })));
    const name = `Sort semantics ${Date.now()}`;
    await api("POST", "/api/v1/list/create", { name, wdr_filter: { World: world.id } });
    listId = (await api("GET", "/api/v1/list")).find(entry => entry.list.name === name).list.id;
    for (const [hq, quantity, acquired] of [[null, 10, 9], [false, 2, 0], [true, 3, 0]]) {
      await api("POST", `/api/v1/list/${listId}/add/item`, { id: 0, list_id: listId, item_id: 5056, hq, quantity, acquired });
    }
    const cookie = (await page.cookies()).map(entry => `${entry.name}=${entry.value}`).join("; ");
    await page.setRequestInterception(true);
    page.on("request", async request => {
      const url = new URL(request.url());
      if (url.origin === base && url.pathname === `/api/v1/list/${listId}/listings` && request.method() === "GET") {
        try {
          const response = await fetch(url, { headers: { cookie } });
          assert(response.ok, "real row fetch");
          const body = await response.json();
          const offers = [{ id: -1475001, world_id: world.id, item_id: 5056, retainer_id: 1, price_per_unit: 20, quantity: 100, hq: false, timestamp: "2026-09-15T00:00:00" }];
          body[1] = body[1].map(([row]) => [row, offers]);
          lookups++;
          await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
        } catch (error) { errors.push(String(error)); await request.abort(); }
      } else if (/(^|\.)(googlesyndication\.com|doubleclick\.net|googleadservices\.com)$/.test(url.hostname)) await request.abort();
      else await request.continue();
    });
    await page.goto(`${base}/list/${listId}`, { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => document.querySelector('[data-testid="list-estimate-total"]')?.textContent === "60 gil");
    console.log("[ok] real account rows and deterministic market offers loaded");
    assert(lookups > 0, "deterministic offers reached editor");
    assert.deepEqual(await page.$$eval('#list-sort-select option[value^="acquired"]', options => options.map(option => option.textContent)),
      ["Fewest needed first", "Most needed first"], "the account toolbar names the same requested-quantity sort as the cart");
    assert.equal(await page.$$eval(`${cart} [role="columnheader"], ${cart} [aria-sort], ${cart} [role="row"]`, nodes => nodes.length), 0, "list controls have no unsupported table roles or attributes");
    assert(await page.$eval(`${cart} [role="group"][aria-label="Sort by"]`, group => group.querySelectorAll("button[aria-controls='cart-row-list']").length === 3));
    const any = await rowSelector("any");
    const nq = await rowSelector("nq");
    const hq = await rowSelector("hq");
    await page.click(`${nq} input[type="checkbox"]`);
    await activate("Qty", "Enter");
    await waitOrder(["nq", "hq", "any"]);
    await checkSortAX("Qty", "Fewest needed first");
    await activate("Qty", "Space");
    await waitOrder(["any", "hq", "nq"]);
    await checkSortAX("Qty", "Most needed first");
    assert(await page.$eval(`${nq} input[type="checkbox"]`, input => input.checked), "selection follows row identity after sorting");
    await activate("Qty", "Enter");
    await checkSortAX("Qty", undefined, false);
    await activate("Est. cost", "Enter");
    await waitOrder(["any", "nq", "hq"]);
    await activate("Est. cost", "Space");
    await waitOrder(["nq", "any", "hq"]);
    await checkSortAX("Est. cost", "Most expensive first");
    console.log("[ok] desktop sorting, selection, and computed accessibility tree");
    const artifacts = path.join(__dirname, "artifacts", "list-sort-accessibility");
    fs.mkdirSync(artifacts, { recursive: true });
    await page.screenshot({ path: path.join(artifacts, "desktop.png"), fullPage: true });
    await page.setViewport({ width: 390, height: 844 });
    await page.select('select[aria-label="Sort by"]', "price");
    await waitOrder(["any", "nq", "hq"]);
    await page.select('select[aria-label="Sort by"]', "price-desc");
    await waitOrder(["nq", "any", "hq"]);
    await page.select('select[aria-label="Sort by"]', "acquired");
    await waitOrder(["nq", "hq", "any"]);
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "mobile cart has no horizontal overflow");
    await page.screenshot({ path: path.join(artifacts, "mobile.png"), fullPage: true });
    console.log("[ok] mobile sort controls and layout");
    await saved();
    await page.reload({ waitUntil: "domcontentloaded" });
    await waitOrder(["nq", "hq", "any"]);
    await page.setViewport({ width: 1280, height: 900 });
    const input = `${any} input[aria-label^="Needed for "]`;
    await page.focus(input);
    await page.$eval(input, input => input.select());
    await page.keyboard.press("Backspace");
    await page.type(input, "1");
    // Another row's update must not move a drafting editor. Drive the
    // other real editor's change event without moving browser focus.
    await page.$eval(`${nq} input[aria-label^="Needed for "]`, input => {
      input.value = "20";
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await page.waitForFunction(selector => document.querySelector(selector)?.dataset.committed === "20", {}, `${nq} input[aria-label^="Needed for "]`);
    assert.deepEqual(await order(), ["nq", "hq", "any"], "an external row update does not move the draft");
    assert(await page.$eval(input, input => document.activeElement === input), "draft keeps focus");
    await page.keyboard.press("Enter");
    await waitOrder(["any", "hq", "nq"]);
    await saved();
    await page.click(`${any} [data-testid="cart-remove"]`);
    await waitOrder(["hq", "nq"]);
    await page.waitForFunction(selector => document.activeElement === document.querySelector(selector), {}, `${hq} [data-testid="cart-remove"]`);
    await page.click('[data-testid="list-undo"]');
    await waitOrder(["any", "hq", "nq"]);
    assert.deepEqual(errors, [], "no application or fixture errors");
    console.log("PASS: displayed quantity, unknown-last cost, native list AX, keyboard/mobile controls, selection, draft pin/release and deletion focus");
  } catch (error) {
    console.error("Sort state", page.url(), await order().catch(String), "errors", errors);
    throw error;
  } finally {
    if (listId) await api("DELETE", `/api/v1/list/${listId}/delete`).catch(console.error);
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
