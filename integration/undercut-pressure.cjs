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

async function main() {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(30000);
  await page.setCookie({ name: 'HIDE_ADS', value: 'true', url: BASE });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  const requests = [];
  await page.setRequestInterception(true);
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.pathname.startsWith('/api/v1/undercut_pressure/')) {
      requests.push(url.pathname + url.search);
      return request.respond({ status: 200, contentType: 'application/json', body: JSON.stringify(pressureBody(url)) });
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
