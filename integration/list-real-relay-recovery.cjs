"use strict";
const assert = require("node:assert/strict");
const { runTerminatedDocRecovery } = require("./list-terminated-recovery.cjs");
const { finishCleanup } = require("./list-fixture-cleanup.cjs");
const tid = id => `[data-testid="${id}"]`;

// Real fixture adapter for #1518. The helper injects only failed transport;
// successful REST prices and every restored socket update use the actual server.
async function runRealRelayRecovery({ browser, base, market, ownerId, createList, ownerApi, record }) {
  const itemName = market.manifest.item_names[0];
  const firstWorld = market.manifest.worlds[0];
  const nq = market.manifest.listings.find(row => row.item_id === 5056 && row.world_id === firstWorld.id && !row.hq);
  const hq = market.manifest.listings.find(row => row.item_id === 5056 && row.world_id === firstWorld.id && row.hq);
  assert(nq && hq, "real fixture needs both first-world quality stacks");
  assert.equal(nq.quantity, 2);
  assert.equal(hq.quantity, 2);
  const nqRow = `[data-shop-key="${nq.id}"]`;
  const hqRow = `[data-shop-key="${hq.id}"]`;
  const evidence = [];
  for (const stream of ["document", "activity"]) {
    const listId = await createList([[false, 5], [true, 6]]);
    let page, remote, originalError;
    const remoteErrors = [];
    async function click(target, selector) {
      await target.waitForSelector(selector, { visible: true });
      await target.$eval(selector, node => node.scrollIntoView({ block: "center", behavior: "instant" }));
      await target.click(selector);
    }
    async function replace(target, selector, value) {
      await click(target, selector);
      await target.$eval(selector, node => node.select());
      await target.keyboard.press("Backspace");
      await target.type(selector, String(value));
    }
    async function open(target) {
      await target.bringToFront();
      const response = await target.goto(`${base}/list/${listId}`, { waitUntil: "domcontentloaded" });
      assert.equal(response.status(), 200);
      await target.waitForFunction(() => window.__relayHydrated);
      await target.waitForFunction(() => document.querySelector('[data-testid="realtime-status-indicator"]')?.dataset.status === "live");
    }
    try {
      page = await browser.newPage();
      remote = await browser.newPage();
      remote.on("pageerror", error => remoteErrors.push(String(error.stack || error)));
      for (const target of [page, remote]) {
        target.setDefaultTimeout(90000);
        await target.setViewport({ width: 1280, height: 900 });
        await target.evaluateOnNewDocument(() => {
          window.__relayHydrated = false;
          addEventListener("ultros:hydrated", () => { window.__relayHydrated = true; });
        });
      }
      // Keep the remote editor on about:blank during the outage: a same-context
      // open list would consume storage events and sync our deliberately unsent edit.
      evidence.push(await runTerminatedDocRecovery({
        page, listId, userId: ownerId, stream, localDelta: 1,
        open: () => open(page),
        prepareShop: async () => {
          await click(page, tid("guest-shop-mode"));
          if (await page.$('[data-testid="shop-change-route"][aria-expanded="false"]')) await page.click('[data-testid="shop-change-route"]'); await click(page, (tid("shop-route-option") + '[data-route-cheapest="true"]'));
          await page.waitForSelector(`${nqRow} ${tid("shop-stack-quantity")}`);
          await page.waitForSelector(`${hqRow} ${tid("shop-stack-quantity")}`);
          for (const row of [nqRow, hqRow])
            assert.equal(await page.$eval(`${row} ${tid("shop-stack-quantity")}`, node => node.max), "2", "actual first-stop stack starts with two purchasable units");
        },
        readServerCounter: async () => {
          const data = await ownerApi("GET", `/api/v1/list/${listId}/listings`);
          for (const [row, offers] of data[1]) market.assertStock(offers, row.item_id);
          const row = data[1].find(([row]) => row.item_id === 5056 && row.hq === false)?.[0];
          assert(row, "real server NQ row required");
          return row.acquired || 0;
        },
        commitLocalEdit: async () => {
          await replace(page, `${nqRow} ${tid("shop-stack-quantity")}`, 1);
          await click(page, `${nqRow} ${tid("shop-stack-bought")}`);
          await page.waitForFunction(selector => document.querySelector(selector)?.max === "1", {}, `${nqRow} ${tid("shop-stack-quantity")}`);
        },
        prepareDraft: async () => {
          const selector = `${hqRow} ${tid("shop-stack-quantity")}`;
          await replace(page, selector, 1);
          return selector;
        },
        readFrozenTrip: () => page.evaluate(() => ({
          reference: document.querySelector('[data-testid="shop-build-reference"]')?.textContent,
          stop: document.querySelector('[data-testid="shop-stop-title"]')?.textContent,
          keys: [...document.querySelectorAll('[data-shop-key]')].map(node => node.dataset.shopKey),
        })),
        remoteEdit: async () => {
          await open(remote);
          await remote.bringToFront();
          const index = await remote.$$eval(`${tid("cart-rows")} > li`, rows => rows.findIndex(row => row.querySelector("select")?.value === "hq"));
          assert(index >= 0, "remote HQ editor row required");
          const row = `${tid("cart-rows")} > li:nth-child(${index + 1})`;
          const details = `${row} button[aria-label="Details for ${itemName}"]`;
          if (await remote.$eval(details, node => node.getAttribute("aria-expanded")) !== "true") await click(remote, details);
          const owned = `${row} input[aria-label="Owned for ${itemName}"]`;
          assert.equal(await remote.$eval(owned, node => node.value), "0");
          await replace(remote, owned, 1);
          await remote.keyboard.press("Enter");
          await page.bringToFront();
        },
        waitRemoteConvergence: () => page.waitForFunction(selector => document.querySelector(selector)?.max === "1", { timeout: 15000 }, `${hqRow} ${tid("shop-stack-quantity")}`),
        cleanup: async () => {},
        record: async (name, result) => {
          console.log(`[evidence] ${name}: ${JSON.stringify(result)}`);
          if (record) await record(name, page);
        },
      }));
      assert.deepEqual(remoteErrors, [], "remote actual editor has no page errors");
    } catch (error) { originalError = error; throw error; }
    finally {
      await finishCleanup([
        async () => { if (remote) await remote.close(); },
        async () => { if (page) await page.close(); },
      ], originalError);
    }
  }
  return evidence;
}
module.exports = { runRealRelayRecovery };
