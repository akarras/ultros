// Own-build HTTP regression probe. Requires disposable listing/floor fixtures.
const assert = require('node:assert/strict');
const base = process.env.BASE_URL || 'http://127.0.0.1:18412';
const world = process.env.WORLD || 'Gilgamesh';
async function request(path, options) {
  const response = await fetch(base + path, options);
  const text = await response.text();
  return { response, text, data: response.ok ? JSON.parse(text) : null };
}
(async () => {
  const route = `/api/v1/listing_stats/${world}`;
  const current = await request(route);
  assert.equal(current.response.status, 200);
  assert(current.data.stats.every(row => !Object.hasOwn(row, 'window')), 'current-only wire stays unchanged');
  for (const days of [1, 7, 30, 90]) {
    const first = await request(`${route}?window=${days}`);
    assert.equal(first.response.status, 200, first.text);
    assert(first.data.stats.length > 0, 'fixture must exercise populated history');
    assert(first.data.stats.every(row => row.window.window_days === days), 'cache must not mix windows');
    const second = await request(`${route}?window=${days}`);
    assert.equal(second.response.headers.get('x-ultros-cache'), 'fresh');
    assert.deepEqual(second.data, first.data);
  }
  for (const invalid of ['0', '2', '365', '-1', 'bogus']) {
    assert.equal((await request(`${route}?window=${invalid}`)).response.status, 400);
  }
  const to = Math.floor(Date.now() / 1000) - 60;
  const body = { item_ids: [12], from: to - 86400, to, interval: 'hourly', hq: false };
  const post = value => request(`/api/v1/floor_history/${world}`, {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(value),
  });
  const hourly = await post(body);
  assert.equal(hourly.response.status, 200, hourly.text);
  assert.equal(hourly.data.series.length, 1);
  assert.equal(hourly.data.series[0].history.points.length, 25);
  assert(hourly.data.series[0].bounds.min > 0, 'seeded floor must carry into window');
  assert.deepEqual((await post(body)).data, hourly.data, 'batch cache hit');
  const daily = await post({ ...body, interval: 'daily' });
  assert.equal(daily.response.status, 200);
  assert.equal(daily.data.series[0].history.points.length, 2, 'cadence cache isolation');
  const both = await post({ ...body, hq: null });
  assert.equal(both.data.series.length, 2, 'quality cache isolation');
  assert(both.data.series[1].unknown_timestamps.length > 0, 'unknown HQ is explicit');
  const legacy = await request(`/api/v1/floor_history/${world}/12?from=${body.from}&to=${to}&hq=nq`);
  assert.equal(legacy.response.status, 200);
  assert(Array.isArray(legacy.data.points), 'legacy per-item chart shape');
  assert(!Object.hasOwn(legacy.data, 'series'), 'legacy chart cache stays separate');
  for (const invalid of [
    { ...body, item_ids: Array(21).fill(12) },
    { ...body, from: to },
    { ...body, from: to - 91 * 86400 },
    { ...body, to: to + 86400 },
    { ...body, item_ids: [] },
  ]) assert.equal((await post(invalid)).response.status, 400);
  const large = await post({ ...body, padding: 'x'.repeat(17000) });
  assert.equal(large.response.status, 413);
  console.log('Listing history API passed: legacy wire, all window caches, bounded floor batches, cadence/quality cache isolation, unknown coverage.');
})().catch(error => { console.error(error); process.exitCode = 1; });
