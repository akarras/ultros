const assert = require('node:assert/strict');
const test = require('node:test');
const { marketWireBody } = require('./market-wire-fixture.cjs');
const { marketFixture } = require('./shared-analyzer-market-fixture.cjs');

const url = kind => `http://localhost/api/v1/${kind}/Gilgamesh?window=7&format=columnar`;

test('cheapest uses the new price column and mutations run before wire encoding', () => {
  const fixture = marketFixture([5, 6]);
  const response = fixture.reply({ url: () => url('cheapest') }, body => {
    body.cheapest_listings[0].cheapest_price = 999999999;
    body.cheapest_listings[1].world_id = 79;
  });
  assert.deepEqual(JSON.parse(response.body), {
    item_id: [5, 6], hq: [false, false], price: [999999999, 400], world_id: [63, 79],
  });
});

test('recent sales keeps zero-count groups and flattens UTC sales in order', () => {
  assert.deepEqual(marketWireBody(url('recentSales'), { sales: [
    { item_id: 5, hq: false, sales: [] },
    { item_id: 5, hq: true, sales: [
      { price_per_unit: 42, sale_date: '2026-09-22T00:00:00' },
      { price_per_unit: 21, sale_date: '2026-09-21T23:59:59' },
    ] },
  ] }), {
    item_id: [5, 5], hq: [false, true], count: [0, 2], price: [42, 21],
    sold_unix: [1790035200, 1790035199],
  });
});

test('sale stats always provides all twelve parallel columns including missing-evidence values', () => {
  const result = marketWireBody(url('sale_stats'), { stats: [{ item_id: 5, hq: true, min_price: 0, median_price: 0, avg_price: 10, num_sold: 3 }] });
  assert.deepEqual(result, {
    item_id: [5], hq: [true], min_price: [0], median_price: [0], avg_price: [10], num_sold: [3],
    last_sold_unix: [0], units_sold: [0], vwap: [0], gil_volume: [0], sales_per_day: [0], confidence: ['unknown'],
  });
  assert(Object.values(marketWireBody(url('sale_stats'), { stats: [] })).every(column => column.length === 0));
});

test('listing windows remain aligned and absent history is not a measured zero', () => {
  const fixture = marketFixture([5]);
  const body = JSON.parse(fixture.reply({ url: () => url('listing_stats') }).body);
  const known = body.window[0];
  const mixed = marketWireBody(url('listing_stats'), { stats: [
    { item_id: 5, hq: false, window: known }, { item_id: 6, hq: false },
  ] });
  assert.deepEqual(mixed.window[0], known);
  assert.equal(mixed.window.length, mixed.item_id.length);
  assert.equal(mixed.window[1].undercuts_per_day, null);
  assert.equal(mixed.window[1].stock_status, 'unavailable');
  const current = marketWireBody(url('listing_stats'), { stats: [{ item_id: 6, hq: false }] });
  assert.equal(Object.hasOwn(current, 'window'), false);
  assert.equal(Object.keys(current).length, 8);
});

test('legacy requests, unrelated DTOs and error responses keep their wire shape', () => {
  const legacy = { cheapest_listings: [] };
  assert.equal(marketWireBody('/api/v1/cheapest/Gilgamesh', legacy), legacy);
  const trends = { items: [] };
  assert.equal(marketWireBody(url('trends'), trends), trends);
  const failure = { error: 'temporarily unavailable' };
  assert.equal(marketWireBody(url('sale_stats'), failure), failure);
});
