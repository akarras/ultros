// Wire fixtures only: the running app supplies all item, recipe, venture,
// collectable and FC-project definitions. IDs were verified against the
// bundled English catalog: Maple Lumber/Log, Copper Ore, shard ventures,
// bronze-weapon leves, Mythrite/Titanium scrip turn-ins, and level 2/3
// Aetherial Wheel Stands with their complete direct material sets.
const ids = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  1602, 1603, 1604, 1605, 1609, 5059, 5060, 5061, 5064, 5065, 5066, 5067,
  5094, 5106, 5186, 5289, 5361, 5366, 5376, 5380, 5480, 5506, 5507, 7017,
  7020, 7606, 9360, 9462, 9463, 12518, 12519, 12525, 12562, 12563, 12565,
  12581, 12582, 12602];

function marketFixture() {
  const now = Date.now();
  const naive = ms => new Date(ms).toISOString().slice(0, -1);
  const recent = { sales: ids.map(item_id => ({ item_id, hq: false,
    sales: Array.from({ length: 6 }, (_, index) => ({
      price_per_unit: 800 + (5 - index) * 40,
      sale_date: naive(now - index * 4 * 3600000),
    })),
  })) };
  const stats = { stats: ids.flatMap(item_id => [false, true].map(hq => ({
    item_id, hq, min_price: [9462, 9463].includes(item_id) ? 600000 : (hq ? 1200 : 600), median_price: [9462, 9463].includes(item_id) ? 900000 : (hq ? 1500 : 900),
    avg_price: hq ? 1700 : 1000, num_sold: 70, last_sold_unix: Math.floor(now / 1000),
    units_sold: 210, gil_volume: hq ? 336000 : 199500, vwap: hq ? 1600 : 950, sales_per_day: 10, confidence: 'high',
  }))) };
  const listings = world => ({ cheapest_listings: ids.map(item_id => ({
    item_id, hq: false, cheapest_price: [9462, 9463].includes(item_id) ? 1000000 : (world === 'Gilgamesh' ? 400 : 100),
    world_id: world === 'Gilgamesh' ? 63 : 79,
  })) });
  const hits = new Map();
  function reply(request) {
    const url = new URL(request.url());
    const [kind, world] = url.pathname.replace(/^\/api\/v1\//, '').split('/');
    const worldId = decodeURIComponent(world || '') === 'Gilgamesh' ? 63 : 79;
    let body;
    if (kind === 'cheapest') body = listings(decodeURIComponent(world));
    if (kind === 'recentSales') body = recent;
    if (kind === 'sale_stats') {
      const multiplier = url.searchParams.get('window') === '30' ? 2 : 1;
      body = { stats: stats.stats.map(row => ({ ...row, min_price: row.min_price * multiplier,
        median_price: row.median_price * multiplier, avg_price: row.avg_price * multiplier })) };
    }
    if (kind === 'sparklines') {
      const items = JSON.parse(request.postData() || '{}').items || [];
      body = { world_id: worldId, series: items.map(([item_id, hq]) => ({
        item_id, hq, world_id: worldId, points: [600, 700, 750, 800, 850, 900], first_price: 600, last_price: 900,
      })) };
    }
    if (kind === 'resale_quality') {
      const items = JSON.parse(request.postData() || '{}').items || [];
      body = { world_id: worldId, window_days: 30, rows: items.map(([item_id, hq]) => ({
        item_id, hq, world_id: worldId, window_days: 30, vwap: 950, sample_size: 300,
        sales_per_day: 10, confidence_band: 'high', launder_suspicion: 0,
      })) };
    }
    if (kind === 'trends') {
      // Trends candidates: three cleaned-sample rows whose order differs by
      // units, VWAP and listing so every legacy sort token is observable.
      // VWAP scales with the window so a window change is visible in a cell;
      // the suspicious toggle adds a launder-flagged fourth row.
      const window = Number(url.searchParams.get('window') || 30);
      const scale = window / 30;
      const rows = [
        { item_id: 5, hq: false, price: 400, units: 900, sales: 60, vwap: 950, band: 'high' },
        { item_id: 6, hq: true, price: 1200, units: 300, sales: 30, vwap: 1600, band: 'medium' },
        { item_id: 7, hq: false, price: 100, units: 2500, sales: 150, vwap: 120, band: 'low' },
      ];
      if (url.searchParams.get('show_suspicious') === '1') {
        rows.push({ item_id: 9462, hq: false, price: 1000000, units: 5000, sales: 400, vwap: 900000, band: 'unusable' });
      }
      body = { items: rows.map(row => {
        const vwap = Math.round(row.vwap * scale);
        return {
          item_id: row.item_id, hq: row.hq, price: row.price, world_id: worldId,
          average_sale_price: vwap, sales_per_week: row.sales / window * 7,
          vwap_30d: window === 30 ? vwap : 0, price_percentile_30d: 50,
          confidence_band: row.band, sample_size_30d: row.sales, launder_suspicion: row.band === 'unusable' ? 0.9 : 0,
          window_days: window, vwap_window: vwap, sales_in_window: row.sales,
          unit_volume_window: row.units, gil_volume_window: row.units * vwap,
          sales_per_day: row.sales / window,
          pct_change_window: vwap > 0 ? (row.price - vwap) / vwap * 100 : 0,
          sparkline_24h: Array.from({ length: 24 }, (_, hour) => vwap + hour * 5),
        };
      }) };
    }
    if (body === undefined) return null;
    hits.set(kind, (hits.get(kind) || 0) + 1);
    return { status: 200, contentType: 'application/json', body: JSON.stringify(body) };
  }
  return { reply, hits };
}

module.exports = { marketFixture };
