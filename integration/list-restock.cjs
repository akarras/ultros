// Browser regression for Lists 2.0 bulk progress actions (Mark acquired /
// Reset owned) and whole-list Reset progress (recurring restock), covering
// both anonymous device lists and authenticated online lists + single-step Undo.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');

const base = new URL(process.env.BASE_URL || 'http://127.0.0.1:8080').origin;
const tid = id => `[data-testid="${id}"]`;

async function main() {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 900 });
  page.setDefaultTimeout(30000);
  const errors = [];
  let createdListId;
  page.on('pageerror', err => errors.push(String(err.stack || err)));
  page.on('dialog', d => (d.type() === 'beforeunload' ? d.accept() : d.dismiss()).catch(() => {}));
  await page.evaluateOnNewDocument(() => {
    window.__restockHydrated = false;
    window.addEventListener('ultros:hydrated', () => { window.__restockHydrated = true; });
  });
  await page.setRequestInterception(true);
  page.on('request', req => {
    const url = new URL(req.url());
    const action = url.hostname !== new URL(base).hostname
      ? req.abort()
      : req.continue();
    action.catch(() => {});
  });

  const load = async route => {
    await page.goto(new URL(route, base).href, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__restockHydrated);
  };
  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const res = await fetch(route, {
      method,
      headers: { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`${method} ${route}: ${res.status}`);
    const text = await res.text();
    return text ? JSON.parse(text) : null;
  }, { method, route, body });

  async function addItem(name, qty) {
    const searchSel = '#list-cart-add-input';
    await page.waitForSelector(searchSel, { visible: true });
    await page.$eval(searchSel, (el, val) => {
      el.value = val;
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }, name);
    const qtySel = `${tid('inline-list-add')} input[type="number"]`;
    await page.$eval(qtySel, (el, val) => {
      el.value = String(val);
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }, qty);
    const addBtn = `${tid('inline-list-add')} button[aria-label="Add ${name}"]`;
    await page.waitForSelector(addBtn, { visible: true });
    await page.click(addBtn);
  }

  async function exerciseRestockFlow(label) {
    await addItem('Maple Log', 2);
    await page.waitForFunction(() => document.querySelectorAll('[data-testid="cart-rows"] > li').length === 1);
    await addItem('Bronze Ingot', 3);
    await page.waitForFunction(() => document.querySelectorAll('[data-testid="cart-rows"] > li').length === 2);

    await page.waitForFunction(() => (document.querySelector('[data-testid="list-estimate-progress"]')?.textContent || '').includes('0 / 5'));
    assert.equal(await page.$(tid('cart-reset-progress')), null, `${label}: reset progress hidden when owned is 0`);

    // Select both rows via their checkboxes
    const checkboxes = await page.$$(`${tid('cart-rows')} > li input[type="checkbox"]`);
    assert.equal(checkboxes.length, 2);
    for (const cb of checkboxes) await cb.click();
    await page.waitForSelector(tid('cart-selection-bar'), { visible: true });

    assert.equal(await page.$eval(tid('cart-bulk-acquired'), el => el.disabled), false, `${label}: bulk mark acquired enabled`);
    assert.equal(await page.$eval(tid('cart-bulk-reset-owned'), el => el.disabled), true, `${label}: bulk reset owned disabled when none owned`);

    // Bulk mark acquired
    await page.click(tid('cart-bulk-acquired'));
    await page.waitForFunction(() => (document.querySelector('[data-testid="list-estimate-progress"]')?.textContent || '').includes('5 / 5'));
    assert.equal(await page.$eval(tid('cart-bulk-acquired'), el => el.disabled), true, `${label}: bulk mark acquired disabled once all selected owned`);
    assert.equal(await page.$eval(tid('cart-bulk-reset-owned'), el => el.disabled), false, `${label}: bulk reset owned enabled once owned`);

    // Deselect second row and reset only first row's owned count
    const checkboxesAfter = await page.$$(`${tid('cart-rows')} > li input[type="checkbox"]`);
    await checkboxesAfter[1].click();
    await page.click(tid('cart-bulk-reset-owned'));
    await page.waitForFunction(() => {
      const txt = document.querySelector('[data-testid="list-estimate-progress"]')?.textContent || '';
      return txt.includes('2 / 5') || txt.includes('3 / 5');
    });
    const partialProgress = await page.$eval(tid('list-estimate-progress'), el => el.textContent.trim());

    // Whole-list Reset progress requires two clicks to confirm
    await page.waitForSelector(tid('cart-reset-progress'), { visible: true });
    await page.click(tid('cart-reset-progress'));
    await page.waitForFunction(() => document.querySelector('[data-testid="cart-reset-progress"]')?.dataset.confirming === 'true');
    assert.equal(await page.$eval(tid('list-estimate-progress'), el => el.textContent.trim()), partialProgress, `${label}: first click only arms reset`);

    await page.click(tid('cart-reset-progress'));
    await page.waitForFunction(() => (document.querySelector('[data-testid="list-estimate-progress"]')?.textContent || '').includes('0 / 5'));
    await page.waitForFunction(() => document.querySelector('[data-testid="cart-reset-progress"]') === null);

    // Single Undo step restores the entire pre-reset progress
    await page.evaluate(() => document.activeElement?.blur());
    const undoModifier = process.platform === 'darwin' ? 'Meta' : 'Control';
    await page.keyboard.down(undoModifier);
    await page.keyboard.press('z');
    await page.keyboard.up(undoModifier);
    await page.waitForFunction(expected => (document.querySelector('[data-testid="list-estimate-progress"]')?.textContent || '').trim() === expected, {}, partialProgress);
    console.log(`[ok] ${label}: bulk mark acquired, bulk reset owned, two-click restock reset, and one-step undo`);
  }

  try {
    await page.setCookie(
      { name: 'HIDE_ADS', value: 'true', url: base, path: '/' },
      { name: 'i18n_pref_locale', value: 'en', url: base, path: '/' },
    );

    // 1. Anonymous device list
    await load('/list');
    await page.waitForSelector(tid('list-new'), { visible: true });
    await page.click(tid('list-new'));
    await page.waitForSelector(tid('device-list-name'), { visible: true });
    await page.type(tid('device-list-name'), 'Restock Device QA');
    await page.click(tid('device-list-create'));
    await page.waitForSelector(tid('device-list-editor'), { visible: true });
    await exerciseRestockFlow('device list');

    // 2. Authenticated online account list
    await load('/test/login?user_id=990000001740&username=RestockQA&redirect=/list');
    const worlds = await api('GET', '/api/v1/world_data');
    const world = worlds.regions[0].datacenters[0].worlds[0];
    const listName = `Restock Online QA ${Date.now()}`;
    await api('POST', '/api/v1/list/create', { name: listName, wdr_filter: { World: world.id } });
    createdListId = (await api('GET', '/api/v1/list')).find(e => e.list.name === listName).list.id;
    await load(`/list/${createdListId}`);
    await page.waitForSelector(tid('list-view-sync'), { visible: true });
    await page.waitForSelector('#list-cart-add-input', { visible: true });
    await exerciseRestockFlow('online list');

    assert.deepEqual(errors, [], 'no application errors');
  } finally {
    try {
      if (createdListId !== undefined) await api('DELETE', `/api/v1/list/${createdListId}/delete`);
    } finally {
      await browser.close();
    }
  }
}

main().catch(err => {
  console.error(err);
  process.exitCode = 1;
});
