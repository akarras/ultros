// Real browser regression: node integration/list-companion.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');

(async () => {
  const { default: puppeteer } = await import('puppeteer');
  const source = fs.readFileSync(path.join(__dirname, '../ultros/static/list-companion.mjs'));
  // The opener carries the app's theme: a linked stylesheet, an inline style and
  // the `data-theme` / `data-palette` attributes the real page sets on <html>.
  const opener = '<!doctype html><html data-theme="light" data-palette="ascian"><head><link rel="stylesheet" href="/theme.css"><style id="inline-probe">.probe{color:red}</style></head><body><button id="open">Open</button></body></html>';
  const server = http.createServer((request, response) => {
    const kind = { '/companion.mjs': 'text/javascript', '/theme.css': 'text/css' }[request.url] ?? 'text/html';
    response.setHeader('Content-Type', kind);
    response.end({ '/companion.mjs': source, '/theme.css': ':root{--companion-probe:1}' }[request.url] ?? opener);
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
      window.data = { title: '<img src=x onerror=alert(1)>', world: 'Gilgamesh', progress: '0 of 3', canEdit: true, canUndoPurchase: true, hasNext: true,
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
    stage = 'companion adopts the opener theme';
    // Same-origin windows can load the app stylesheet directly, so the popout
    // uses the real theme tokens and component classes instead of its own palette.
    await popup.waitForFunction(() => getComputedStyle(document.documentElement).getPropertyValue('--companion-probe').trim() === '1');
    assert.equal(await popup.$$eval('link[rel="stylesheet"]', (nodes) => nodes.filter((node) => node.href.endsWith('/theme.css')).length), 1, 'opener stylesheets are linked once');
    assert.equal(await popup.$$eval('style#inline-probe', (nodes) => nodes.length), 1, 'inline opener styles are cloned');
    assert.deepEqual(await popup.evaluate(() => [document.documentElement.dataset.theme, document.documentElement.dataset.palette]), ['light', 'ascian']);
    await page.evaluate(() => { document.documentElement.dataset.theme = 'dark'; document.documentElement.dataset.palette = 'ultros'; });
    await popup.waitForFunction(() => document.documentElement.dataset.theme === 'dark' && document.documentElement.dataset.palette === 'ultros');
    assert.equal(await popup.$$eval('article', (nodes) => nodes.map((node) => node.className)).then((classes) => classes.every((name) => name.split(' ').includes('card'))), true, 'rows use the app card surface');
    assert.equal(await popup.$eval('[data-testid="companion-buy"]', (node) => node.classList.contains('btn-primary')), true);
    assert.equal(await popup.$eval('[data-testid="companion-next"]', (node) => node.classList.contains('btn-secondary')), true);
    assert.equal(await popup.$eval('input', (node) => node.classList.contains('input')), true);
    stage = 'clipboard control matches the list rows';
    // The list rows show the Clipboard icon control beside each name: an
    // icon-only button whose icon flips to the checkmark once copied.
    const copy = '[data-testid="companion-copy"]';
    assert.equal(await popup.$eval(copy, (node) => node.classList.contains('clipboard') && node.querySelector('svg') !== null && node.textContent.trim() === ''), true, 'copy is an icon-only clipboard button');
    assert.equal(await popup.$eval(copy, (node) => node.getAttribute('aria-label')), 'Copy name');
    assert.equal(await popup.$eval(copy, (node) => node.closest('article').querySelector('h2 + *') === node), true, 'clipboard sits beside the item name');
    assert.equal(await popup.$eval(copy, (node) => node.dataset.copied), undefined);
    await popup.evaluate(() => { window.copies = []; navigator.clipboard.writeText = (text) => { window.copies.push(text); return Promise.resolve(); }; });
    await popup.click(copy);
    await popup.waitForFunction(() => document.querySelector('[data-testid="companion-copy"]').dataset.copied === 'true');
    assert.deepEqual(await popup.evaluate(() => window.copies), ['<script>unsafe()</script>']);
    assert.equal(await popup.$eval('#notice', (node) => node.textContent), 'Item name copied.');
    await page.evaluate(() => api.updateCompanion(JSON.stringify(data)));
    assert.equal(await popup.$eval(copy, (node) => node.dataset.copied), 'true', 'copied state survives an update');
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
    await page.evaluate(() => { data.canUndoPurchase = false; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.$eval('[data-testid="companion-undo"]', node => node.disabled), true);
    await popup.click('[data-testid="companion-undo"]');
    assert.equal(await page.evaluate(() => events.length), 3, 'no purchase means no undo action');
    await page.evaluate(() => { data.rows[0].canBuy = false; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.$eval('[data-testid="companion-buy"]', (node) => node.disabled), true);
    await popup.click('[data-testid="companion-buy"]');
    assert.equal(await page.evaluate(() => events.length), 3);
    // Locale changes flow through the owning view without changing action IDs.
    await page.evaluate(() => {
      data.labels = { bought: 'Acheté', undo: 'Annuler', nextWorld: 'Monde suivant', copyName: 'Copier le nom', copied: 'Nom copié.', keepOpen: 'Gardez cet onglet ouvert.' };
      data.rows[0].canBuy = true;
      data.rows[0].description = 'HQ · 3 restants · 100 gils';
      data.rows[0].quantityLabel = 'Quantité achetée';
      data.rows[0].invalidQuantity = 'Saisissez un entier de 1 à 3.';
      api.updateCompanion(JSON.stringify(data));
    });
    assert.equal(await popup.$eval('[data-testid="companion-buy"]', (node) => node.textContent), 'Acheté');
    assert.equal(await popup.$eval('input', (node) => node.getAttribute('aria-label')), 'Quantité achetée');
    assert.equal(await popup.$eval(copy, (node) => node.getAttribute('aria-label')), 'Nom copié.', 'the copied row announces its state in the active locale');
    assert.match(await popup.$eval('article', (node) => node.textContent), /100 gils/);
    await popup.$eval('input', (node) => { node.value = '4'; });
    await popup.click('[data-testid="companion-buy"]');
    assert.equal(await popup.$eval('#notice', (node) => node.textContent), 'Saisissez un entier de 1 à 3.');
    await page.evaluate(() => { data.rows[0].done = true; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.$eval('[data-testid="companion-buy"]', node => node.hidden), true);
    stage = 'keyboard state across live snapshots';
    await popup.setViewport({ width: 360, height: 480 });
    await page.evaluate(() => {
      data.labels = {};
      data.canUndoPurchase = true;
      data.rows = Array.from({ length: 12 }, (_, i) => ({ key: `stack:${i}`, name: `Item ${i}`, quantity: 5, canBuy: true }));
      api.updateCompanion(JSON.stringify(data));
    });
    const first = 'article[data-key="stack:0"]';
    await popup.focus(`${first} input`);
    await popup.$eval(`${first} input`, node => node.select());
    await popup.keyboard.type('3');
    await popup.keyboard.press('Tab');
    assert.equal(await popup.evaluate(() => document.activeElement.dataset.testid), 'companion-buy');
    for (const selector of [`${first} input`, `${first} [data-testid="companion-buy"]`, `${first} [data-testid="companion-copy"]`, '[data-testid="companion-undo"]', '[data-testid="companion-next"]']) {
      await popup.focus(selector);
      const before = await popup.evaluate(() => { window.focusBefore = document.activeElement; return window.scrollY; });
      await page.evaluate(() => { data.progress = String(Date.now()); api.updateCompanion(JSON.stringify(data)); });
      assert.equal(await popup.evaluate(() => document.activeElement === window.focusBefore), true, `${selector} keeps DOM identity and focus`);
      assert.equal(await popup.evaluate(() => window.scrollY), before, `${selector} keeps scroll position`);
      assert.equal(await popup.$eval(`${first} input`, node => node.value), '3', 'draft survives even when a button is focused');
    }
    assert.match(await popup.$eval('#window-mode', node => node.textContent), /Regular window mode/);
    assert.equal(await popup.$eval('#notice', node => node.textContent), 'Saisissez un entier de 1 à 3.', 'updates retain announcements');
    await popup.focus(`${first} [data-testid="companion-buy"]`);
    await page.evaluate(() => { data.rows[0].quantity = 2; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.$eval(`${first} input`, node => node.value), '3', 'a changed limit must not silently rewrite a draft');
    const count = await page.evaluate(() => events.length);
    await popup.keyboard.press('Enter');
    assert.equal(await page.evaluate(() => events.length), count, 'out-of-range draft cannot buy');
    assert.match(await popup.$eval('#notice', node => node.textContent), /1 to 2/);
    assert.equal(await popup.evaluate(() => document.activeElement.tagName), 'INPUT');
    await popup.keyboard.press('Backspace');
    await popup.$eval(`${first} input`, node => node.select());
    await popup.keyboard.press('Backspace');
    await page.evaluate(() => api.updateCompanion(JSON.stringify(data)));
    assert.equal(await popup.$eval(`${first} input`, node => node.value), '', 'empty in-progress input survives updates');
    await popup.keyboard.press('Escape');
    assert.equal(await popup.$eval(`${first} input`, node => node.value), '2', 'Escape restores the current default');
    await popup.$eval(`${first} input`, node => node.select());
    await popup.keyboard.type('1');
    await popup.keyboard.press('Tab');
    await popup.keyboard.press('Enter');
    assert.deepEqual(await page.evaluate(() => events.at(-1)), ['bought', 'stack:0', 1]);
    await page.evaluate(() => { data.rows[0].quantity = 1; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.$eval(`${first} input`, node => node.value), '1');
    await page.evaluate(() => { data.rows[0].done = true; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.evaluate(() => document.activeElement.dataset.testid), 'companion-copy', 'completed purchase moves focus to its Copy control');
    await page.evaluate(() => { data.rows.shift(); api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.evaluate(() => document.activeElement.closest('article').dataset.key), 'stack:1', 'removed stack chooses next row');
    await popup.focus('[data-testid="companion-next"]');
    await popup.keyboard.press('Enter');
    await page.evaluate(() => { data.world = 'Next world'; data.hasNext = false; data.rows = [{ key: 'last', name: 'Last stack', quantity: 2 }]; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.evaluate(() => document.activeElement.closest('article').dataset.key), 'last', 'last stop moves Next focus to its first row');
    const sizes = await popup.$$eval('button,input', nodes => nodes.filter(node => !node.hidden).map(node => {
      const { width, height } = node.getBoundingClientRect(); return { width, height };
    }));
    assert(sizes.every(({ width, height }) => width >= 44 && height >= 44), JSON.stringify(sizes));
    assert.equal(await popup.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true, '360px companion has no horizontal overflow');
    // Disabled controls and removed final rows also receive deterministic focus.
    await popup.focus('[data-testid="companion-undo"]');
    await page.evaluate(() => { data.canUndoPurchase = false; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.evaluate(() => document.activeElement.dataset.testid), 'companion-copy');
    await page.evaluate(() => { data.rows = []; api.updateCompanion(JSON.stringify(data)); });
    assert.equal(await popup.evaluate(() => document.activeElement.tagName), 'H1');
    await page.evaluate(() => api.closeCompanion());
    await new Promise((resolve) => popup.isClosed() ? resolve() : popup.once('close', resolve));
    assert.equal(popup.isClosed(), true);
    stage = 'Shop focus successors';
    await page.evaluate(() => {
      const root = document.createElement('section');
      root.id = 'shop';
      root.innerHTML = '<h3 tabindex="-1" data-testid="shop-stop-title">World</h3><div data-shop-key="one"><button>Copy one</button><input><button id="bought">Bought</button></div><div data-shop-key="two"><button>Copy two</button><input></div><button id="outside-shop-action">Next world</button>';
      document.body.append(root);
      window.stopWatching = api.watchShopFocus(root);
      document.querySelector('#bought').focus();
      document.querySelector('#bought').disabled = true;
    });
    await page.waitForFunction(() => document.activeElement.textContent === 'Copy one');
    await page.evaluate(() => document.querySelector('[data-shop-key="one"]').remove());
    await page.waitForFunction(() => document.activeElement.textContent === 'Copy two');
    await page.focus('#open');
    await page.evaluate(() => document.querySelector('[data-shop-key="two"]').remove());
    assert.equal(await page.evaluate(() => document.activeElement.id), 'open', 'updates must not steal focus from outside Shop');
    await page.evaluate(() => {
      document.querySelector('#outside-shop-action').focus();
      document.querySelector('#outside-shop-action').disabled = true;
    });
    await page.waitForFunction(() => document.activeElement.dataset.testid === 'shop-stop-title');
    await page.evaluate(() => stopWatching());
    console.log('Shopping companion browser regression passed.');
  } catch (error) {
    error.message = `${stage}: ${error.message}`;
    throw error;
  } finally {
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
