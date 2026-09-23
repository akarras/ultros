// Undercut pressure renders at world scope and is never requested at datacenter scope.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const ITEM = process.env.PRESSURE_ITEM || '5057';
const WORLD = process.env.PRESSURE_WORLD || 'Gilgamesh';
const DC = process.env.PRESSURE_DC || 'Aether';

const pressureBody = (url) => {
  const bucket = Number(url.searchParams.get('bucket') || 3600);
  const to = Number(url.searchParams.get('to') || Math.floor(Date.now() / 1000));
  const from = Number(url.searchParams.get('from') || to - 48 * 3600);
  const first = Math.floor(from / bucket) * bucket;
  const buckets = [];
  for (let start = first, i = 0; start < to; start += bucket, i++) {
    const war = i % 7 === 3;
    buckets.push({ start, trims: war ? 2 : 1, cuts: war ? 6 : 1, sellers: war ? 3 : 1,
      floor_open: 1000, floor_close: war ? 900 : 1000, state: war ? 'war' : 'churn' });
  }
  return { world_id: 0, from, to, bucket_seconds: bucket, coverage_from: first, baseline: 2, buckets, wars: [],
    summary: { floor_trend_24h: -0.12, war: { kind: 'active' },
      last_war: { start: to - 3600, end: to, undercuts: 8, sellers: 3, floor_change: -0.1 },
      contested_share: 1, typical_undercuts_per_hour: 2, floor_holds_median_secs: 2100,
      episodes_left: 5, episodes_undercut: 3 } };
};

const naive = (s) => new Date(s * 1000).toISOString().slice(0, 19);

// Local ClickHouse `sales` can't load on this machine, so `/api/v1/price_series/` 500s and the
// item page never gets far enough to request undercut_pressure (it waits on the series for
// bucket_seconds first). Stub it deterministically; this probe doesn't depend on real sales data.
const priceSeriesBody = (url) => {
  const to = Number(url.searchParams.get('to') || Math.floor(Date.now() / 1000));
  const from = Number(url.searchParams.get('from') || to - 7 * 24 * 3600);
  const bucket = 86400;
  const first = Math.floor(from / bucket) * bucket;
  const buckets = [];
  for (let start = first; start < to; start += bucket) {
    buckets.push({ ts: naive(start), open: 1000, high: 1100, low: 900, close: 1000,
      gil: 10000, units: 10, sales: 3, p25: 950, p50: 1000, p75: 1050 });
  }
  const last = buckets.length ? buckets[buckets.length - 1].ts : naive(first);
  return { bucket_seconds: bucket, group: 'world', from: naive(first), to: last,
    series: [{ id: 63, buckets }], raw: null };
};

async function main() {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(30000);
  await page.setCookie({ name: 'HIDE_ADS', value: 'true', url: BASE });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  const requests = [];
  const seriesRequests = [];
  await page.setRequestInterception(true);
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.pathname.startsWith('/api/v1/undercut_pressure/')) {
      requests.push(url.pathname + url.search);
      return request.respond({ status: 200, contentType: 'application/json', body: JSON.stringify(pressureBody(url)) });
    }
    if (url.pathname.startsWith('/api/v1/price_series/')) {
      seriesRequests.push(url.pathname + url.search);
      return request.respond({ status: 200, contentType: 'application/json', body: JSON.stringify(priceSeriesBody(url)) });
    }
    return request.continue();
  });

  await page.goto(`${BASE}/item/${WORLD}/${ITEM}`, { waitUntil: 'networkidle2' });
  await page.waitForSelector('.mh-pressure-pane svg');
  await page.waitForSelector('.mh-pressure-stats');
  const cards = await page.$$eval('.mh-pressure-stats .mh-stat strong', els => els.map(e => e.textContent.trim()));
  assert.equal(cards.length, 4);
  assert.equal(cards[0], '-12.0%');
  assert.equal(cards[3], '63% / 37%');
  assert.ok(requests.some(r => r.startsWith(`/api/v1/undercut_pressure/${WORLD}/${ITEM}?`) && r.includes('bucket=')), requests.join('\n'));

  const before = requests.length;
  await page.goto(`${BASE}/item/${DC}/${ITEM}`, { waitUntil: 'networkidle2' });
  assert.equal(await page.$('.mh-pressure-pane'), null, 'no pane at datacenter scope');
  assert.equal(await page.$('.mh-pressure-stats'), null, 'no cards at datacenter scope');
  assert.equal(requests.length, before, `no pressure request at DC scope: ${requests.slice(before).join(', ')}`);

  assert.deepEqual(errors, []);
  await browser.close();
  console.log('undercut-pressure: ok');
}
main().catch(e => { console.error(e); process.exit(1); });
