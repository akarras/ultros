// Real service-worker install + disconnected cold reload, no Ultros DB needed.
// The tiny client stands in for WASM; the production helper, worker, generated
// shell, and IndexedDB guest storage are all exercised without substitutions.
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');

const staticRoot = path.resolve(__dirname, '../ultros/static');
const client = `export default async function init() { await (await fetch('/pkg/f4b41bf1/ultros.wasm')).arrayBuffer(); }
export async function hydrate() {
  const { openGuestListStore } = await import('/static/guest-list-store.mjs');
  document.body.textContent = 'Offline client opened';
  document.body.dataset.offline = String(window.__ULTROS_OFFLINE_GUEST__ === true);
  document.body.dataset.anonymous = String(window.__ULTROS_BOOTSTRAP__.current_user === null);
  // The production Rust client decodes Loro; this fixture checks durable bytes.
  const store = await openGuestListStore();
  document.body.dataset.saved = (await store.list())[0].name;
}`;
const html = `<!doctype html><html><head><link id="leptos" rel="stylesheet" href="/pkg/f4b41bf1/ultros.css"></head><body>
<script type="module">
import init from '/pkg/f4b41bf1/ultros.js';
await init();
const { openGuestListStore } = await import('/static/guest-list-store.mjs');
const store = await openGuestListStore();
await store.create({ name: 'Raid supplies', snapshot: new Uint8Array([1,2,3]) });
const { prepareGuestOffline }=await import('/static/guest-offline.mjs');
window.prepared=await prepareGuestOffline('/static/data/test/en.rkyv','en');
</script></body></html>`;

async function main() {
  console.log('Starting offline browser fixture');
  let disconnected = false;
  const server = http.createServer((req, res) => {
    if (disconnected) { req.socket.destroy(); return; }
    let body;
    let type = 'text/javascript';
    if (req.url === '/service-worker.js') body = fs.readFileSync(path.join(staticRoot, 'service-worker.js'));
    else if (['/static/guest-offline.mjs', '/static/guest-list-store.mjs', '/static/list-companion.mjs'].includes(req.url)) body = fs.readFileSync(path.join(staticRoot, path.basename(req.url)));
    else if (req.url === '/pkg/f4b41bf1/ultros.js') body = client;
    else if (req.url === '/pkg/f4b41bf1/ultros.wasm') { body = 'fixture'; type = 'application/wasm'; }
    else if (req.url === '/pkg/f4b41bf1/ultros.css') { body = 'body { color: white; background: #181020 }'; type = 'text/css'; }
    else if (req.url === '/static/data/test/en.rkyv') body = 'catalog fixture';
    else if (req.url === '/api/v1/world_data') { body = '{"worlds":[]}'; type = 'application/json'; }
    else if (req.url.startsWith('/list')) { body = html; type = 'text/html'; }
    else { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { 'Content-Type': type }); res.end(body);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
    console.log('Chromium started');
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/list/device/device%3Atest`);
    await page.waitForFunction(() => typeof window.prepared === 'boolean', { timeout: 90000 });
    assert.equal(await page.evaluate(() => window.prepared), true, 'offline preparation failed');
    // CDP's page offline emulation can leave the service-worker target online.
    // Refuse connections as well so its navigation fetch really fails.
    disconnected = true;
    await page.setOfflineMode(true);
    await page.reload({ waitUntil: 'domcontentloaded' });
    try {
      await page.waitForFunction(() => document.body.dataset.saved === 'Raid supplies');
    } catch (error) {
      console.error(await page.content(), errors);
      throw error;
    }
    assert.equal(await page.evaluate(() => document.body.dataset.offline), 'true');
    assert.equal(await page.evaluate(() => document.body.dataset.anonymous), 'true');
    assert.deepEqual(errors, []);
    console.log('PASS: installed production worker/helper, disconnected cold reload, anonymous shell, durable device data');
  } finally {
    await browser?.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
