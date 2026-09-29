"use strict";
const assert = require("node:assert/strict");
const { randomUUID } = require("node:crypto");

// Uses actual server/database rows; no Puppeteer request interception. The
// route refuses preexisting stock, so this must run on a disposable market DB.
async function createMarketFixture(api, worldData) {
  const token = randomUUID().replaceAll("-", "");
  const region = worldData.regions.find(region =>
    region.datacenters.filter(dc => dc.worlds.length).length >= 2 &&
    region.datacenters.some(dc => dc.worlds.length >= 2));
  assert(region, "market fixture requires two populated datacenters in one region");
  const fixture = { token, region, manifest: null };
  fixture.set = async scenario => {
    const result = await api("POST", "/test/list/market-fixture", {
      token, region_id: region.id, scenario,
    });
    assert.equal(result.status, 200,
      `Required real market fixture failed (${scenario}): ${JSON.stringify(result.body)}. Build with test-auth and use a disposable empty market database.`);
    assert.equal(result.body.token, token);
    assert.equal(result.body.isolated, true, "required fixture must attest frozen test-auth market writer isolation");
    fixture.manifest = result.body;
    console.log(`[fixture] ${JSON.stringify({ token, scenario, ...result.body })}`);
    return result.body;
  };
  fixture.cleanup = async () => {
    const result = await api("DELETE", `/test/list/market-fixture/${token}`);
    assert.equal(result.status, 200, "synthetic market cleanup must succeed");
    assert.equal(result.body, true);
  };
  fixture.assertStock = (offers, itemId) => {
    const select = rows => rows.map(row => [row.id, row.world_id, row.item_id,
      row.hq, row.quantity, row.price_per_unit]).sort((a, b) => a[0] - b[0]);
    assert.deepEqual(select(offers), select(fixture.manifest.listings.filter(row => itemId === undefined || row.item_id === itemId)),
      "real listing response must match exact fixture; outside ingestion or missing rows invalidates acceptance");
  };
  try {
    await fixture.set("baseline");
    const world = fixture.manifest.worlds[0];
    const blocked = await api("GET", `/item/refresh/${encodeURIComponent(world.name)}/${fixture.manifest.item_ids[0]}`);
    assert.equal(blocked.status, 409, "manual upstream refresh must be blocked in isolated market QA");
    assert.match(blocked.body, /test-auth market fixture isolation/, "refresh must be rejected by the explicit isolation guard");
  } catch (error) {
    try { await fixture.cleanup(); } catch (cleanupError) {
      throw new AggregateError([error, cleanupError], "Market fixture setup and owned cleanup failed");
    }
    throw error;
  }
  return fixture;
}
module.exports = { createMarketFixture };
