// Access form drafts survive real list updates; authoritative deletion closes it.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const base = new URL(process.env.BASE_URL || 'http://127.0.0.1:8080').origin;
const tid = value => `[data-testid="${value}"]`;
(async () => {
  const browser = await puppeteer.launch({ headless: true, args: ['--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(30000);
  let id;
  const errors = [];
  page.on('pageerror', error => errors.push(String(error.stack || error)));
  await page.setRequestInterception(true);
  page.on('request', request => {
    const url = new URL(request.url());
    const action = url.hostname === 'pagead2.googlesyndication.com' && url.pathname.endsWith('/adsbygoogle.js')
      ? request.respond({ status: 200, contentType: 'application/javascript', body: '' }) : request.continue();
    action.catch(error => errors.push(String(error)));
  });
  await page.evaluateOnNewDocument(() => {
    window.__parentHydrated = false;
    window.addEventListener('ultros:hydrated', () => { window.__parentHydrated = true; });
  });
  const load = async path => {
    await page.bringToFront();
    await page.goto(new URL(path, base).href, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__parentHydrated, { polling: 100 });
  };
  const api = (method, path, body) => page.evaluate(async ({ method, path, body }) => {
    const response = await fetch(path, { method, headers: { 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
    if (!response.ok) throw new Error(`${method} ${path}: ${response.status}`);
    const text = await response.text();
    return text ? JSON.parse(text) : null;
  }, { method, path, body });
  try {
    await page.setCookie(...[['LABS', 'lists-sync'], ['HIDE_ADS', 'true'], ['i18n_pref_locale', 'en']].map(([name, value]) => ({ name, value, url: base, path: '/' })));
    await load('/test/login?user_id=990000001511&username=AccessDraftQA&redirect=/list?labs=lists-sync');
    const worlds = await api('GET', '/api/v1/world_data');
    const world = worlds.regions.flatMap(region => region.datacenters.flatMap(dc => dc.worlds)).find(world => world.name === 'Gilgamesh');
    assert(world);
    const name = `Access draft ${Date.now()}`;
    await api('POST', '/api/v1/list/create', { name, wdr_filter: { World: world.id } });
    id = (await api('GET', '/api/v1/list')).find(entry => entry.list.name === name).list.id;
    const add = quantity => api('POST', `/api/v1/list/${id}/add/item`, { id: 0, item_id: 5056, list_id: id, hq: null, quantity, acquired: 0 });
    await add(3);
    await load(`/list/${id}?labs=lists-sync`);
    await page.waitForFunction(() => document.querySelector('input[aria-label="Needed for Bronze Ingot"]')?.value === '3', { polling: 100 });
    await page.click(tid('list-access-btn'));
    await page.waitForSelector(tid('list-invite-max-uses'), { visible: true });
    await page.select(tid('list-invite-permission'), 'Write');
    await page.type(tid('list-invite-max-uses'), '7');
    const before = await page.evaluate(() => ({ limit: document.querySelector('[data-testid="list-invite-max-uses"]')?.value, permission: document.querySelector('[data-testid="list-invite-permission"]')?.value, active: document.activeElement?.getAttribute('data-testid') }));
    await add(1);
    await page.waitForFunction(() => document.querySelector('input[aria-label="Needed for Bronze Ingot"]')?.value === '4', { polling: 100 });
    const after = await page.evaluate(() => ({ limit: document.querySelector('[data-testid="list-invite-max-uses"]')?.value, permission: document.querySelector('[data-testid="list-invite-permission"]')?.value, active: document.activeElement?.getAttribute('data-testid') }));
    console.log(JSON.stringify({ before, after, errors }));
    assert.deepEqual(after, before, 'live list updates preserve the open invite draft and focus');
    await api('DELETE', `/api/v1/list/${id}/delete`);
    id=undefined;
    await page.waitForSelector(tid('list-invite-max-uses'), { hidden: true });
    await page.waitForSelector(tid('list-access-btn'), { hidden: true });
    assert.deepEqual(errors, []);
    console.log('PASS: live list edits preserve access drafts and focus; deletion closes the guarded dialog');
  } finally {
    try { if (id !== undefined) await api('DELETE', `/api/v1/list/${id}/delete`); }
    finally { await browser.close(); }
  }
})().catch(error => { console.error(error); process.exitCode = 1; });

