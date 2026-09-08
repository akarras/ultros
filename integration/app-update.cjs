// Run against this worktree's fresh cargo-leptos build. Only the listings
// response is replaced; the app, router, update detector and WASM are real.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const ITEM = '/item/Gilgamesh/5364';
const banner = '[role="status"]';

async function main() {
  const response = await fetch(BASE);
  const commit = response.headers.get('x-ultros-commit');
  assert(commit && commit !== 'dirty', 'server must expose a real build commit');
  for (const path of ['/api/v1/world_data', '/__missing_update_test']) {
    assert.equal((await fetch(BASE + path)).headers.get('x-ultros-commit'), commit);
  }
  const browser = await puppeteer.launch({headless: true, args: ['--no-sandbox']});
  try {
    for (const mode of ['matching', 'reload', 'dismiss-and-navigate']) {
      const page = await browser.newPage();
      page.setDefaultTimeout(90000);
      const errors = [];
      page.on('pageerror', error => {
        const message = error.stack || error.message;
        // Match the route runner's allowance for AdSense's headless-only error.
        if (!message.includes('pagead2.googlesyndication.com')) errors.push(message);
      });
      let observedFetches = 0;
      let header = mode === 'matching' ? commit : '0000000000000000000000000000000000000000';
      await page.evaluateOnNewDocument(() => {
        window.__updateTestDocument = Math.random().toString(36);
        window.__updateTestHydrated = false;
        window.addEventListener('ultros:hydrated', () => { window.__updateTestHydrated = true; });
      });
      await page.setRequestInterception(true);
      page.on('request', request => {
        if (!new URL(request.url()).pathname.startsWith('/api/v1/listings/')) return request.continue();
        observedFetches++;
        return request.respond({status: 200, contentType: 'application/json',
          headers: {'x-ultros-commit': header},
          body: JSON.stringify({listings: [], sales: [], last_updated: []})});
      });
      await page.goto(BASE + '/about', {waitUntil: 'networkidle2'});
      await page.waitForFunction(() => window.__updateTestHydrated);
      // Navigate from a hydrated page so the real API helper runs instead
      // of consuming a response serialized into the initial SSR document.
      await page.evaluate(href => {
        const link = document.createElement('a');
        link.href = href;
        document.body.append(link);
        link.click();
      }, ITEM);
      await page.waitForFunction(path => location.pathname === path, {}, ITEM);
      await page.waitForNetworkIdle();
      assert(observedFetches > 0, 'the client API helper must observe a response header');
      const visible = () => page.$$eval(banner, nodes => nodes.some(n => n.textContent.includes('Ultros has been updated')));
      if (mode === 'matching') {
        assert.equal(await visible(), false, 'matching build must not show a banner');
      } else {
        await page.waitForFunction(() => [...document.querySelectorAll('[role="status"]')].some(n => n.textContent.includes('Ultros has been updated')));
        const documentId = await page.evaluate(() => window.__updateTestDocument);
        if (mode === 'reload') {
          header = commit;
          await Promise.all([
            page.waitForNavigation({waitUntil: 'networkidle2'}),
            page.$$eval('[role="status"] button', buttons => buttons.find(b => b.textContent.trim() === 'Reload').click()),
          ]);
          await page.waitForFunction(() => window.__updateTestHydrated);
          assert.notEqual(await page.evaluate(() => window.__updateTestDocument), documentId);
          assert.equal(await visible(), false, 'reload onto matching build clears the notice');
        } else {
          await page.click('[role="status"] button[aria-label="Dismiss"]');
          assert.equal(await visible(), false);
          async function clickLink(href) {
            await page.evaluate(href => {
              const link = document.createElement('a');
              link.href = href;
              link.textContent = 'Update test destination';
              document.body.append(link);
              link.click();
            }, href);
          }
          await clickLink(ITEM + '?update-test=filter');
          await page.waitForFunction(() => location.search.includes('update-test=filter'));
          assert.equal(await page.evaluate(() => window.__updateTestDocument), documentId, 'query changes must not reload');
          header = commit;
          // Use the canonical locale query so ShareLocale does not append it.
          const destination = '/item/Gilgamesh/13709?update-test=destination&lang=en#details';
          await Promise.all([page.waitForNavigation({waitUntil: 'networkidle2'}), clickLink(destination)]);
          await page.waitForFunction(() => window.__updateTestHydrated);
          assert.equal(new URL(page.url()).pathname + new URL(page.url()).search + new URL(page.url()).hash, destination);
          assert.notEqual(await page.evaluate(() => window.__updateTestDocument), documentId, 'dismissed notice still reloads on path navigation');
          assert.equal(await visible(), false);
        }
      }
      assert.deepEqual(errors, [], `${mode}: no browser errors`);
      await page.close();
    }
    console.log('App updates: headers, matching version, reload, dismissal and destination navigation passed.');
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
