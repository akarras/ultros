// GlitchTip #7391: a partial IndexedDB engine (the Lightpanda crawler) fires
// `upgradeneeded` with a bare `IDBRequest` as `event.target`. rexie's `idb`
// dependency `.expect()`s that target to be an `IDBOpenDBRequest` inside the
// JS event callback, so the whole wasm module panics before hydration.
//
// This probe reproduces the engine defect in Chrome by shimming
// `Event.prototype.target` for `upgradeneeded` events only, then loads an
// item page and reports whether the wasm panicked and whether the page
// hydrated. Run it against prod for the positive control (old client:
// panics) and against a local build of the fix (no panic, hydrated, and the
// `game_data` store exists in the `ultros` database).
//
//   BASE_URL=https://ultros.app node idb-partial-engine.cjs
//   BASE_URL=http://127.0.0.1:8097 node idb-partial-engine.cjs
//
// SHIM=off skips the shim — a sanity pass that the normal cache path still
// works (second load must not refetch the .rkyv pack).
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'https://ultros.app';
const ITEM_PATH = process.env.ITEM_PATH || '/item/Garuda/9454';
const SHIM = (process.env.SHIM || 'on') === 'on';

const shim = () => {
  const desc = Object.getOwnPropertyDescriptor(Event.prototype, 'target');
  Object.defineProperty(Event.prototype, 'target', {
    configurable: true,
    get() {
      const real = desc.get.call(this);
      if (this.type !== 'upgradeneeded') return real;
      // What Lightpanda hands out: an IDBRequest that is not an
      // IDBOpenDBRequest. `result` still resolves to the database so a
      // tolerant handler that goes through the request it captured is fine.
      const fake = Object.create(IDBRequest.prototype);
      Object.defineProperty(fake, 'result', { get: () => real.result });
      Object.defineProperty(fake, 'transaction', { get: () => real.transaction });
      return fake;
    },
  });
};

(async () => {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  const panics = [];
  const logs = [];
  page.on('console', (m) => {
    const t = m.text();
    logs.push(t);
    if (t.includes('panicked at')) panics.push(t.split('\n').slice(0, 2).join(' | '));
  });
  const rkyvFetches = [];
  page.on('request', (r) => {
    if (r.url().includes('.rkyv')) rkyvFetches.push(r.url());
  });
  if (SHIM) await page.evaluateOnNewDocument(shim);

  const load = async () => {
    await page.goto(BASE_URL + ITEM_PATH, { waitUntil: 'load', timeout: 120000 });
    // A debug wasm bundle takes a while to compile; wait for the client to
    // either hydrate or panic, then a moment more for late panics.
    const deadline = Date.now() + Number(process.env.HYDRATE_TIMEOUT_MS || 180000);
    while (Date.now() < deadline) {
      if (logs.some((l) => l.includes('hydrating body') || l.includes('un-hydrated')) || panics.length) break;
      await new Promise((r) => setTimeout(r, 500));
    }
    await new Promise((r) => setTimeout(r, 5000));
    return page.evaluate(async () => {
      const hydrated = !!document.querySelector('[data-hk], [data-hydrated]') ||
        (window.__ultros_hydrated === true);
      let stores = null;
      try {
        const db = await new Promise((res, rej) => {
          const req = indexedDB.open('ultros');
          req.onsuccess = () => res(req.result);
          req.onerror = () => rej(req.error);
        });
        stores = Array.from(db.objectStoreNames);
        db.close();
      } catch (e) {
        stores = 'error: ' + e;
      }
      return { stores, bodyKids: document.body.childNodes.length };
    });
  };

  const first = await load();
  const hydratedLog = logs.some((l) => l.includes('hydrating body'));
  const idbSkipped = logs.filter((l) => l.includes('IndexedDB unavailable') || l.includes('game data cache unusable'));
  const rkyvFirst = rkyvFetches.length;
  rkyvFetches.length = 0;
  const second = SHIM ? null : await load();
  const rkyvSecond = rkyvFetches.length;

  const result = {
    base: BASE_URL,
    shim: SHIM,
    panics,
    hydratingBodyLogged: hydratedLog,
    idbSkippedLogs: idbSkipped,
    stores: first.stores,
    rkyvFetchesFirstLoad: rkyvFirst,
    rkyvFetchesSecondLoad: SHIM ? null : rkyvSecond,
    secondLoadStores: second && second.stores,
  };
  console.log(JSON.stringify(result, null, 2));
  await browser.close();
  const bad = panics.length > 0;
  process.exit(bad ? 1 : 0);
})().catch((e) => {
  console.error(e);
  process.exit(2);
});
