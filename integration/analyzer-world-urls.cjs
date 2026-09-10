// Canonical analyzer worlds, legacy bookmarks, saved views, and browser history.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const {marketFixture} = require('./shared-analyzer-market-fixture.cjs');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const TOOLS = ['recipe-analyzer', 'venture-analyzer', 'leve-analyzer', 'scrip-sources'];
const PICKER = '[data-testid="analyzer-world-picker"]';
const PROBE = 'ore & crystals=1 + HQ/材料';
const query = () => new URLSearchParams({v: '1', 'min-sales': '0', 'buy-scope': 'region', 'cost-basis': 'sale-avg', probe: PROBE});
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));

async function main() {
  const browser = await puppeteer.launch({headless: true, args: ['--no-sandbox']});
  let activePage;
  try {
    for (const tool of TOOLS) {
      const page = await browser.newPage();
      activePage = page;
      await page.setViewport({width: 1600, height: 1000});
      page.setDefaultTimeout(90000);
      const errors = [], requests = [];
      const fixture = marketFixture();
      await page.setRequestInterception(true);
      page.on('request', request => {
        const path = decodeURIComponent(new URL(request.url()).pathname);
        if (/\/api\/v1\/(cheapest|recentSales|sale_stats)\//.test(path)) requests.push(path);
        const response = fixture.reply(request);
        return response ? request.respond(response) : request.continue();
      });
      page.on('pageerror', e => { errors.push(e.message); console.error('Page error at', page.url(), e.message); });
      page.on('console', m => {
        if (m.type() === 'error' && !/favicon|ERR_BLOCKED_BY_CLIENT|net::ERR_ABORTED/.test(m.text())) errors.push(m.text());
      });
      await page.setCookie({name: 'HOME_WORLD', value: 'Cerberus', url: BASE}, {name: 'HIDE_ADS', value: 'true', url: BASE});
      await page.evaluateOnNewDocument(tool => {
        window.__initialHistoryLength = history.length;
        window.addEventListener('ultros:hydrated', () => window.__hydrated = true);
        localStorage.setItem(`ultros.grid.${tool}-grid.views`, JSON.stringify([
          {name: 'Legacy world view', query: '?world=Goblin&v=1&min-sales=0&profit=123&lang=en'},
        ]));
      }, tool);
      async function ready(world, checkProbe = true) {
        await page.waitForFunction(() => window.__hydrated);
        await page.waitForFunction((tool, world, picker) =>
          decodeURIComponent(location.pathname) === `/${tool}/${world}` &&
          !new URL(location.href).searchParams.has('world') &&
          document.querySelector(picker)?.textContent.includes(world), {}, tool, world, PICKER);
        if (checkProbe) {
          assert.equal(new URL(page.url()).searchParams.get('probe'), PROBE);
          assert.equal(new URL(page.url()).hash, '#scope');
        }
      }
      async function open(path, world) {
        console.log(`CHECK ${tool}: open ${world}`);
        const response = await page.goto(`${BASE}/${path}#scope`, {waitUntil: 'domcontentloaded'});
        assert(response.ok(), `${tool}: SSR ${response.status()}`);
        await ready(world);
        assert.equal(await page.evaluate(() => history.length), await page.evaluate(() => window.__initialHistoryLength), `${tool}: canonicalization replaces history`);
      }
      async function choose(world, region) {
        console.log(`CHECK ${tool}: choose ${world}`);
        const previous = decodeURIComponent(new URL(page.url()).pathname).split('/').pop();
        const previousRegion = {Gilgamesh: 'North-America', Goblin: 'North-America', Cerberus: 'Europe', '红玉海': '中国'}[previous] || 'North-America';
        requests.length = 0;
        await page.click(`${PICKER} input[role="combobox"]`);
        await page.type(`${PICKER} input[role="combobox"]`, world);
        await page.waitForFunction(world => [...document.querySelectorAll('[role="option"]')].some(e => e.textContent.trim() === world), {}, world);
        await page.evaluate(world => [...document.querySelectorAll('[role="option"]')].find(e => e.textContent.trim() === world).click(), world);
        await ready(world);
        await page.waitForSelector('.virtual-grid');
        const stats = `/api/v1/sale_stats/${region}`;
        const deadline = Date.now() + 90000;
        while (previousRegion !== region && !requests.includes(stats) && Date.now() < deadline) await pause(100);
        if (previousRegion !== region) assert(requests.includes(stats), `${tool}: expected ${stats}, got ${requests}`);
        if (tool === 'recipe-analyzer') {
          assert(requests.includes(`/api/v1/sale_stats/${world}`), `${tool}: sell-world statistics ${requests}`);
        } else if (tool !== 'scrip-sources') {
          assert(requests.includes(`/api/v1/recentSales/${world}`), `${tool}: recent sales ${requests}`);
          assert(requests.filter(p => p.includes('/recentSales/')).every(p => p.endsWith(`/${world}`)), `${tool}: stale sales ${requests}`);
        }
        if (tool !== 'recipe-analyzer') {
          assert((await page.$eval('[data-testid="analyzer-market-scope"]', e => e.textContent)).includes(region));
          assert(requests.filter(p => p.includes('/cheapest/')).every(p => p.endsWith(`/${region}`)), `${tool}: ingredients remain regional`);
        }
        return [...requests];
      }
      // SSR and hydration both use the path, even against two conflicting fallbacks.
      console.log(`CHECK ${tool}: SSR path precedence`);
      const ssr = await browser.newPage();
      await ssr.setJavaScriptEnabled(false);
      await ssr.goto(`${BASE}/${tool}/Gilgamesh?world=Goblin&${query()}`, {waitUntil: 'domcontentloaded'});
      assert((await ssr.$eval(PICKER, e => e.textContent)).includes('Gilgamesh'), `${tool}: SSR path precedence`);
      assert((await ssr.$eval('meta[property="og:image"]', e => e.content)).includes(`/tool/${tool}`), `${tool}: world route keeps its social card`);
      await ssr.close();
      await open(`${tool}/Gilgamesh?world=Goblin&${query()}`, 'Gilgamesh');
      await choose('Goblin', 'North-America');
      const crossRegion = await choose('Cerberus', 'Europe');
      assert(crossRegion.includes('/api/v1/cheapest/Europe'), `${tool}: crossing region refreshes ingredient listings`);
      await page.reload({waitUntil: 'domcontentloaded'});
      await ready('Cerberus');
      await page.goBack({waitUntil: 'domcontentloaded'}); await ready('Goblin');
      await page.goBack({waitUntil: 'domcontentloaded'}); await ready('Gilgamesh');
      await page.goForward({waitUntil: 'domcontentloaded'}); await ready('Goblin');
      await open(`${tool}?world=Gilgamesh&${query()}`, 'Gilgamesh');
      await open(`${tool}?${query()}`, 'Cerberus');
      await choose('红玉海', '中国');
      await page.reload({waitUntil: 'domcontentloaded'}); await ready('红玉海');
      // Filter edits replace the current entry and retain both world and scroll.
      await page.waitForSelector('.virtual-grid');
      const column = tool === 'scrip-sources' ? 'scrip-type' : 'profit';
      const heading = `.virtual-grid-heading[data-column="${column}"]`;
      await page.waitForSelector(heading);
      await page.$eval(heading, e => e.scrollIntoView({block: 'center', inline: 'nearest'}));
      await page.click(heading, {button: 'right'});
      const form = tool === 'recipe-analyzer'
        ? '.grid-column-filter[data-filter="profit"]'
        : `.grid-column-filter[data-metric-filter="${column}"]`;
      await page.waitForSelector(form);
      const value = tool === 'scrip-sources' ? 'OrangeCrafters' : '0';
      if (tool === 'scrip-sources') await page.select(`${form} select[aria-label="Value"]`, value);
      else { await page.click(`${form} input`, {count: 3}); await page.type(`${form} input`, value); }
      const beforeEdit = await page.evaluate(() => ({length: history.length, scroll: scrollY}));
      await page.$eval(`${form} button[type="submit"]`, e => e.click());
      console.log(`CHECK ${tool}: filter applied`, page.url());
      await page.waitForFunction((tool, column, value) => {
        const query = new URL(location.href).searchParams;
        return (tool === 'recipe-analyzer' ? query.get('profit') : JSON.parse(query.get('gf') || '{}')[column]?.value) === value;
      }, {}, tool, column, value);
      await pause(200);
      assert.deepEqual(await page.evaluate(() => ({length: history.length, scroll: scrollY})), beforeEdit, `${tool}: filter edits replace without scrolling`);
      await ready('红玉海');
      await page.keyboard.press('Escape');
      // Saved views created before path worlds must keep the current world.
      await page.waitForSelector('.virtual-grid');
      await page.click('[aria-label="Views"]');
      await page.waitForFunction(() => [...document.querySelectorAll('a')].some(a => a.textContent.trim() === 'Legacy world view'));
      const href = await page.evaluate(() => [...document.querySelectorAll('a')].find(a => a.textContent.trim() === 'Legacy world view').href);
      assert.equal(decodeURIComponent(new URL(href).pathname), `/${tool}/红玉海`);
      assert.equal(new URL(href).searchParams.has('world'), false);
      await page.evaluate(() => [...document.querySelectorAll('a')].find(a => a.textContent.trim() === 'Legacy world view').click());
      await ready('红玉海', false);
      assert.equal(new URL(page.url()).searchParams.get('profit'), '123');
      await page.deleteCookie({name: 'HOME_WORLD', url: BASE});
      await page.goto(`${BASE}/${tool}?${query()}#scope`, {waitUntil: 'domcontentloaded'});
      await page.waitForFunction(() => window.__hydrated);
      assert.equal(new URL(page.url()).pathname, `/${tool}`, `${tool}: no cookie leaves bare route selectable`);
      await choose('Gilgamesh', 'North-America');
      assert.deepEqual(errors, [], `${tool}: browser errors`);
      await page.close();
      console.log(`PASS ${tool}: path/query/cookie precedence, regional ingredients, sales scopes, encoded filters, reload/history, legacy saved views`);
    }
  } catch (error) {
    if (activePage && !activePage.isClosed()) console.error('Failure state:', activePage.url(), await activePage.$$eval('.grid-column-filter input', inputs => inputs.map(i => ({value: i.value, valid: i.validity.valid}))));
    throw error;
  } finally { await browser.close(); }
}
main().catch(error => {console.error(error); process.exitCode = 1;});
