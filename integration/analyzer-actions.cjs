// Exercise the real hydrated controls with deterministic market data.
// No list writes: dialogs are inspected and dismissed before submitting.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const puppeteer = require('puppeteer');
const { marketFixture, itemIds } = require('./shared-analyzer-market-fixture.cjs');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const output = path.join(__dirname, 'artifacts', 'analyzer-actions');

async function main() {
  assert(['127.0.0.1', 'localhost', '[::1]'].includes(new URL(BASE).hostname), 'Use an isolated local server');
  fs.mkdirSync(output, { recursive: true });
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(90000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const fixture = marketFixture([...itemIds, 25, 6766]);
  await page.evaluateOnNewDocument(() => {
    window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; });
    window.__copyMode = 'success';
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      writeText(text) {
        window.__copiedText = text;
        if (window.__copyMode === 'pending') return new Promise(resolve => { window.__finishCopy = resolve; });
        if (window.__copyMode === 'reject') return Promise.reject(new DOMException('Test blocked copy', 'NotAllowedError'));
        return Promise.resolve();
      },
    }});
  });
  await page.setCookie({ name: 'HOME_WORLD', value: 'Gilgamesh', url: BASE }, { name: 'HIDE_ADS', value: 'true', url: BASE });
  await page.setRequestInterception(true);
  page.on('request', request => {
    if (request.isInterceptResolutionHandled()) return;
    const url = new URL(request.url());
    if (url.origin !== new URL(BASE).origin && !['data:', 'blob:'].includes(url.protocol)) return request.abort();
    const response = fixture.reply(request, body => {
      // Both market qualities are represented; Vendor Sell must carry HQ.
      if (body.cheapest_listings) for (const row of body.cheapest_listings) {
        if (row.item_id === 1602) { row.hq = true; row.cheapest_price = 1; }
      }
    });
    return response ? request.respond(response) : request.continue();
  });
  async function navigate(route) {
    await page.goto(`${BASE}/__test/shared-analyzer-data?lang=en`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__hydrated);
    await page.evaluate(href => {
      const a = document.createElement('a'); a.href = href; document.querySelector('main').prepend(a); a.click();
    }, `${BASE}${route}`);
    await page.waitForFunction(pathname => location.pathname === pathname, {}, new URL(`${BASE}${route}`).pathname);
    await page.waitForSelector('[data-item-actions] .clipboard');
  }
  const routes = [
    ['recipes', '/recipe-analyzer/Gilgamesh'],
    ['fc', '/fc-crafting-analyzer/Gilgamesh'],
    ['leves', '/leve-analyzer/Gilgamesh'],
    ['ventures', '/venture-analyzer/Gilgamesh'],
    ['vendor-resale', '/vendor-resale/Gilgamesh'],
    ['scrips', '/scrip-sources/Gilgamesh'],
    ['exchange', '/currency-exchange/25?world=Gilgamesh'],
    ['flips', '/flip-finder/Gilgamesh'],
    ['trends', '/trends/Gilgamesh'],
    ['vendor-sell', '/vendor-sell/Gilgamesh'],
  ];
  try {
    for (const width of [1440, 390]) {
      await page.setViewport({ width, height: 900 });
      for (const [name, route] of routes) {
        console.log(`${width}: ${name}`);
        await navigate(route);
        const controls = await page.$$eval('[data-item-actions]', groups => groups.filter(group => group.checkVisibility()).map(group => {
          const cell = group.closest('[role="gridcell"], .mobile-grid-field');
          // Cards reuse the table's column labels without claiming gridcell semantics.
          const label = cell?.classList.contains('mobile-grid-field') && cell.firstElementChild.textContent.trim();
          const heading = label && [...group.closest('.virtual-grid-shell').querySelectorAll('.virtual-grid-heading')]
            .find(header => header.querySelector('.grid-heading-content')?.textContent.trim().startsWith(label));
          const buttons = [...group.querySelectorAll('button')];
          const rect = group.getBoundingClientRect(), bounds = cell?.getBoundingClientRect();
          return {
            column: cell?.dataset.column || heading?.dataset.column,
            first: buttons[0]?.classList.contains('clipboard'),
            extraTooltipStops: group.querySelectorAll('button [tabindex="0"], [tabindex="0"]:has(button:not(:disabled))').length,
            labels: buttons.map(b => b.getAttribute('aria-label')),
            fits: bounds && rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1,
          };
        }));
        assert(controls.length, `${name}: no controls`);
        for (const control of controls) {
          assert(['item', 'market-ingredient', 'cost'].includes(control.column), `${name}: controls outside item identity`);
          assert(control.first, `${name}: Copy must be first`);
          assert.equal(control.extraTooltipStops, 0, `${name}: tooltip adds a redundant keyboard stop`);
          assert(control.labels.every(Boolean), `${name}: unnamed action`);
          assert(control.fits, `${name}: actions overflow their item cell`);
        }
        assert.equal(await page.$('.virtual-grid-heading[data-column="actions"]'), null, `${name}: separate Actions column`);
        const url = page.url();
        const expected = await page.$eval('[data-item-actions] .clipboard', b => b.getAttribute('aria-label').replace(/^Copy /, '').replace(/ to clipboard$/, ''));
        await page.click('[data-item-actions] .clipboard');
        await page.waitForFunction(() => document.querySelector('[data-item-actions] .clipboard')?.getAttribute('aria-label').startsWith('Copied '));
        assert.equal(await page.evaluate(() => window.__copiedText), expected);
        assert.equal(page.url(), url, `${name}: Copy navigated`);
        if (name === 'exchange') {
          const currencyButtons = await page.$$eval('[data-item-actions="25"]', groups => groups.map(group => group.querySelectorAll('button').length));
          // The optional cost column is hidden by the default mobile view.
          if (width === 1440) assert(currencyButtons.length, 'Desktop exchange must show its currency cost');
          assert(currencyButtons.every(count => count === 1), 'Wolf Marks must only have Copy');
        }
        await page.screenshot({ path: path.join(output, `${name}-${width}.png`) });
      }
    }
    await page.setViewport({ width: 1440, height: 900 });
    await navigate('/vendor-sell/Gilgamesh');
    await page.waitForSelector('[data-item-actions="1602"] button:not(.clipboard)');
    await page.click('[data-item-actions="1602"] button:not(.clipboard)');
    await page.waitForSelector('[role="dialog"] [role="switch"]');
    assert.equal(await page.$eval('[role="dialog"] [role="switch"]', b => b.getAttribute('aria-checked')), 'true', 'HQ row must open as HQ');
    await page.keyboard.press('Escape');
    await page.waitForSelector('[role="dialog"]', { hidden: true });

    await navigate('/leve-analyzer/Gilgamesh');
    const leveGroup = await page.$('[data-item-actions]:has(button:not(.clipboard))');
    const quantity = await leveGroup.evaluate(group => Number(group.closest('[role="gridcell"]').textContent.match(/×\s*(\d+)/)[1]));
    await (await leveGroup.$('button:not(.clipboard)')).click();
    await page.waitForSelector('[role="dialog"] input[type="number"]');
    assert.equal(await page.$eval('[role="dialog"] input[type="number"]', input => Number(input.value)), quantity, 'Leve uses one turn-in quantity');
    await page.keyboard.press('Escape');
    await page.waitForSelector('[role="dialog"]', { hidden: true });

    await navigate('/recipe-analyzer/Gilgamesh');
    // Select by the visible operation, not the other list icon on the row.
    const ingredientButtons = await page.$$('button');
    let opened = false;
    for (const button of ingredientButtons) if ((await button.evaluate(b => b.textContent.trim())) === 'Add ingredients…') {
      await button.click(); opened = true; break;
    }
    assert(opened, 'Recipe ingredient shortcut remains available');
    await page.waitForSelector('[role="dialog"] input[id^="craft-qty-"]');
    await page.keyboard.press('Escape');
    await page.waitForSelector('[role="dialog"]', { hidden: true });

    await navigate('/vendor-resale/Gilgamesh');
    await page.evaluate(() => { window.__copyMode = 'pending'; });
    await page.click('[data-item-actions] .clipboard');
    assert.match(await page.$eval('[data-item-actions] .clipboard', b => b.getAttribute('aria-label')), /^Copy /, 'Pending copy must not show success');
    await page.evaluate(() => window.__finishCopy());
    await page.waitForFunction(() => document.querySelector('[data-item-actions] .clipboard').getAttribute('aria-label').startsWith('Copied '));
    const second = (await page.$$('[data-item-actions] .clipboard'))[1];
    await page.evaluate(() => { window.__copyMode = 'reject'; });
    await second.click();
    await page.waitForFunction(() => document.body.textContent.includes("Couldn't copy."));
    assert.match(await second.evaluate(b => b.getAttribute('aria-label')), /^Copy /, 'Rejected copy must not show success');
    assert.deepEqual(errors, [], 'No hydration or unhandled promise errors');
    console.log('Analyzer inline actions passed');
  } catch (error) {
    console.error('Page errors:', errors);
    console.error('Page:', page.url());
    console.error(await page.$eval('[data-item-actions]', group => group.outerHTML).catch(() => 'No item actions'));
    console.error((await page.evaluate(() => document.body.innerText)).slice(-2500));
    await page.screenshot({ path: path.join(output, 'failure.png') });
    throw error;
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
