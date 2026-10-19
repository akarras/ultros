const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const vm = require('node:vm');

// `entries` lets a second worker instance (e.g. an updated worker script) see
// the CacheStorage an earlier instance left behind; `transform` edits its source.
function fixture({ entries = new Map(), transform = source => source } = {}) {
  const listeners = {};
  const fetched = [];
  let online = true;
  let failPath;
  let putFailure;
  let assetBody = 'public asset';
  let locksAvailable = true;
  let lockQueue = Promise.resolve();
  const key = value => new URL(typeof value === 'string' ? value : value.url, 'https://ultros.test').href;
  const cacheStorage = {
    open: async name => {
      if (!entries.has(name)) entries.set(name, new Map());
      const content = entries.get(name);
      return {
        put: async (path, response) => {
          if (putFailure?.(name, key(path))) throw new Error('QuotaExceededError');
          content.set(key(path), response.clone());
        },
        match: async path => content.get(key(path))?.clone(),
      };
    },
    keys: async () => [...entries.keys()],
    has: async name => entries.has(name),
    delete: async name => entries.delete(name),
  };
  const context = {
    URL, Request, Response, console,
    self: {
      location: { origin: 'https://ultros.test' },
      addEventListener: (name, listener) => { listeners[name] = listener; },
      clients: { claim: async () => {} },
      skipWaiting: async () => {},
      crypto: require('node:crypto').webcrypto,
      get navigator() {
        return { locks: locksAvailable ? {
          request: (_name, options, callback) => {
            const action = typeof options === 'function' ? options : callback;
            const next = lockQueue.then(action);
            lockQueue = next.catch(() => {});
            return next;
          },
        } : undefined };
      },
    },
    caches: cacheStorage,
    fetch: async request => {
      fetched.push(request);
      if (!online || new URL(key(request)).pathname === failPath) throw new Error('offline');
      return new Response(new URL(key(request)).pathname === '/api/v1/world_data'
        ? JSON.stringify({ worlds: [], malicious: '</script><script>private</script>' })
        : assetBody);
    },
  };
  vm.runInNewContext(transform(readFileSync(require.resolve('../ultros/static/service-worker.js'), 'utf8')), context);
  const manifest = {
    type: 'ULTROS_PREPARE_GUEST_OFFLINE', js: '/pkg/ultros.123.js',
    wasm: '/pkg/ultros_bg.123.wasm', css: '/pkg/ultros.123.css',
    assets: ['/static/data/123/en.rkyv', '/pkg/snippets/a/import.js', '/pkg/f4b41bf1/snippets/guest-offline.mjs'], lang: 'en',
  };
  return {
    entries, fetched, listeners, manifest,
    activeGeneration: async () => entries.get('ultros-guest-offline-v2')
      .get(key('/__ultros_guest_offline_active__')).clone().text(),
    offline: () => { online = false; },
    fail: path => { failPath = path; },
    failPut: predicate => { putFailure = predicate; },
    body: value => { assetBody = value; },
    noLocks: () => { locksAvailable = false; },
    async prepare(data = manifest) {
      let pending;
      let result;
      listeners.message({ data, waitUntil: value => { pending = value; }, ports: [{ postMessage: value => { result = value; } }] });
      await pending;
      return result;
    },
    async request(path, mode = 'navigate') {
      let response;
      listeners.fetch({ request: { url: key(path), method: 'GET', mode }, respondWith: value => { response = value; } });
      return response;
    },
  };
}

test('cold guest reload returns only generated anonymous shell and prepared public assets', async () => {
  const f = fixture();
  assert.equal((await f.prepare()).ready, true);
  assert.ok(f.fetched.every(request => request.credentials === 'omit'));
  f.offline();
  const html = await (await f.request('/list/device/device%3Aabc')).text();
  assert.match(html, /__ULTROS_OFFLINE_GUEST__=true/);
  assert.match(html, /"current_user":null/);
  assert.match(html, /\\u003c\/script>/);
  assert.doesNotMatch(html, /<script>private/);
  assert.match(html, /ultros\.123\.js/);
  assert.equal((await f.request('/static/data/123/en.rkyv', 'cors')).status, 200);
  assert.equal((await f.request('/list')).status, 200);
  assert.equal(await f.request('/list/123'), undefined);
  assert.equal(await f.request('/api/v1/current_user', 'cors'), undefined);
  assert.equal(await f.request('/api/v1/list/123', 'cors'), undefined);
  assert.equal(await f.request('/alerts'), undefined);
});

test('unprepared offline visit does not fabricate a working app', async () => {
  const f = fixture();
  f.offline();
  assert.equal((await f.request('/list')).type, 'error');
});

test('projected startup catalog works offline without downloading the full pack', async () => {
  const f = fixture();
  f.manifest.assets[0] = '/static/startup/123/en.rkyv';
  assert.equal((await f.prepare()).ready, true);
  assert.ok(f.fetched.every(request => !request.url.includes('/static/data/')));
  f.offline();
  assert.equal((await f.request('/static/startup/123/en.rkyv', 'cors')).status, 200);
});

test('reject account endpoints, arbitrary scripts, and external assets before fetching', async () => {
  for (const asset of ['/api/v1/current_user', '/list/123', 'https://evil.test/pkg/a.js', '/pkg/a.js?private=1']) {
    const f = fixture();
    assert.equal((await f.prepare({ ...f.manifest, assets: [asset] })).ready, false);
    assert.equal(f.fetched.length, 0);
    assert.equal([...f.entries.keys()].filter(name => name.includes('-generation-')).length, 0);
  }
});

test('failed preparation leaves previous working offline shell intact', async () => {
  const f = fixture();
  assert.equal((await f.prepare()).ready, true);
  f.fail('/pkg/ultros.456.js');
  assert.equal((await f.prepare({ ...f.manifest, js: '/pkg/ultros.456.js' })).ready, false);
  f.offline();
  assert.match(await (await f.request('/list')).text(), /ultros\.123\.js/);
});

test('worker retains existing notification handlers', () => {
  const f = fixture();
  assert.equal(typeof f.listeners.push, 'function');
  assert.equal(typeof f.listeners.notificationclick, 'function');
});

for (const failure of ['asset', 'shell', 'pointer']) {
  test(`${failure} cache.put failure cannot overwrite previous shell or dependencies`, async () => {
    const f = fixture();
    assert.equal((await f.prepare()).ready, true);
    f.body('new incompatible build');
    f.failPut((_name, url) => failure === 'asset' ? url.endsWith('/pkg/ultros_bg.123.wasm')
      : url.endsWith(failure === 'shell' ? '/__ultros_guest_offline_shell__' : '/__ultros_guest_offline_active__'));
    assert.equal((await f.prepare({ ...f.manifest, lang: 'ja' })).ready, false);
    f.offline();
    assert.match(await (await f.request('/list')).text(), /lang="en"/);
    assert.equal(await (await f.request('/pkg/ultros.123.js', 'cors')).text(), 'public asset');
    assert.equal(await (await f.request('/pkg/ultros_bg.123.wasm', 'cors')).text(), 'public asset');
    assert.equal(f.entries.size, 2, 'metadata plus one intact generation; failed staging removed');
  });
}

test('concurrent preparations publish complete generations and prune obsolete caches', async () => {
  const f = fixture();
  const results = await Promise.all(Array.from({ length: 6 }, (_, index) =>
    f.prepare({ ...f.manifest, js: `/pkg/ultros.${index}.js` })));
  assert.ok(results.every(result => result.ready));
  assert.equal(f.entries.size, 2, 'metadata plus latest complete generation');
  f.offline();
  assert.match(await (await f.request('/list')).text(), /ultros\.5\.js/);
  assert.equal(await (await f.request('/pkg/ultros.5.js', 'cors')).text(), 'public asset');
});

test('without Web Locks preparation fails safely and existing offline generation remains readable', async () => {
  const f = fixture();
  assert.equal((await f.prepare()).ready, true);
  f.noLocks();
  assert.equal((await f.prepare()).ready, false);
  f.offline();
  assert.equal((await f.request('/list')).status, 200);
});


test('offline shell renders every supported language before loading the client', async () => {
  for (const [lang, opening] of Object.entries({en:'Opening device lists',de:'Gerätelisten werden geöffnet',fr:'Ouverture des listes locales',ja:'端末のリストを開いています',cn:'正在打开设备列表',ko:'기기 목록 여는 중',tc:'正在開啟裝置清單'})) {
    const f = fixture();
    assert.equal((await f.prepare({...f.manifest, lang})).ready, true);
    f.offline();
    const html = await (await f.request('/list')).text();
    assert.ok(html.includes(opening));
    assert.ok(html.includes('lang="'+lang+'"'));
    if (lang !== 'en') assert.ok(!html.includes('Could not open offline lists'));
  }
});

test('bundled module generations stay matched to their client build', async () => {
  const f = fixture();
  const oldModule = '/pkg/build-old/ultros/static/guest-list-store.mjs';
  assert.equal((await f.prepare({...f.manifest, assets:[oldModule]})).ready,true);
  f.body('new module');
  f.fail('/pkg/build-new/ultros.js');
  assert.equal((await f.prepare({...f.manifest, js:'/pkg/build-new/ultros.js',assets:['/pkg/build-new/ultros/static/guest-list-store.mjs']})).ready,false);
  f.offline();
  assert.equal(await (await f.request(oldModule,'cors')).text(),'public asset');
  assert.equal(await f.request('/static/guest-list-store.mjs','cors'),undefined);
});

test('identical manifest reuses the active generation without refetching assets', async () => {
  const f = fixture();
  assert.equal((await f.prepare()).ready, true);
  const generation = await f.activeGeneration();
  const staged = f.fetched.length;
  for (let visit = 0; visit < 3; visit++) assert.equal((await f.prepare()).ready, true);
  assert.equal(f.fetched.length, staged, 'repeat Lists visits must not refetch the client');
  assert.equal(await f.activeGeneration(), generation);
  assert.equal(f.entries.size, 2);
});

test('reordered, duplicated and normalized-language-equal manifests reuse', async () => {
  const f = fixture();
  assert.equal((await f.prepare()).ready, true);
  const staged = f.fetched.length;
  const assets = [...f.manifest.assets].reverse();
  assert.equal((await f.prepare({ ...f.manifest, assets: [...assets, assets[0], f.manifest.js] })).ready, true);
  assert.equal((await f.prepare({ ...f.manifest, lang: 'xx' })).ready, true, 'normalizes to en');
  assert.equal(f.fetched.length, staged);
});

for (const [name, change] of [
  ['language', m => ({ ...m, lang: 'ja' })],
  ['asset set', m => ({ ...m, assets: [...m.assets, '/pkg/snippets/b/extra.js'] })],
  ['client build', m => ({ ...m, js: '/pkg/ultros.456.js', wasm: '/pkg/ultros_bg.456.wasm' })],
  ['stylesheet', m => ({ ...m, css: '/pkg/ultros.456.css' })],
  ['catalog snapshot', m => ({ ...m, assets: ['/static/data/456/en.rkyv', ...m.assets.slice(1)] })],
]) {
  test(`changed ${name} restages a new generation`, async () => {
    const f = fixture();
    assert.equal((await f.prepare()).ready, true);
    const generation = await f.activeGeneration();
    const staged = f.fetched.length;
    const next = change(f.manifest);
    assert.equal((await f.prepare(next)).ready, true);
    assert.ok(f.fetched.length > staged);
    assert.notEqual(await f.activeGeneration(), generation);
    assert.equal(f.entries.size, 2, 'old generation pruned after publication');
    const restaged = f.fetched.length;
    assert.equal((await f.prepare(next)).ready, true);
    assert.equal(f.fetched.length, restaged, 'new generation is itself reusable');
    f.offline();
    const html = await (await f.request('/list')).text();
    assert.ok(html.includes(`lang="${next.lang}"`));
    assert.ok(html.includes(next.js) && html.includes(next.css));
  });
}

test('updated worker shell version restages an unchanged build', async () => {
  const entries = new Map();
  const bump = source => {
    assert.match(source, /const GUEST_SHELL_VERSION = 1;/);
    return source.replace('const GUEST_SHELL_VERSION = 1;', 'const GUEST_SHELL_VERSION = 2;');
  };
  const before = fixture({ entries });
  assert.equal((await before.prepare()).ready, true);
  const generation = await before.activeGeneration();
  const after = fixture({ entries, transform: bump });
  assert.equal((await after.prepare()).ready, true);
  assert.ok(after.fetched.length > 0);
  assert.notEqual(await after.activeGeneration(), generation);
  // Another instance of the same updated worker reuses what was just staged.
  const again = fixture({ entries, transform: bump });
  assert.equal((await again.prepare()).ready, true);
  assert.equal(again.fetched.length, 0);
});

for (const [name, damage] of [
  ['evicted wasm dependency', (cache, key) => cache.delete(key('/pkg/ultros_bg.123.wasm'))],
  ['evicted snippet', (cache, key) => cache.delete(key('/pkg/snippets/a/import.js'))],
  ['evicted world data', (cache, key) => cache.delete(key('/api/v1/world_data'))],
  ['evicted shell', (cache, key) => cache.delete(key('/__ultros_guest_offline_shell__'))],
  ['pre-manifest generation format', (cache, key) => cache.delete(key('/__ultros_guest_offline_manifest__'))],
  ['foreign manifest metadata', (cache, key) => cache.set(key('/__ultros_guest_offline_manifest__'), new Response('{"old":true}'))],
]) {
  test(`${name} forces a safe restage`, async () => {
    const f = fixture();
    assert.equal((await f.prepare()).ready, true);
    const generation = await f.activeGeneration();
    damage(f.entries.get(generation), value => new URL(value, 'https://ultros.test').href);
    const staged = f.fetched.length;
    assert.equal((await f.prepare()).ready, true);
    assert.ok(f.fetched.length > staged);
    assert.notEqual(await f.activeGeneration(), generation);
    f.offline();
    assert.equal((await f.request('/list')).status, 200);
    assert.equal((await f.request('/pkg/ultros_bg.123.wasm', 'cors')).status, 200);
    assert.equal((await f.request('/pkg/snippets/a/import.js', 'cors')).status, 200);
  });
}

test('pointer to a wholly evicted generation restages instead of reusing an empty cache', async () => {
  const f = fixture();
  assert.equal((await f.prepare()).ready, true);
  const generation = await f.activeGeneration();
  f.entries.delete(generation);
  assert.equal((await f.prepare()).ready, true);
  assert.notEqual(await f.activeGeneration(), generation);
  assert.equal(f.entries.size, 2);
  f.offline();
  assert.equal((await f.request('/list')).status, 200);
});

test('failed replacement keeps the old generation reusable', async () => {
  const f = fixture();
  assert.equal((await f.prepare()).ready, true);
  const generation = await f.activeGeneration();
  f.fail('/pkg/ultros.456.js');
  assert.equal((await f.prepare({ ...f.manifest, js: '/pkg/ultros.456.js' })).ready, false);
  assert.equal(await f.activeGeneration(), generation);
  const afterFailure = f.fetched.length;
  assert.equal((await f.prepare()).ready, true);
  assert.equal(f.fetched.length, afterFailure, 'old generation still matches and is reused');
  assert.equal(f.entries.size, 2);
});

test('invalid manifests are rejected before reuse or fetch', async () => {
  const f = fixture();
  assert.equal((await f.prepare()).ready, true);
  const staged = f.fetched.length;
  const generation = await f.activeGeneration();
  assert.equal((await f.prepare({ ...f.manifest, assets: [...f.manifest.assets, '/api/v1/current_user'] })).ready, false);
  assert.equal(f.fetched.length, staged);
  assert.equal(await f.activeGeneration(), generation);
});

test('manifest metadata is never served by the fetch handler', async () => {
  const f = fixture();
  assert.equal((await f.prepare()).ready, true);
  f.offline();
  assert.equal(await f.request('/__ultros_guest_offline_manifest__', 'cors'), undefined);
  assert.equal(await f.request('/__ultros_guest_offline_manifest__'), undefined);
});
