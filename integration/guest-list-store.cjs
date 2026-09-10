// Real IndexedDB regression tests. No Ultros server, database, account or
// market data needed. Run: node integration/guest-list-store.cjs
const assert = require('node:assert/strict');
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');

async function main() {
  const { default: puppeteer } = await import('puppeteer');
  const source = fs.readFileSync(path.join(__dirname, '../ultros/static/guest-list-store.mjs'));
  const server = http.createServer((request, response) => {
    response.setHeader('Content-Type', request.url === '/store.mjs' ? 'text/javascript' : 'text/html');
    response.end(request.url === '/store.mjs' ? source : '<!doctype html><title>Guest storage test</title>');
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    // A non-loopback HTTP origin reproduces LAN previews where randomUUID is
    // absent. Only this browser resolves the test hostname to our local server.
    browser = await puppeteer.launch({ headless: true, args: [
      '--host-resolver-rules=MAP guest-lists.test 127.0.0.1', '--no-proxy-server',
    ] });
    const context = await browser.createBrowserContext();
    const first = await context.newPage();
    const second = await context.newPage();
    const url = `http://guest-lists.test:${server.address().port}`;
    async function setup(page) {
      await page.goto(url);
      assert.deepEqual(await page.evaluate(() => [isSecureContext, typeof crypto.randomUUID]),
        [false, 'undefined']);
      await page.evaluate(async () => {
        window.api = await import('/store.mjs');
        window.store = await api.openGuestListStore();
      });
    }
    await setup(first);
    await setup(second);
    const id = await first.evaluate(async () => {
      localStorage.setItem('ultros.listdoc.v1.42.1', 'account-only sentinel');
      const created = await store.create({ name: 'Raid', snapshot: new Uint8Array([1, 2, 3]) });
      return created.id;
    });
    assert.match(id, /^device:[\da-f]{8}-[\da-f]{4}-4[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/);
    await setup(first); // Full navigation disposes the previous connection.
    assert.deepEqual(await first.evaluate(async (id) => {
      const record = await store.load(id);
      return [record.name, record.revision, [...record.snapshot]];
    }, id), ['Raid', 1, [1, 2, 3]]);

    assert.deepEqual(await first.evaluate(async () => {
      const observed = await store.create({ name: 'Focus check', snapshot: new Uint8Array([7]) });
      let callbacks = 0;
      const stop = api.guestWatch(observed.id, () => observed.revision, () => callbacks++);
      const focus = async () => { window.dispatchEvent(new Event('focus')); await new Promise(resolve => setTimeout(resolve, 40)); };
      await focus();
      const unchanged = callbacks;
      await store.save(observed.id, observed.revision, {name:'Changed elsewhere', snapshot:new Uint8Array([8])});
      await focus();
      const changed = callbacks;
      stop();
      await focus();
      let disposedCalls = 0;
      const dispose = api.guestWatch(observed.id, () => { disposedCalls++; return 0; }, () => disposedCalls++);
      window.dispatchEvent(new Event('focus'));
      dispose(); // focus is suspended on its asynchronous IndexedDB read
      await new Promise(resolve => setTimeout(resolve, 40));
      if (disposedCalls) throw new Error('Disposed focus watcher invoked a released Rust closure');
      await store.remove(observed.id, 2);
      return [unchanged, changed, callbacks];
    }), [0, 1, 1], 'focus checks only the watched revision, and teardown removes the listener');

    const race = await Promise.all([first, second].map((page, index) => page.evaluate(async ({ id, index }) => {
      try {
        const saved = await store.save(id, 1, { name: 'Raid', snapshot: new Uint8Array([index + 4]) });
        return { revision: saved.revision };
      } catch (error) { return { code: error.code }; }
    }, { id, index })));
    assert.equal(race.filter((result) => result.revision === 2).length, 1);
    assert.equal(race.filter((result) => result.code === 'conflict').length, 1);

    assert.equal(await first.evaluate(async (id) => {
      try { await store.remove(id, 1); } catch (error) { return error.code; }
    }, id), 'conflict');

    const failure = await first.evaluate(async (id) => {
      const previous = await store.load(id);
      const put = IDBObjectStore.prototype.put;
      IDBObjectStore.prototype.put = function () {
        throw new DOMException('Simulated quota failure', 'QuotaExceededError');
      };
      let errorName;
      try { await store.save(id, 2, { name: 'Lost?', snapshot: new Uint8Array([99]) }); }
      catch (error) { errorName = error.name; }
      finally { IDBObjectStore.prototype.put = put; }
      const current = await store.load(id);
      return { errorName, unchanged: current.revision === previous.revision && current.name === previous.name && current.snapshot[0] === previous.snapshot[0] };
    }, id);
    assert.deepEqual(failure, { errorName: 'QuotaExceededError', unchanged: true });

    // A request can succeed before its containing transaction aborts.
    assert.equal(await first.evaluate(async (id) => {
      const put = IDBObjectStore.prototype.put;
      IDBObjectStore.prototype.put = function (...args) {
        const request = put.apply(this, args);
        request.onsuccess = () => this.transaction.abort();
        return request;
      };
      let rejected = false;
      try { await store.save(id, 2, { name: 'Aborted', snapshot: new Uint8Array([100]) }); }
      catch { rejected = true; }
      finally { IDBObjectStore.prototype.put = put; }
      return rejected && (await store.load(id)).revision === 2;
    }, id), true);

    assert.deepEqual(await first.evaluate(async (id) => {
      for (let i = 0; i < 25; i++) await store.create({ name: `List ${i}`, snapshot: new Uint8Array([1]) });
      const lists = await store.list();
      return [lists.length, lists.some((list) => list.id === id), localStorage.getItem('ultros.listdoc.v1.42.1')];
    }, id), [26, true, 'account-only sentinel']);

    assert.equal(await first.evaluate(async (id) => {
      const record = await store.load(id);
      const restored = await store.create(api.decodeGuestListBackup(api.encodeGuestListBackup(record)));
      return restored.id !== id && restored.snapshot[0] === record.snapshot[0] && restored.name === record.name;
    }, id), true);

    assert.deepEqual(await first.evaluate(async (id) => {
      const db = await new Promise((resolve) => {
        const request = indexedDB.open('ultros-device-lists-v1', 1);
        request.onsuccess = () => resolve(request.result);
      });
      await new Promise((resolve, reject) => {
        const tx = db.transaction('lists', 'readwrite');
        tx.objectStore('lists').put({ id, revision: 2, name: 'Damaged', snapshot: 'invalid' });
        tx.oncomplete = resolve;
        tx.onabort = () => reject(tx.error);
      });
      db.close();
      let code;
      try { await store.load(id); } catch (error) { code = error.code; }
      const lists = await store.list();
      return [code, lists.length, Boolean(lists.find((list) => list.id === id).error)];
    }, id), ['corrupt', 27, true]);

    await context.close();
    console.log('Guest storage passed: reload, concurrent CAS, stale delete, failed/aborted writes, no eviction, account separation, backup restore and corruption isolation.');
  } finally {
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
