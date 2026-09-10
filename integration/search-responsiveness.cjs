// Real hydrated search UI, controlled responses (including an intentionally
// uncooperative transport) to exercise latest-query-wins independently of abort.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const INPUT = '.search-overlay input[role="combobox"]';

async function main() {
  const browser = await puppeteer.launch({headless: true, args: ['--no-sandbox']});
  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(90000);
    const errors = [];
    page.on('pageerror', error => {
      // Match the route runner's allowance for AdSense's headless-only error.
      if (!error.message.includes('pagead2.googlesyndication.com')) errors.push(error.message);
    });
    await page.evaluateOnNewDocument(() => {
      window.__searchHydrated = false;
      window.addEventListener('ultros:hydrated', () => { window.__searchHydrated = true; });
      window.__searchRequests = [];
      const original = window.fetch;
      window.fetch = function(input, init) {
        const url = new URL(input instanceof Request ? input.url : input, location.href);
        if (url.pathname !== '/api/v1/search') return original.call(this, input, init);
        const signal = init?.signal || input.signal;
        return new Promise(resolve => {
          // Deliberately resolve even after abort: the UI must reject stale results.
          window.__searchRequests.push({query: url.searchParams.get('q'), signal,
            finish: (title, status = 200) => resolve(new Response(JSON.stringify(
              title ? [{score: 1, title, result_type: 'recipe', url: '/recipe/' + encodeURIComponent(title),
                icon_id: null, category: 'CRP Lv. 1'}] : []
            ), {status, headers: {'content-type': 'application/json'}}))});
        });
      };
    });
    await page.goto(BASE + '/about', {waitUntil: 'networkidle2'});
    await page.waitForFunction(() => window.__searchHydrated);
    await page.keyboard.down('Control');
    await page.keyboard.press('KeyK');
    await page.keyboard.up('Control');
    await page.waitForSelector(INPUT, {visible: true});
    async function edit(text) {
      await page.$eval(INPUT, (input, text) => {
        input.value = text;
        input.dispatchEvent(new Event('input', {bubbles: true}));
      }, text);
      // Drain tasks without waiting out the old 300 ms debounce.
      await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
    }
    const count = () => page.evaluate(() => window.__searchRequests.length);
    const finish = (index, title) => page.evaluate(({index, title}) => {
      window.__searchRequests[index].finish(title);
    }, {index, title});
    const results = () => page.$$eval('#search-results [role="option"]', rows => rows.map(row => row.textContent));
    async function expectResult(title) {
      await page.waitForFunction(title => document.querySelector('#search-results')?.textContent.includes(title), {}, title);
    }
    await edit('alpha');
    assert.equal(await count(), 1, 'first query starts eagerly');
    await finish(0, 'Alpha recipe');
    await expectResult('Alpha recipe');
    await edit('beta');
    assert.equal(await count(), 2);
    assert((await results()).some(text => text.includes('Alpha recipe')), 'keep previous results during requests');
    await edit('gamma');
    assert.equal(await count(), 3);
    assert(await page.evaluate(() => window.__searchRequests[1].signal.aborted), 'superseded fetch is aborted');
    await finish(2, 'Gamma recipe');
    await expectResult('Gamma recipe');
    await finish(1, 'Stale beta recipe');
    await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
    assert(!(await results()).some(text => text.includes('Stale')), 'late result cannot replace newest');
    await edit(' ALPHA ');
    await expectResult('Alpha recipe');
    assert.equal(await count(), 3, 'revisited normalized query uses cache');
    await edit('empty');
    await finish(3, null);
    await page.waitForFunction(selector => document.querySelector(selector).getAttribute('aria-busy') === 'false', {}, INPUT);
    await edit('alpha');
    await edit('empty');
    assert.equal(await count(), 4, 'zero results are cached too');
    await edit('pending');
    await edit('');
    assert.equal(await page.$eval(INPUT, input => input.getAttribute('aria-busy')), 'false');
    await finish(4, 'Cleared stale recipe');
    await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
    assert.deepEqual(await results(), [], 'clear invalidates outstanding requests');
    await edit('closing');
    await page.keyboard.press('Escape');
    await page.waitForSelector(INPUT, {hidden: true});
    assert(await page.evaluate(() => window.__searchRequests[5].signal.aborted), 'unmount aborts request');
    await finish(5, 'Disposed stale recipe');
    await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 300)));
    assert.deepEqual(errors, [], 'no disposed-signal errors');
    await page.keyboard.down('Control');
    await page.keyboard.press('KeyK');
    await page.keyboard.up('Control');
    await page.waitForSelector(INPUT, {visible: true});
    await edit('retry');
    await page.evaluate(() => window.__searchRequests[6].finish(null, 503));
    await page.waitForFunction(selector => document.querySelector(selector).getAttribute('aria-busy') === 'false', {}, INPUT);
    await edit('retry');
    assert.equal(await count(), 8, 'HTTP errors must not be cached as empty results');
    await finish(7, 'Recovered recipe');
    await expectResult('Recovered recipe');
    assert.deepEqual(errors, []);
    console.log('Eager search, stale responses, cancellation, cache, clear, unmount and retry passed.');
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
