// Requires a fresh test-auth server. Exercises real account handles and Loro,
// two offline tabs, failure feedback/retry, and restoration after both close.
const assert = require("node:assert/strict");
const puppeteer = require("puppeteer");
const fs = require("node:fs");
const path = require("node:path");
const base = process.env.BASE_URL || "http://127.0.0.1:8080";
const user = 990000000811;
const needed = 'input[aria-label^="Needed for "]';
async function api(page, method, path, body) {
  return page.evaluate(
    async ({ method, path, body }) => {
      const r = await fetch(path, {
        method,
        headers: { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      if (!r.ok) throw new Error(`${method} ${path}: ${r.status}`);
      const text = await r.text();
      return text ? JSON.parse(text) : null;
    },
    { method, path, body },
  );
}
async function ready(page) {
  await page.waitForFunction(
    (sel) => document.querySelectorAll(sel).length === 2,
    { polling: 100, timeout: 60000 },
    needed,
  );
}
async function saved(page) {
  try {
    await page.waitForFunction(
      () =>
        document
          .querySelector('[data-testid="account-list-save-state"]')
          ?.textContent.includes("Saved on this device"),
      { polling: 100, timeout: 60000 },
    );
  } catch (error) {
    console.error(
      "Save state:",
      await page.evaluate(() => ({
        state: document.querySelector('[data-testid="account-list-save-state"]')
          ?.textContent,
        keys: Object.keys(localStorage).filter((k) =>
          k.startsWith("ultros.listdoc."),
        ),
        locks: !!navigator.locks,
      })),
    );
    throw error;
  }
}
async function edit(page, index, value) {
  await page.evaluate(
    ({ sel, index, value }) => {
      const el = document.querySelectorAll(sel)[index];
      el.value = String(value);
      el.dispatchEvent(new Event("change", { bubbles: true }));
    },
    { sel: needed, index, value },
  );
}
async function main() {
  const browser = await puppeteer.launch({ headless: true });
  const appErrors = [];
  const watch = (page) =>
    page.on("pageerror", (error) => {
      if (!String(error.stack).includes("pagead2.googlesyndication.com"))
        appErrors.push(String(error.stack || error));
    });
  let control, id, testError;
  try {
    const context = await browser.createBrowserContext();
    control = await context.newPage();
    await control.setCacheEnabled(false);
    const response = await control.goto(
      `${base}/test/login?user_id=${user}&username=AccountStorageQA&redirect=/list`,
    );
    assert(response.ok(), "test-auth login must be enabled");
    await control.setCookie({
      name: "LABS",
      value: "lists-sync",
      url: base,
      path: "/",
    });
    const worlds = await api(control, "GET", "/api/v1/world_data");
    const name = `Account storage QA ${Date.now()}`;
    await api(control, "POST", "/api/v1/list/create", {
      name,
      wdr_filter: { World: worlds.regions[0].datacenters[0].worlds[0].id },
    });
    id = (await api(control, "GET", "/api/v1/list")).find(
      (v) => v.list.name === name,
    ).list.id;
    for (const item_id of [5, 6])
      await api(control, "POST", `/api/v1/list/${id}/add/item`, {
        id: 0,
        item_id,
        list_id: id,
        hq: null,
        quantity: 1,
        acquired: 0,
      });
    const a = await context.newPage(),
      b = await context.newPage();
    for (const [index, page] of [a, b].entries()) {
      console.log(`Opening account tab ${index + 1}`);
      await page.setCacheEnabled(false);
      watch(page);
      await page.goto(`${base}/list/${id}`, {
        waitUntil: "domcontentloaded",
        timeout: 60000,
      });
      await ready(page);
      await saved(page);
      await page.setOfflineMode(true);
    }
    console.log("Both tabs offline; editing separate rows");
    await Promise.all([edit(a, 0, 3), edit(b, 1, 4)]);
    await Promise.all([saved(a), saved(b)]);
    for (const page of [a, b])
      await page.waitForFunction(
        (sel) =>
          [...document.querySelectorAll(sel)]
            .map((e) => Number(e.value))
            .sort()
            .join(",") === "3,4",
        { polling: 100 },
        needed,
      );
    const server = (
      await api(control, "GET", `/api/v1/list/${id}/listings`)
    )[1].map(([item]) => item.quantity);
    assert.deepEqual(
      server,
      [1, 1],
      "edits must actually be offline before persistence verification",
    );
    await b.close();
    await a.close();
    const recovered = await context.newPage();
    watch(recovered);
    await recovered.setCacheEnabled(false);
    await recovered.goto(`${base}/list/${id}`, {
      waitUntil: "domcontentloaded",
      timeout: 60000,
    });
    await ready(recovered);
    await saved(recovered);
    await recovered.waitForFunction(
      (sel) =>
        [...document.querySelectorAll(sel)]
          .map((e) => Number(e.value))
          .sort()
          .join(",") === "3,4",
      { polling: 100 },
      needed,
    );
    console.log(
      "Both offline tabs closed; both edits recovered. Injecting quota failure",
    );
    await recovered.setOfflineMode(true);
    // Fault injection affects only this test tab and only list snapshot writes.
    await recovered.evaluate(() => {
      window.originalSet = Storage.prototype.setItem;
      Storage.prototype.setItem = function (k, v) {
        if (k.startsWith("ultros.listdoc.v1."))
          throw new DOMException("QA quota", "QuotaExceededError");
        return originalSet.call(this, k, v);
      };
    });
    await edit(recovered, 0, 7);
    await recovered.waitForFunction(
      () =>
        document
          .querySelector('[data-testid="account-list-save-state"]')
          ?.textContent.includes("not saved"),
      { polling: 100 },
    );
    assert.equal(
      await recovered.evaluate(() =>
        [...document.querySelectorAll("button")].some((e) =>
          e.textContent.includes("Download recovery copy"),
        ),
      ),
      true,
    );
    const artifacts = path.join(__dirname, "artifacts", "account-storage");
    fs.mkdirSync(artifacts, { recursive: true });
    await recovered.screenshot({
      path: path.join(artifacts, "save-failure.png"),
      fullPage: true,
    });
    // Client-side navigation must retain the failed save in the same runtime.
    await recovered.evaluate(() =>
      [...document.querySelectorAll("a")]
        .find((e) => e.getAttribute("href") === "/list")
        .click(),
    );
    await recovered.waitForFunction(() => location.pathname === "/list", {
      polling: 100,
    });
    // The account route still needs its REST shell when it remounts. Restore
    // network after closing the old route; its failed snapshot stays in memory.
    await recovered.setOfflineMode(false);
    // Use an ordinary internal link so Leptos performs a client navigation;
    // the offline directory may not have fetched this account's list cards.
    await recovered.evaluate((id) => {
      const link = document.createElement("a");
      link.href = `/list/${id}`;
      document.body.append(link);
      link.click();
      link.remove();
    }, id);
    await recovered.waitForFunction(
      (id) => location.pathname === `/list/${id}`,
      { polling: 100 },
      id,
    );
    await ready(recovered);
    await recovered.waitForFunction(
      (sel) => [...document.querySelectorAll(sel)].some((e) => e.value === "7"),
      { polling: 100 },
      needed,
    );
    await recovered.waitForFunction(
      () =>
        document
          .querySelector('[data-testid="account-list-save-state"]')
          ?.textContent.includes("not saved"),
      { polling: 100 },
    );
    await recovered.evaluate(() => {
      Storage.prototype.setItem = originalSet;
      [...document.querySelectorAll("button")]
        .find((e) => e.textContent.includes("Retry save"))
        .click();
    });
    await saved(recovered);
    await recovered.close();
    const reopened = await context.newPage();
    watch(reopened);
    await reopened.setCacheEnabled(false);
    await reopened.goto(`${base}/list/${id}`, {
      waitUntil: "domcontentloaded",
      timeout: 60000,
    });
    await ready(reopened);
    await saved(reopened);
    await reopened.waitForFunction(
      (sel) =>
        [...document.querySelectorAll(sel)]
          .map((e) => Number(e.value))
          .sort()
          .join(",") === "4,7",
      { polling: 100 },
      needed,
    );
    reopened.on("dialog", async dialog => { console.log("Account navigation dialog", dialog.type()); await dialog.accept(); });
    const logoutStarted = Date.now();
    reopened.on("response", response => { if (response.request().isNavigationRequest()) console.log("Account navigation response", Date.now() - logoutStarted, response.status(), new URL(response.url()).pathname); });
    console.log("Logging out recovered account");
    await reopened.goto(`${base}/logout`, {
      waitUntil: "domcontentloaded",
      timeout: 60000,
    });
    await reopened.goto(
      `${base}/test/login?user_id=${user + 1}&username=AccountStorageOther&redirect=/list`,
      { waitUntil: "domcontentloaded", timeout: 60000 },
    );
    assert.equal(
      (await api(reopened, "GET", "/api/v1/list")).some(
        (v) => v.list.id === id,
      ),
      false,
      "another account must not list the original account snapshot",
    );
    await reopened.goto(
      `${base}/test/login?user_id=${user}&username=AccountStorageQA&redirect=/list`,
      { waitUntil: "domcontentloaded", timeout: 60000 },
    );
    await reopened.goto(`${base}/list/${id}`, {
      waitUntil: "domcontentloaded",
      timeout: 60000,
    });
    await ready(reopened);
    await saved(reopened);
    await reopened.waitForFunction(
      (sel) =>
        [...document.querySelectorAll(sel)]
          .map((e) => Number(e.value))
          .sort()
          .join(",") === "4,7",
      { polling: 100 },
      needed,
    );
    assert.deepEqual(
      appErrors,
      [],
      "account pages must not throw application errors",
    );
    console.log("Account list UI persistence and recovery checks passed");
  } catch (error) {
    testError = error;
  } finally {
    const cleanup = [];
    if (control && id) cleanup.push((async () => {
      await control.goto(`${base}/test/login?user_id=${user}&username=AccountStorageQA&redirect=/`, { waitUntil: "domcontentloaded", timeout: 60000 });
      await api(control, "DELETE", `/api/v1/list/${id}/delete`);
    })());
    const results = await Promise.allSettled(cleanup);
    results.push(...await Promise.allSettled([browser.close()]));
    const failures = [testError, ...results.filter(result => result.status === "rejected").map(result => result.reason)].filter(Boolean);
    if (failures.length) throw new AggregateError(failures, "Account UI validation or owned fixture cleanup failed");
  }
}
main().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
