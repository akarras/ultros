// Invoked by shared-analyzer-data.cjs with its deterministic market fixtures.
const assert = require('node:assert/strict');
module.exports = async function flipSaleColumns({ page, base, route, openFixture, artifacts }) {
  const median = 'market-sale-median';
  const related = [median, 'market-sale-avg', 'market-sale-min', 'market-units', 'market-gil'];
  const heading = id => `.virtual-grid-heading[data-column="${id}"]`;
  const cell = id => `.virtual-grid-cell[data-column="${id}"]`;
  const defaults = { v: '1', lang: 'en', 'min-sales': '0', profit: '-1000000000',
    roi: '-1000000000', 'next-sale': '1M', sort: 'grid:item', dir: 'asc' };
  async function navigate(params = defaults) {
    await openFixture();
    await page.evaluate(href => {
      const link = document.createElement('a'); link.id = 'flip-columns-nav';
      link.href = href; document.querySelector('main').prepend(link); link.click();
    }, `${base}${route}?${new URLSearchParams(params)}`);
    await page.waitForSelector('.virtual-grid[data-auto-fitted]');
    await page.waitForFunction(() => location.pathname.startsWith('/flip-finder/'));
    await page.waitForSelector(heading('hq'));
    if (params.v) await page.waitForFunction(() => Number(document.querySelector('.virtual-grid')?.getAttribute('aria-rowcount')) > 1);
  }
  async function direct(params) {
    await page.deleteCookie({ name: 'ultros_last_flip-finder', url: base, path: '/flip-finder' });
    const response = await page.goto(`${base}${route}?${new URLSearchParams(params)}`, { waitUntil: 'domcontentloaded' });
    assert(response.ok(), 'direct Flip Finder visit succeeds');
    await page.waitForFunction(() => window.__queryHydrated);
    await page.waitForSelector('.virtual-grid');
  }
  async function columns() {
    const found = new Map();
    const width = await page.$eval('.virtual-grid', el => el.scrollWidth);
    for (let left = 0; left <= width + 500; left += 500) {
      await page.$eval('.virtual-grid', (el, left) => { el.scrollLeft = left; }, left);
      await new Promise(resolve => setTimeout(resolve, 100));
      for (const c of await page.$$eval('.virtual-grid-heading', els => els.map(el => ({
        id: el.dataset.column, index: Number(el.getAttribute('aria-colindex')),
      })))) found.set(c.id, c.index);
    }
    return [...found].sort((a, b) => a[1] - b[1]).map(([id]) => id);
  }
  async function reveal(id) {
    const width = await page.$eval('.virtual-grid', el => el.scrollWidth);
    for (let left = 0; left <= width + 500; left += 500) {
      await page.$eval('.virtual-grid', (el, left) => { el.scrollLeft = left; }, left);
      await new Promise(resolve => setTimeout(resolve, 100));
      if (await page.$(heading(id))) {
        await page.$eval(heading(id), el => el.scrollIntoView({ block: 'nearest', inline: 'center' }));
        return;
      }
    }
    assert.fail(`Missing column ${id}`);
  }
  async function aligned(id) {
    await reveal(id);
    await page.waitForSelector(cell(id));
    const geometry = await page.evaluate(id => {
      const h = document.querySelector(`.virtual-grid-heading[data-column="${id}"]`).getBoundingClientRect();
      const c = document.querySelector(`.virtual-grid-cell[data-column="${id}"]`).getBoundingClientRect();
      const label = document.querySelector(`.virtual-grid-heading[data-column="${id}"] [data-metric-sort] > span`);
      const range = document.createRange(); range.selectNodeContents(label);
      return { left: Math.abs(h.left - c.left), delta: Math.abs(h.width - c.width), width: h.width,
        clipped: range.getBoundingClientRect().width - label.getBoundingClientRect().width };
    }, id);
    assert(geometry.left < 1 && geometry.delta < 1, `${id}: header and body align`);
    assert(geometry.clipped < 0.1, `${id}: auto-fit includes the complete window label (${geometry.clipped}px clipped)`);
    return geometry.width;
  }
  console.log('CHECK Flip Finder sale columns: fresh and seeded views');
  await page.evaluate(() => localStorage.clear());
  // A truly fresh visit seeds filters but leaves columns absent to inherit defaults.
  await direct({ lang: 'en' });
  await page.waitForFunction(() => new URL(location.href).searchParams.has('min-buy'));
  assert.equal(new URL(page.url()).searchParams.has('cols'), false);
  let ids = await columns();
  assert.equal(ids[ids.indexOf('sale_estimate') + 1], median);
  for (const id of related.slice(1)) assert(!ids.includes(id), `${id} stays opt-in`);
  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => window.__queryHydrated);
  await page.waitForSelector('.virtual-grid');
  assert.equal(new URL(page.url()).searchParams.has('cols'), false);
  ids = await columns();
  assert.equal(ids[ids.indexOf('sale_estimate') + 1], median);

  console.log('CHECK Flip Finder sale columns: picker and market window');
  await navigate();
  await page.click('button[aria-label="Columns"]');
  async function pickerEntry(prefix) {
    for (const label of await page.$$('label')) {
      if (await label.evaluate((el, prefix) => el.textContent.trim().startsWith(prefix)
        && !!el.querySelector('input[type="checkbox"]'), prefix)) return label;
    }
    assert.fail(`Missing picker entry ${prefix}`);
  }
  assert(await (await pickerEntry('Sale median')).$eval('input', el => el.checked));
  assert.equal(await (await pickerEntry('Sale average')).$eval('input', el => el.checked), false);
  assert.equal(await (await pickerEntry('Active listings')).$eval('input', el => el.checked), false, 'listing columns are off by default');
  assert(await page.$$eval('span.basis-full', spans => spans.some(el => el.textContent.trim() === 'Listings')), 'picker groups the alive set under Listings');
  await (await pickerEntry('Sale average')).click();
  await page.waitForFunction(() => new URL(location.href).searchParams.get('cols')?.split(',').includes('market-sale-avg'));
  assert(new URL(page.url()).searchParams.get('cols').split(',').includes(median));
  assert(new URL(page.url()).searchParams.get('cols').split(',').includes('world'), 'shared toggle keeps native defaults');
  await page.keyboard.press('Escape');
  await navigate();
  await reveal(median);
  await page.waitForFunction(selector => [...document.querySelectorAll(selector)].some(el => /900/.test(el.textContent)), {}, cell(median));
  assert.match(await page.$eval(heading(median), el => el.textContent), /7d/);
  await page.select('[data-market-window]', '30');
  await reveal(median);
  await page.waitForFunction(selector => [...document.querySelectorAll(selector)].some(el => /1[,. ]?800/.test(el.textContent)), {}, cell(median));
  assert.match(await page.$eval(heading(median), el => el.textContent), /30d/);

  await navigate({ ...defaults, cols: '' });
  assert.deepEqual(await columns(), ['hq', 'item', 'profit', 'buy_price']);
  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => window.__queryHydrated);
  await page.waitForSelector('.virtual-grid');
  assert.equal(new URL(page.url()).searchParams.get('cols'), '');
  assert.deepEqual(await columns(), ['hq', 'item', 'profit', 'buy_price']);

  await navigate({ ...defaults, cols: 'market-sale-median-7', window: '30' });
  assert.deepEqual(await columns(), ['hq', 'item', 'profit', 'buy_price', 'market-sale-median-7']);
  await reveal('market-sale-median-7');
  assert.match(await page.$eval(heading('market-sale-median-7'), el => el.textContent), /7d/);

  const explicit = ['sale_estimate', ...related, 'market-sale-median-7'].join(',');
  await navigate({ ...defaults, cols: explicit, window: '30' });
  ids = await columns();
  assert.deepEqual(ids.slice(ids.indexOf('sale_estimate') + 1, ids.indexOf('sale_estimate') + 6), related);
  const widths = {};
  for (const id of related) widths[id] = await aligned(id);
  console.log('Flip Finder populated auto-fit widths:', JSON.stringify(widths));
  await reveal('market-sale-median-7');
  assert.match(await page.$eval(heading('market-sale-median-7'), el => el.textContent), /7d/);
  await reveal(median);
  await page.click(heading(median), { button: 'right' });
  await page.waitForSelector('.grid-menu-panel');
  let autoFit;
  for (const button of await page.$$('.grid-menu-panel button')) {
    if ((await button.evaluate(el => el.textContent.trim())) === 'Auto-fit column') { autoFit = button; break; }
  }
  assert(autoFit, 'shared column menu offers auto-fit');
  await autoFit.click();
  await page.waitForFunction(() => !document.querySelector('.grid-menu-panel'));
  await aligned(median);

  console.log('CHECK Flip Finder sale columns: saved layouts and mobile');
  // Both old JSON layouts and current compact URLs retain explicit order/width.
  const order = ['market-sale-avg', 'item', median, 'sale_estimate'];
  for (const layout of [
    { layout: JSON.stringify({ v: 1, order, widths: { [median]: 222 } }) },
    { l: `2~${order.join('.')}~${median}.${(222).toString(36)}` },
  ]) {
    await navigate({ ...defaults, cols: explicit, ...layout });
    assert.deepEqual((await columns()).slice(0, 4), order);
    assert.equal(await aligned(median), 222);
    const saved = page.url();
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__queryHydrated);
    await page.waitForSelector('.virtual-grid');
    assert.equal(page.url(), saved);
    assert.deepEqual((await columns()).slice(0, 4), order);
    await reveal(median);
    assert.equal(await page.$eval(heading(median), el => el.getBoundingClientRect().width), 222);
    // Reload's SSR resources read the local DB, outside browser interception.
    // Preserve the layout even with no deals; populated alignment is checked
    // above and again here whenever the server supplies rows.
    if (await page.$eval('.virtual-grid', el => Number(el.getAttribute('aria-rowcount')) > 1)) {
      await aligned(median);
    }
  }
  // A saved landing view's explicit empty selection survives default seeding.
  await page.evaluate(() => {
    localStorage.removeItem('ultros.last-view.flip-finder');
    localStorage.setItem('ultros.flipfinder.default_view', '?v=1&cols=&l=2~item.buy_price~item.a0');
  });
  await direct({ lang: 'en' });
  await page.waitForFunction(() => new URL(location.href).searchParams.get('cols') === '');
  assert.equal(new URL(page.url()).searchParams.get('l'), '2~item.buy_price~item.a0');
  assert.deepEqual(await columns(), ['item', 'buy_price', 'hq', 'profit']);
  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => window.__queryHydrated);
  await page.waitForSelector('.virtual-grid');
  assert.equal(new URL(page.url()).searchParams.get('cols'), '');
  await page.evaluate(() => localStorage.removeItem('ultros.flipfinder.default_view'));
  await page.setViewport({ width: 393, height: 844, isMobile: true, hasTouch: true });
  await navigate();
  assert(await page.$eval('.virtual-grid', el => el.scrollWidth > el.clientWidth));
  await aligned(median);
  assert(await page.$eval('.virtual-grid', el => el.scrollLeft > 0));
  await page.screenshot({ path: `${artifacts}/flip-sale-columns-mobile.png`, fullPage: true });
  await page.setViewport({ width: 1600, height: 1000 });
  console.log('PASS Flip Finder sale columns: seeded/absent/empty/explicit, reload, follow/fixed windows, default/saved order, widths, auto-fit alignment, mobile scroll');
};
