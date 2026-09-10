// Real browser regression: node integration/list-companion.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');

(async () => {
  const { default: puppeteer } = await import('puppeteer');
  const source = fs.readFileSync(path.join(__dirname, '../ultros/static/list-companion.mjs'));
  const server = http.createServer((request, response) => {
    response.setHeader('Content-Type', request.url === '/companion.mjs' ? 'text/javascript' : 'text/html');
    response.end(request.url === '/companion.mjs' ? source : '<!doctype html><button id="open">Open</button>');
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  let browser;
  let stage = 'launch browser';
  try {
    browser = await puppeteer.launch({ headless: true, timeout: 120000 });
    stage = 'create opener page';
    const page = await browser.newPage();
    stage = 'load opener page';
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.evaluate(async () => {
      window.api = await import('/companion.mjs');
      window.events = [];
      window.data = { title: '<img src=x onerror=alert(1)>', world: 'Gilgamesh', progress: '0 of 3', canEdit: true, hasNext: true,
        rows: [{ key: 'item:1', name: '<script>unsafe()</script>', quality: 'HQ', quantity: 3, cost: 100, done: false }] };
      // Explicitly exercise the unsupported-browser fallback, independently of host PiP support.
      Object.defineProperty(window, 'documentPictureInPicture', { value: undefined, configurable: true });
      document.querySelector('#open').onclick = () => api.openCompanion(JSON.stringify(data), (...args) => events.push(args));
    });
    stage = 'open companion popup';
    const popupPromise = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('No popup event received')), 30000);
      page.once('popup', (opened) => { clearTimeout(timer); resolve(opened); });
    });
    await page.click('#open');
    const popup = await popupPromise;
    stage = 'render companion controls';
    await popup.waitForSelector('[data-testid="companion-buy"]');
    assert.equal(await popup.$eval('h1', (node) => node.textContent), '<img src=x onerror=alert(1)>');
    assert.equal(await popup.$$eval('img,script', (nodes) => nodes.length), 0);
    await popup.$eval('input', (node) => { node.value = '2'; });
    await popup.click('[data-testid="companion-buy"]');
    assert.deepEqual(await page.evaluate(() => events), [['bought', 'item:1', 2]]);
    await popup.$eval('input', (node) => { node.value = '4'; });
    await popup.click('[data-testid="companion-buy"]');
    assert.equal(await page.evaluate(() => events.length), 1);
    assert.match(await popup.$eval('#notice', (node) => node.textContent), /whole quantity/);
    await popup.click('[data-testid="companion-next"]');
    await popup.click('[data-testid="companion-undo"]');
    assert.deepEqual(await page.evaluate(() => events.slice(1)), [['next', '', 0], ['undo', '', 0]]);
    await page.evaluate(() => { data.rows[0].canBuy = false; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.$eval('[data-testid="companion-buy"]', (node) => node.disabled), true);
    await popup.click('[data-testid="companion-buy"]');
    assert.equal(await page.evaluate(() => events.length), 3);
    await page.evaluate(() => { data.rows[0].done = true; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.$$eval('[data-testid="companion-buy"]', (nodes) => nodes.length), 0);
    await page.evaluate(() => api.closeCompanion());
    await new Promise((resolve) => popup.isClosed() ? resolve() : popup.once('close', resolve));
    assert.equal(popup.isClosed(), true);
    console.log('Shopping companion browser regression passed.');
  } catch (error) {
    error.message = `${stage}: ${error.message}`;
    throw error;
  } finally {
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
