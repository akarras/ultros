// Requires populated markets on WORLD and OTHER_WORLD. Exercises real SSR,
// WASM hydration, router navigation, and the existing websocket refetch path.
// The controlled Stale message is delivered to the browser's real websocket;
// no HTML, API response, or application state is replaced by this probe.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD = process.env.WORLD || 'Gilgamesh';
const OTHER_WORLD = process.env.OTHER_WORLD || 'Sargatanas';
const TIMEOUT = Number(process.env.TIMEOUT_MS || 90000);
const TABLE = '[data-testid="flip-finder-table"]';
const route = (world) => `/flip-finder/${encodeURIComponent(world)}?next-sale=&last-sold=&min-buy=0`;

async function main() {
  assert.notEqual(WORLD, OTHER_WORLD, 'world-switch probe requires two distinct worlds');
  const browser = await puppeteer.launch({
    headless: 'new',
    args: ['--no-sandbox', '--disable-dev-shm-usage'],
  });
  const errors = [];
  let releaseRequest;
  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(TIMEOUT);
    await page.setViewport({ width: 1440, height: 1000 });
    await page.setCookie({ name: 'HIDE_ADS', value: 'true', url: BASE_URL });
    page.on('console', (message) => {
      if (message.type() === 'error') errors.push(message.text());
    });
    page.on('pageerror', (error) => errors.push(error.stack || String(error)));
    await page.evaluateOnNewDocument(() => {
      window.__flipHydrated=false;
      window.addEventListener('ultros:hydrated',()=>{window.__flipHydrated=true;});
      // Record real subscription IDs so a Stale message reaches the same
      // handler used when the server detects missed market updates.
      const send = WebSocket.prototype.send;
      window.__flipSockets = new Map();
      WebSocket.prototype.send = function (payload) {
        try {
          const message = JSON.parse(payload);
          let subscriptions = window.__flipSockets.get(this);
          if (!subscriptions) {
            subscriptions = new Map();
            window.__flipSockets.set(this, subscriptions);
          }
          if (message.AddSubscribe) {
            subscriptions.set(message.AddSubscribe.subscription_id, message.AddSubscribe);
          }
          if (message.Unsubscribe) subscriptions.delete(message.Unsubscribe.subscription_id);
        } catch (_) {
          // Non-JSON websocket traffic is still sent unchanged.
        }
        return send.call(this, payload);
      };
      window.__flipDocument = {};
    });

    async function ready(world, requireRows = true) {
      await page.waitForFunction(()=>window.__flipHydrated);
      await page.waitForFunction((world) => document.title.includes(world), {}, world);
      await page.waitForSelector(TABLE);
      if (requireRows) {
        await page.waitForFunction((selector) =>
          [...document.querySelectorAll(`${selector} a[href]`)]
            .some((a) => /^\/item\/[^/]+\/\d+(?:\?|$)/.test(a.getAttribute('href'))), {}, TABLE);
      }
      // An interactive popover proves hydration completed, rather than merely
      // accepting the table and title already present in server HTML.
      const columns = `${TABLE} button[aria-label="Columns"]`;
      await page.click(columns);
      await page.waitForFunction((selector) =>
        document.querySelector(selector)?.getAttribute('aria-expanded') === 'true', {}, columns);
      await page.click(columns);
      await page.waitForFunction((selector) =>
        document.querySelector(selector)?.getAttribute('aria-expanded') === 'false', {}, columns);
      assert.deepEqual(errors, [], `browser errors on ${world}`);
    }

    async function navigate(path) {
      await page.evaluate((path) => {
        window.__flipBeforeNavigation = window.__flipDocument;
        const anchor = document.createElement('a');
        anchor.href = path;
        document.body.append(anchor);
        anchor.click();
        anchor.remove();
      }, path);
      await page.waitForFunction((path) => location.pathname === new URL(path, location.origin).pathname, {}, path);
      assert.equal(await page.evaluate(() => window.__flipDocument === window.__flipBeforeNavigation), true,
        'navigation must reuse the hydrated document');
    }

    // Bare shared URLs must hydrate before default-view seeding runs.
    const response = await page.goto(`${BASE_URL}/flip-finder/${encodeURIComponent(WORLD)}`, {
      waitUntil: 'networkidle0', timeout: TIMEOUT,
    });
    assert.equal(response.status(), 200);
    const html = await response.text();
    assert.ok(html.includes(`Flip Finder - ${WORLD}`), 'SSR title missing');
    assert.ok(html.includes('data-testid="flip-finder-table"'), 'SSR table missing');
    await ready(WORLD, false);
    console.log('[ok] direct world URL hydrates with interactive controls');

    await navigate('/items');
    await page.waitForFunction(() => document.title.includes('Items Explorer'));
    await navigate(route(WORLD));
    await ready(WORLD);
    console.log('[ok] client navigation renders populated results');
    await navigate(route(OTHER_WORLD));
    await ready(OTHER_WORLD);
    await navigate(route(WORLD));
    await ready(WORLD);
    console.log('[ok] world round-trip stays hydrated');

    await page.waitForFunction(() => [...window.__flipSockets].some(([socket, subscriptions]) =>
      socket.readyState === WebSocket.OPEN && [...subscriptions.values()].some((s) => s.msg_type === 'Listings')));
    await page.evaluate((selector) => { window.__flipBeforeRefresh = document.querySelector(selector); }, TABLE);
    const isRefresh = (request) => {
      const url = new URL(request.url());
      return url.pathname === `/api/v1/cheapest/${WORLD}` && Number(url.searchParams.get('rt')) > 0;
    };
    let held = false;
    await page.setRequestInterception(true);
    page.on('request', (request) => {
      if (!held && isRefresh(request)) {
        held = true;
        releaseRequest = () => request.continue();
      } else {
        request.continue();
      }
    });
    const refreshRequested = page.waitForRequest(isRefresh);
    await page.evaluate(() => {
      for (const [socket, subscriptions] of window.__flipSockets) {
        if (socket.readyState !== WebSocket.OPEN) continue;
        for (const [id, subscription] of subscriptions) {
          if (subscription.msg_type === 'Listings') {
            socket.dispatchEvent(new MessageEvent('message', { data: JSON.stringify({ Stale: { subscription_id: id } }) }));
          }
        }
      }
    });
    await refreshRequested;
    assert.equal(held, true, 'refetch request was not held');
    await page.waitForFunction((selector) => {
      const table = document.querySelector(selector);
      return table === window.__flipBeforeRefresh && table?.isConnected && table.getClientRects().length > 0;
    }, {}, TABLE);
    const refreshed = page.waitForResponse((response) => isRefresh(response.request()));
    await releaseRequest();
    releaseRequest = null;
    assert.equal((await refreshed).status(), 200, 'market refetch failed');
    await page.waitForNetworkIdle({ idleTime: 500, timeout: TIMEOUT });
    await ready(WORLD);
    assert.equal(await page.evaluate((selector) => document.querySelector(selector) === window.__flipBeforeRefresh, TABLE), true,
      'market refetch remounted the table');
    assert.deepEqual(errors, [], 'strict browser console must remain clean');
    console.log('[ok] real market refetch preserves the mounted interactive table');

    await page.setViewport({width:393,height:844,isMobile:true,hasTouch:true,deviceScaleFactor:1});
    await ready(WORLD);
    const bounds=await page.$eval(`${TABLE} .virtual-grid`,e=>{
      const r=e.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height};
    });
    assert(await page.evaluate(()=>document.querySelector('.virtual-grid').getBoundingClientRect().bottom<=document.querySelector('.mobile-bar').getBoundingClientRect().top),
      'mobile navigation must not cover Flip Finder results');
    const touch=await page.createCDPSession();
    const x=bounds.x+bounds.width-30,y=bounds.y+Math.min(bounds.height-30,160);
    await touch.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x,y}]});
    for(let i=1;i<=12;i++) {
      await touch.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:x-220*i/12,y:y-60*i/12}]});
      await new Promise(r=>setTimeout(r,25));
    }
    await touch.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    await page.waitForFunction(()=>document.querySelector('.virtual-grid').scrollLeft>100);
    assert(await page.$eval('.virtual-grid',port=>{
      const bounds=port.getBoundingClientRect();
      return [...port.querySelectorAll('[role=columnheader]')].every(header=>{
        const h=header.getBoundingClientRect();
        if(h.right<=bounds.left||h.left>=bounds.right)return true;
        const c=port.querySelector(`.virtual-grid-cell[data-column="${header.dataset.column}"]`)?.getBoundingClientRect();
        return c&&Math.abs(c.left-h.left)<=1&&Math.abs(c.width-h.width)<=1&&Math.abs(h.top-bounds.top-1)<=1;
      });
    }),'populated Flip Finder headings stay aligned after a touch swipe');
    const artifactDir=require('node:path').join(__dirname,'artifacts','virtual-grid');
    require('node:fs').mkdirSync(artifactDir,{recursive:true});
    await page.screenshot({path:require('node:path').join(artifactDir,'flip-finder-touch-393.png'),fullPage:true});
    await touch.detach();
    assert.deepEqual(errors, [], 'mobile hydration and scrolling remain error-free');
    console.log('[ok] populated Flip Finder supports mobile touch scrolling with aligned headings and clear navigation');
  } finally {
    if (releaseRequest) await releaseRequest().catch(() => {});
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error.stack || error);
  process.exitCode = 1;
});
