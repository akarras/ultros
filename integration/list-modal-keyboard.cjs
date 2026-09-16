// Real Lists dialogs: initial focus, keyboard containment, nested dismissal,
// and restoration. Requires a fresh test-auth build with Lists enabled.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const base = new URL(process.env.BASE_URL || 'http://127.0.0.1:8080').origin;
const tid = id => `[data-testid="${id}"]`;
const dialog = '[role="dialog"]';

async function main() {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  const errors = [];
  let listId;
  page.setDefaultTimeout(30000);
  page.on('pageerror', error => errors.push(String(error.stack || error)));
  await page.evaluateOnNewDocument(() => {
    window.__modalHydrated = false;
    window.addEventListener('ultros:hydrated', () => { window.__modalHydrated = true; });
  });
  await page.setRequestInterception(true);
  page.on('request', request => {
    const url = new URL(request.url());
    const action = url.hostname === 'pagead2.googlesyndication.com' && url.pathname.endsWith('/adsbygoogle.js')
      ? request.respond({ status: 200, contentType: 'application/javascript', body: '' })
      : request.continue();
    action.catch(error => errors.push(String(error)));
  });
  const load = async route => {
    await page.goto(new URL(route, base).href, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__modalHydrated);
  };
  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, {
      method, headers: { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!response.ok) throw new Error(`${method} ${route}: ${response.status}`);
    const text = await response.text();
    return text ? JSON.parse(text) : null;
  }, { method, route, body });
  const inTop = () => page.evaluate(() => [...document.querySelectorAll('[role="dialog"]')].at(-1)?.contains(document.activeElement));
  async function open(id) {
    await page.waitForSelector(tid(id), { visible: true });
    await page.focus(tid(id));
    await page.keyboard.press('Enter');
    await page.waitForSelector(dialog, { visible: true });
    await page.waitForFunction(() => [...document.querySelectorAll('[role="dialog"]')].at(-1)?.contains(document.activeElement));
  }
  async function cycle(id, name) {
    await open(id);
    if(name)assert.equal(await page.$eval(dialog,node=>node.getAttribute('aria-label')),name,`${id}: dialog has its visible localized name`);
    const count = await page.$eval(dialog, node => [...node.querySelectorAll('a[href],button,input,select,textarea,[tabindex]')]
      .filter(control => control.tabIndex >= 0 && !control.disabled && control.checkVisibility()).length);
    assert(count > 0);
    for (const backwards of [false, true]) {
      if (backwards) await page.keyboard.down('Shift');
      try {
        for (let index = 0; index < count + 2; index++) {
          await page.keyboard.press('Tab');
          assert(await inTop(), `${id}: ${backwards ? 'Shift+Tab' : 'Tab'} escaped dialog`);
        }
      } finally { if (backwards) await page.keyboard.up('Shift'); }
    }
    await page.keyboard.press('Escape');
    await page.waitForSelector(dialog, { hidden: true });
    assert(await page.$eval(tid(id), node => node === document.activeElement), `${id}: opener focus restored`);
    console.log(`[ok] ${id}: keyboard open, forward/backward containment, Escape, restoration`);
  }
  try {
    await page.setCookie(...[['LABS', 'lists-sync'], ['HIDE_ADS', 'true'], ['i18n_pref_locale', 'en']]
      .map(([name, value]) => ({ name, value, url: base, path: '/' })));
    await load('/list?labs=lists-sync');
    await cycle('list-new', 'New list');
    await cycle('list-restore-open', 'Restore backup');
    await load('/test/login?user_id=990000001509&username=ModalKeyboardQA&redirect=/list?labs=lists-sync');
    await cycle('list-join-open', 'Redeem invite');

    // Exercise two actual component instances together. Opening the second
    // programmatically preserves the first dialog's focused control as opener.
    await open('list-new');
    await page.focus(tid('device-list-name'));
    await page.$eval(tid('list-restore-open'), button => button.click());
    await page.waitForFunction(() => document.querySelectorAll('[role="dialog"]').length === 2);
    await page.waitForFunction(() => [...document.querySelectorAll('[role="dialog"]')].at(-1)?.contains(document.activeElement));
    await page.keyboard.press('Escape');
    await page.waitForFunction(() => document.querySelectorAll('[role="dialog"]').length === 1);
    assert(await page.$eval(tid('device-list-name'), node => document.activeElement === node));
    await page.keyboard.press('Escape');
    await page.waitForSelector(dialog, { hidden: true });
    assert(await page.$eval(tid('list-new'), node => document.activeElement === node));
    console.log('[ok] nested real dialogs close only the topmost and restore each opener');

    const worlds = await api('GET', '/api/v1/world_data');
    const world = worlds.regions[0].datacenters[0].worlds[0];
    const name = `Modal keyboard ${Date.now()}`;
    await api('POST', '/api/v1/list/create', { name, wdr_filter: { World: world.id } });
    listId = (await api('GET', '/api/v1/list')).find(entry => entry.list.name === name).list.id;
    await load(`/list/${listId}?labs=lists-sync`);
    await cycle('list-access-btn', 'Manage access');

    await open('list-settings-btn');
    const scope = `${dialog} [role="combobox"]`;
    await page.waitForSelector(scope, { visible: true });
    await page.focus(scope);
    await page.keyboard.press('ArrowDown');
    const chosen = await page.$eval(scope, input => {
      const listbox = [...document.querySelectorAll('[role="listbox"]')].find(node => node.checkVisibility());
      return listbox?.querySelector(`[id="${input.getAttribute('aria-activedescendant')}"]`)?.textContent.trim();
    });
    assert(chosen, 'custom world picker exposes a highlighted choice');
    await page.keyboard.press('Enter');
    await page.waitForFunction((selector, chosen) => {
      const input = document.querySelector(selector);
      return input?.getAttribute('aria-expanded') === 'false' && input.parentElement.textContent.includes(chosen);
    }, {}, scope, chosen);
    assert.equal(await page.$$(dialog).then(nodes => nodes.length), 1, 'selecting an external portal choice retains the modal');
    await page.focus(scope);
    await page.waitForFunction(selector => document.querySelector(selector)?.getAttribute('aria-expanded') === 'true', {}, scope);
    await page.keyboard.press('Escape');
    await page.waitForFunction(selector => document.querySelector(selector)?.getAttribute('aria-expanded') === 'false', {}, scope);
    assert.equal(await page.$$(dialog).then(nodes => nodes.length), 1, 'first Escape closes the dropdown only');
    await page.keyboard.press('Escape');
    await page.waitForSelector(dialog, { hidden: true });
    assert(await page.$eval(tid('list-settings-btn'), node => document.activeElement === node));
    console.log('[ok] modal world picker supports keyboard selection, dropdown Escape, then modal Escape');

    await load('/list?labs=lists-sync');
    await open('list-new');
    await page.$eval(tid('list-new'), button => button.remove());
    await page.keyboard.press('Escape');
    await page.waitForSelector(dialog, { hidden: true });
    assert(await page.evaluate(() => document.activeElement.isConnected));
    console.log('[ok] a detached opener does not cause a stale focus or application error');
    assert.deepEqual(errors, [], 'no application errors');
  } finally {
    try {
      if (listId !== undefined) await api('DELETE', `/api/v1/list/${listId}/delete`);
    } finally { await browser.close(); }
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
