"use strict";
const assert = require("node:assert/strict");
const tid = id => `[data-testid="${id}"]`;

// A single account document, edited through the UI from an empty cart through
// partial purchase and back. All prices come from test-auth DB fixture rows.
async function runPricedCartJourney({ page, market, createList, load, click,
  replace, summary, stock, checkBuildReference, checked, record, refreshOwner }) {
  await market.set("multi_item");
  const id = await createList([]);
  await load(`/list/${id}`);
  const [firstName, secondName] = market.manifest.item_names;
  for (const [index, quantity] of [[0, 3], [1, 2]]) {
    const name = market.manifest.item_names[index];
    await replace('input[aria-label="Quantity to add"]', quantity);
    await replace('input[aria-label="Add an item"]', name);
    await page.waitForSelector(`button[aria-label="Add ${name}"]`);
    await page.keyboard.press("Enter"); await page.keyboard.press("Escape");
    await page.waitForSelector(`[data-item-id="${market.manifest.item_ids[index]}"] input[aria-label="Needed for ${name}"]`);
  }
  assert.equal(await page.$$eval(`${tid("cart-rows")} > li`, rows => rows.length), 2, "two distinct catalog items added through UI to this document");
  await summary("46 gil", false); // Bronze Any3 = 32, second item Any2 = 14.
  await stock(id);
  await replace(`input[aria-label="Needed for ${firstName}"]`, 2);
  await page.keyboard.press("Tab");
  await summary("34 gil", false);
  await page.select(`select[aria-label="Quality for ${firstName}"]`, "hq");
  await summary("54 gil", false); // HQ2 = 40, second item2 = 14.
  await page.waitForFunction(({ first, second }) => {
    const line = item => document.querySelector(`[data-item-id="${item}"] [data-testid="cart-line-estimate"]`)?.textContent;
    return line(first)?.includes("40 gil") && line(second)?.includes("14 gil");
  }, {}, { first: market.manifest.item_ids[0], second: market.manifest.item_ids[1] });
  await click(`button[aria-label="Remove ${secondName}"]`);
  await page.waitForFunction(item => !document.querySelector(`[data-testid="cart-rows"] [data-item-id="${item}"]`), {}, market.manifest.item_ids[1]);
  await summary("40 gil", false);
  await click(tid("list-undo"));
  await page.waitForSelector(`[data-item-id="${market.manifest.item_ids[1]}"]`);
  await summary("54 gil", false);
  assert.equal(await page.$eval(`select[aria-label="Quality for ${firstName}"]`, select => select.value), "hq", "Undo restores deleted second item without reverting preceding quality edit");
  await checkBuildReference(false);
  const hqOffer = market.manifest.listings.find(offer => offer.item_id === market.manifest.item_ids[0] && offer.hq && offer.world_id === market.manifest.worlds[0].id);
  assert(hqOffer, "real first-world HQ stack");
  const row = `[data-shop-key="${hqOffer.id}"]`;
  await page.waitForSelector(`${row} ${tid("shop-stack-bought")}`);
  await replace(`${row} ${tid("shop-stack-quantity")}`, 1);
  await click(`${row} ${tid("shop-stack-bought")}`);
  await click(tid("guest-build-mode"));
  await click(`button[aria-label="Details for ${firstName}"]`);
  await page.waitForFunction(name => document.querySelector(`input[aria-label="Owned for ${name}"]`)?.value === "1", {}, firstName);
  await summary("34 gil", false); // remaining HQ1 = 20 plus second item2 = 14.
  assert.equal(await page.$eval(`input[aria-label="Needed for ${firstName}"]`, input => input.value), "2");
  assert.equal(await page.$$eval(`${tid("cart-rows")} > li`, rows => rows.length), 2);
  await page.waitForFunction(async ({ id, item }) => {
    const response = await fetch(`/api/v1/list/${id}/listings`);
    if (!response.ok) return false;
    const rows = (await response.json())[1].map(([row]) => row);
    return rows.length === 2 && rows.some(row => row.item_id === item && row.hq === true && row.quantity === 2 && row.acquired === 1);
  }, {}, { id, item: market.manifest.item_ids[0] });
  await stock(id);
  await record("account-two-item-priced-edit-delete-undo-purchase-build");
  // Current Build changes after purchase; the already chosen trip reference
  // remains the reviewed 54-gil estimate until an explicit review is applied.
  await click(tid("guest-shop-mode"));
  await page.$eval(tid("shop-estimate"), details => { details.open = true; });
  assert.match(await page.$eval(tid("shop-build-reference"), node => node.textContent), /Build estimate: 54 gil/);
  await click(tid("guest-build-mode"));
  const document = (await checked("GET", `/api/v1/list/${id}/listings`))[0];
  assert.equal(document.list.id, id, "the complete journey stayed on its original account list");
  await refreshOwner();
  await market.set("baseline");
}
module.exports = { runPricedCartJourney };
