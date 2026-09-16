// Real client lifecycle regression. Only readyState is controlled; handshake,
// subscriptions, edits and reconnects use native WebSockets and the real server.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const base = process.env.BASE_URL || 'http://127.0.0.1:8080';
const needed = 'input[aria-label^="Needed for "]';
async function api(page, method, route, body) {
  return page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, { method, headers: { 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
    if (!response.ok) throw new Error(`${method} ${route}: ${response.status}`);
    const text = await response.text(); return text ? JSON.parse(text) : null;
  }, { method, route, body });
}
async function main() {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  const errors = [], lists = [];
  page.setDefaultTimeout(90000);
  page.setDefaultNavigationTimeout(90000);
  page.on('pageerror', error => errors.push(String(error.stack || error)));
  page.on('dialog', dialog => dialog.accept());
  // Keep the regression focused on our client; every first-party request and
  // every application exception remains enabled and asserted below.
  await page.setRequestInterception(true);
  page.on('request', request => {
    const host = new URL(request.url()).hostname;
    if (/(^|\.)(googlesyndication\.com|doubleclick\.net|googleadservices\.com)$/.test(host)) request.abort();
    else request.continue();
  });
  await page.evaluateOnNewDocument(() => {
    window.retirementHydrated = false;
    addEventListener('ultros:hydrated', () => { window.retirementHydrated = true; });
    const NativeSocket = window.WebSocket;
    window.retirementSockets = [];
    window.retirementTimers = [];
    const nativeTimeout = window.setTimeout;
    window.setTimeout = function (callback, delay, ...args) {
      if (window.retirementTrackTimers && [1000, 2000, 4000, 8000, 16000, 32000].includes(delay)) window.retirementTimers.push(delay);
      return nativeTimeout.call(this, callback, delay, ...args);
    };
    window.WebSocket = class extends NativeSocket {
      constructor(...args) {
        super(...args);
        if (String(args[0]).includes('/api/v1/realtime/events')) window.retirementSockets.push(this);
      }
      get readyState() { return this.forceClosing ? NativeSocket.CLOSING : super.readyState; }
    };
  });
  try {
    const login = await page.goto(`${base}/test/login?user_id=990000001505&username=SocketRetirementQA&redirect=/list`, { waitUntil: 'domcontentloaded' });
    assert(login.ok(), 'test-auth build is required');
    await page.setCookie({ name: 'LABS', value: 'lists-sync', url: base, path: '/' });
    for (const entry of await api(page, 'GET', '/api/v1/list')) {
      if (entry.list.name.startsWith('Socket retirement ')) await api(page, 'DELETE', `/api/v1/list/${entry.list.id}/delete`);
    }
    const worlds = await api(page, 'GET', '/api/v1/world_data');
    for (const label of ['A', 'B']) {
      const name = `Socket retirement ${label} ${Date.now()}`;
      await api(page, 'POST', '/api/v1/list/create', { name, wdr_filter: { World: worlds.regions[0].datacenters[0].worlds[0].id } });
      const id = (await api(page, 'GET', '/api/v1/list')).find(entry => entry.list.name === name).list.id;
      lists.push(id);
      await api(page, 'POST', `/api/v1/list/${id}/add/item`, { id: 0, item_id: 5, list_id: id, hq: null, quantity: 10, acquired: 0 });
    }
    await page.goto(`${base}/list/${lists[0]}`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.retirementHydrated && document.querySelector('input[aria-label^="Needed for "]') && window.retirementSockets.some(socket => socket.readyState === WebSocket.OPEN));
    await page.evaluate(id => {
      window.retirementOld = window.retirementSockets.at(-1);
      window.retirementOld.forceClosing = true;
      // A real SPA navigation drops the previous subscription and subscribes
      // the next list while the old connection is still CLOSING.
      const link = document.createElement('a'); link.href = `/list/${id}`;
      document.body.append(link); link.click(); link.remove();
    }, lists[1]);
    await page.waitForFunction(id => location.pathname === `/list/${id}` && document.querySelector('input[aria-label^="Needed for "]') && window.retirementSockets.at(-1) !== window.retirementOld && window.retirementSockets.at(-1).readyState === WebSocket.OPEN, {}, lists[1]);
    const detached = await page.evaluate(() => {
      const old = window.retirementOld;
      const handlers = ['onopen', 'onmessage', 'onclose', 'onerror'].map(name => old[name] === null);
      old.dispatchEvent(new CloseEvent('close'));
      old.dispatchEvent(new Event('error'));
      old.dispatchEvent(new Event('open'));
      old.dispatchEvent(new MessageEvent('message', { data: JSON.stringify({ Error: { message: 'obsolete socket' } }) }));
      return handlers;
    });
    assert.deepEqual(detached, [true, true, true, true], 'retired socket handlers must be detached before Rust closures drop');
    console.log('PASS: CLOSING replacement retires all old callbacks; delayed events are inert');
    await page.evaluate(() => {
      window.retirementTrackTimers = true;
      window.retirementTimers = [];
      window.retirementBeforeReconnect = window.retirementSockets.length;
      const socket = window.retirementSockets.at(-1);
      // Registered after the client's onclose property, so capture the whole
      // failure sequence but exclude unrelated timers during the new handshake.
      socket.addEventListener('close', () => { window.retirementTrackTimers = false; }, { once: true });
      socket.dispatchEvent(new Event('error'));
      socket.dispatchEvent(new Event('error'));
      socket.close();
    });
    await page.waitForFunction(() => window.retirementSockets.length > window.retirementBeforeReconnect && window.retirementSockets.at(-1).readyState === WebSocket.OPEN);
    assert.equal(await page.evaluate(() => window.retirementTimers.length), 1, 'error/error/close must coalesce into one reconnect timer');
    assert.equal(await page.evaluate(() => window.retirementSockets.length - window.retirementBeforeReconnect), 1, 'exactly one successor connection');
    await page.evaluate(selector => {
      const input = document.querySelector(selector); input.value = '17'; input.dispatchEvent(new Event('change', { bubbles: true }));
    }, needed);
    await page.waitForFunction(async id => {
      const response = await fetch(`/api/v1/list/${id}/listings`);
      return (await response.json())[1][0][0].quantity === 17;
    }, {}, lists[1]);
    assert.deepEqual(errors, [], 'browser application errors');
    console.log('PASS: reconnect events coalesce and the successor keeps the real list subscription and syncs edits');
  } finally {
    for (const error of errors) console.error('Browser application error:', error);
    for (const id of lists) await api(page, 'DELETE', `/api/v1/list/${id}/delete`).catch(error => console.error('Cleanup:', error.message));
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
