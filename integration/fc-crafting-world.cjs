// Route, cookie, hydrated picker and the three market sources must agree.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const PICKER = '[data-testid="fc-world-picker"]';
const QUERY = '?min-sales=0&cost-basis=sale-avg&probe=ore%20%26%20crystals%3D1%20%2B%20HQ%2F%E6%9D%90%E6%96%99#scope';

async function main() {
  const browser = await puppeteer.launch({headless: true, args: ['--no-sandbox']});
  try {
    const page = await browser.newPage();
    const errors = [];
    const requests = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {
      if (message.type() === 'error' && !/favicon|ERR_BLOCKED_BY_CLIENT|net::ERR_ABORTED/.test(message.text())) errors.push(message.text());
    });
    page.on('request', request => {
      const path = new URL(request.url()).pathname;
      if (/\/api\/v1\/(cheapest|recentSales|sale_stats)\//.test(path)) requests.push(decodeURIComponent(path));
    });
    await page.setCookie({name: 'HOME_WORLD', value: 'Cerberus', url: BASE}, {name: 'HIDE_ADS', value: 'true', url: BASE});
    await page.evaluateOnNewDocument(() => window.addEventListener('ultros:hydrated', () => window.__hydrated = true));
    async function ready(world, region) {
      await page.waitForFunction(() => window.__hydrated, {timeout: 90000});
      await page.waitForFunction((selector, world, region) =>
        decodeURIComponent(location.pathname) === `/fc-crafting-analyzer/${world}` &&
        document.querySelector(selector)?.textContent.includes(world) &&
        document.querySelector('[data-testid="fc-market-scope"]')?.textContent.includes(region),
      {timeout: 90000}, PICKER, world, region);
      const url = new URL(page.url());
      assert.equal(decodeURIComponent(url.pathname), `/fc-crafting-analyzer/${world}`);
      assert.equal(url.searchParams.get('probe'), 'ore & crystals=1 + HQ/材料');
      assert.equal(url.hash, '#scope');
    }
    async function choose(world, region) {
      requests.length = 0;
      const input = await page.$(`${PICKER} input[role="combobox"]`);
      await input.click();
      await input.type(world);
      await page.waitForFunction(world => [...document.querySelectorAll('[role="option"]')].some(el => el.textContent.trim() === world), {}, world);
      await page.evaluate(world => [...document.querySelectorAll('[role="option"]')].find(el => el.textContent.trim() === world).click(), world);
      await ready(world, region);
      await page.waitForFunction(() => !!document.querySelector('.virtual-grid'), {timeout: 90000});
      // Statistics are fetched after table mount, so wait for the request event too.
      const deadline = Date.now() + 90000;
      while (!requests.includes(`/api/v1/sale_stats/${region}`) && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 100));
      assert(requests.includes(`/api/v1/recentSales/${world}`), `recent sales: ${requests}`);
      assert(requests.includes(`/api/v1/sale_stats/${region}`), `regional statistics: ${requests}`);
      assert(requests.filter(path => path.includes('/cheapest/')).every(path => path === `/api/v1/cheapest/${region}`), `regional ingredients: ${requests}`);
      assert(requests.filter(path => path.includes('/recentSales/')).every(path => path === `/api/v1/recentSales/${world}`), `no cookie-world sales: ${requests}`);
      return [...requests];
    }
    // A separate non-JS page observes the server-rendered choice directly.
    const ssr = await browser.newPage();
    await ssr.setJavaScriptEnabled(false);
    await ssr.goto(`${BASE}/fc-crafting-analyzer/Gilgamesh${QUERY}`, {waitUntil: 'domcontentloaded', timeout: 90000});
    assert((await ssr.$eval(PICKER, el => el.textContent)).includes('Gilgamesh'));
    assert((await ssr.$eval('[data-testid="fc-market-scope"]', el => el.textContent)).includes('North-America'));
    await ssr.close();
    await page.goto(`${BASE}/fc-crafting-analyzer/Gilgamesh${QUERY}`, {waitUntil: 'domcontentloaded', timeout: 90000});
    await ready('Gilgamesh', 'North-America');
    await choose('Goblin', 'North-America');
    const crossRegion = await choose('Cerberus', 'Europe');
    assert(crossRegion.includes('/api/v1/cheapest/Europe'), 'cross-region selection must refetch ingredient listings');
    await page.reload({waitUntil: 'domcontentloaded', timeout: 90000});
    await ready('Cerberus', 'Europe');
    await page.goBack({waitUntil: 'domcontentloaded', timeout: 90000});
    await ready('Goblin', 'North-America');
    await page.goBack({waitUntil: 'domcontentloaded', timeout: 90000});
    await ready('Gilgamesh', 'North-America');
    await page.goForward({waitUntil: 'domcontentloaded', timeout: 90000});
    await ready('Goblin', 'North-America');
    await page.goto(`${BASE}/fc-crafting-analyzer${QUERY}`, {waitUntil: 'domcontentloaded', timeout: 90000});
    await ready('Cerberus', 'Europe');
    await page.deleteCookie({name: 'HOME_WORLD', url: BASE});
    await page.goto(`${BASE}/fc-crafting-analyzer${QUERY}`, {waitUntil: 'domcontentloaded', timeout: 90000});
    await page.waitForFunction(() => window.__hydrated, {timeout: 90000});
    await choose('Gilgamesh', 'North-America');
    await choose('红玉海', '中国');
    assert.equal(new URL(page.url()).pathname, `/fc-crafting-analyzer/${encodeURIComponent('红玉海')}`);
    await page.reload({waitUntil: 'domcontentloaded', timeout: 90000});
    await ready('红玉海', '中国');
    assert.deepEqual(errors, []);
    console.log('PASS: FC world route/cookie precedence, regional pricing/statistics, world sales, reload/history, encoded query and cookie fallback.');
  } finally {
    await browser.close();
  }
}
main().catch(error => {console.error(error); process.exitCode = 1;});
