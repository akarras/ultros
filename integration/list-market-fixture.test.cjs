"use strict";
const assert = require("node:assert/strict");
const test = require("node:test");
const { createMarketFixture } = require("./list-market-fixture.cjs");
const manifest = (token, listings) => ({ token, listings, isolated: true, worlds: [{ id: 1, name: "Fixture World" }], item_ids: [5056, 5057] });
const blocked = { status: 409, body: "Upstream writes blocked by test-auth market fixture isolation" };
const worlds = { regions: [{ id: 2, datacenters: [{ worlds: [{ id: 1 }, { id: 2 }] }, { worlds: [{ id: 3 }] }] }] };

test("fixture refusal is a hard failure and attempts only its own token cleanup", async () => {
  const calls = [];
  await assert.rejects(createMarketFixture(async (method, route, body) => {
    calls.push({ method, route, body });
    return method === "POST" ? { status: 400, body: "existing stock" } : { status: 200, body: true };
  }, worlds), /Required real market fixture failed/);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].route, `/test/list/market-fixture/${calls[0].body.token}`);
});

test("fixture detects missing, changed and additional ingested offers", async () => {
  const listing = { id: 5, world_id: 1, item_id: 5056, hq: false, quantity: 2, price_per_unit: 10 };
  const fixture = await createMarketFixture(async (method, route, body) => ({
    ...(method === "GET" ? blocked : { status: 200, body: method === "DELETE" ? true : manifest(body.token, [listing]) }),
  }), worlds);
  fixture.assertStock([listing]);
  assert.throws(() => fixture.assertStock([]), /outside ingestion or missing rows/);
  assert.throws(() => fixture.assertStock([{ ...listing, price_per_unit: 11 }]), /outside ingestion or missing rows/);
  assert.throws(() => fixture.assertStock([listing, { ...listing, id: 6 }]), /outside ingestion or missing rows/);
});

test("fixture per-item checks still reject cross-item or missing real stock", async () => {
  const first = { id: 5, world_id: 1, item_id: 5056, hq: false, quantity: 2, price_per_unit: 10 };
  const second = { ...first, id: 6, item_id: 5057, quantity: 5, price_per_unit: 7 };
  const fixture = await createMarketFixture(async (method, route, body) => ({
    ...(method === "GET" ? blocked : { status: 200, body: method === "DELETE" ? true : manifest(body.token, [first, second]) }),
  }), worlds);
  fixture.assertStock([first], 5056);
  fixture.assertStock([second], 5057);
  assert.throws(() => fixture.assertStock([second], 5056), /outside ingestion or missing rows/);
  assert.throws(() => fixture.assertStock([], 5057), /outside ingestion or missing rows/);
});

test("acceptance refuses a server without writer-isolation attestation", async () => {
  let cleaned = false;
  await assert.rejects(createMarketFixture(async (method, route, body) => {
    if (method === "DELETE") { cleaned = true; return { status: 200, body: true }; }
    return { status: 200, body: { ...manifest(body.token, []), isolated: false } };
  }, worlds), /attest frozen/);
  assert.equal(cleaned, true);
});

test("acceptance refuses an unblocked manual upstream writer and cleans its fixture", async () => {
  let cleaned = false;
  await assert.rejects(createMarketFixture(async (method, route, body) => {
    if (method === "DELETE") { cleaned = true; return { status: 200, body: true }; }
    if (method === "GET") return { status: 200, body: "unprotected upstream refresh" };
    return { status: 200, body: manifest(body.token, []) };
  }, worlds), /manual upstream refresh must be blocked/);
  assert.equal(cleaned, true);
});
