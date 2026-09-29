// Exercise hydrated shared components; no account or live market data required.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const fs = require('node:fs');
const path = require('node:path');
const captures = path.join(__dirname, 'artifacts', 'accessibility');
fs.mkdirSync(captures, {recursive: true});
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';

(async () => {
  const browser = await puppeteer.launch({headless: true, args: ['--no-sandbox']});
  let page;
  try {
    const ssr = await browser.newPage();
    await ssr.setJavaScriptEnabled(false);
    await ssr.goto(`${BASE}/recipe-analyzer/Gilgamesh?world=Goblin&v=1&lang=en`, {waitUntil: 'domcontentloaded', timeout: 120000});
    assert.equal(await ssr.$eval('[data-testid="analyzer-world-picker"] input[role="combobox"]', el => el.value), 'Gilgamesh', 'selected world is present before hydration and follows the route');
    await ssr.close();
    page = await browser.newPage();
    page.setDefaultTimeout(30000);
    page.setDefaultNavigationTimeout(120000);
    const errors = [];
    page.on('pageerror', error => { errors.push(error.message); console.error('Page error:', error.message); });
    page.on('console', message => { if (message.type() === 'error') console.error('Browser console:', message.text().slice(0, 2000)); });
    await page.setRequestInterception(true);
    page.on('request', request => {
      const url = new URL(request.url());
      const result = url.hostname === 'pagead2.googlesyndication.com' && url.pathname.endsWith('/adsbygoogle.js')
        ? request.respond({status: 200, contentType: 'application/javascript', headers: {'Access-Control-Allow-Origin': '*'}, body: ''}) : request.continue();
      result.catch(error => errors.push(error.message));
    });
    await page.evaluateOnNewDocument(() => window.addEventListener('ultros:hydrated', () => { window.testHydrated = true; }));
    await page.setViewport({width: 1280, height: 800});
    await page.goto(`${BASE}/__test/accessibility?lang=en`);
    console.log('Loaded accessibility fixture');
    await page.waitForFunction(() => window.testHydrated === true);
    console.log('Fixture hydrated');
    if (!await page.$('.app-shell-collapsed')) await page.click('.side-nav-collapse');
    const axSession = await page.createCDPSession();
    const {nodes} = await axSession.send('Accessibility.getFullAXTree');
    assert(nodes.some(node => node.role?.value === 'link' && /^Recipes$/.test(node.name?.value || '')), 'collapsed navigation retains tool names');
    await axSession.detach();
    await page.focus('.skip-content');
    await page.keyboard.press('Enter');
    assert.equal(await page.evaluate(() => document.activeElement.id), 'main-content', 'skip link moves keyboard focus');
    await page.click('#open-fixture');
    await page.waitForSelector('dialog[open]');
    assert.equal(await page.$eval('dialog', el => el.getAttribute('aria-label')), 'Choose worlds');
    await page.$eval('#fixture-help', el => el.parentElement.focus());
    await page.waitForSelector('dialog [role=tooltip]');
    await page.keyboard.press('Escape');
    await page.waitForSelector('[role=tooltip]', {hidden: true});
    assert(await page.$('dialog[open]'), 'closing tooltip help does not close its dialog');
    await page.click('#fixture-dialog-error');
    await page.waitForSelector('dialog[open] .toast-error[role=alert]');
    const toastSession = await page.createCDPSession();
    const toastTree = await toastSession.send('Accessibility.getFullAXTree');
    assert(toastTree.nodes.some(node => !node.ignored && node.name?.value === 'Dialog fixture error'),
      'a modal error remains in the accessibility tree');
    await toastSession.detach();
    await page.focus('dialog[open] .toast-error button');
    assert(await page.evaluate(() => !!document.activeElement.closest('dialog[open] .toast-error')),
      'modal notifications can receive keyboard focus');
    await page.keyboard.press('Enter');
    await page.waitForSelector('.toast-error', {hidden: true});
    for (let i = 0; i < 12; i++) {
      await page.keyboard.press('Tab');
      assert(await page.evaluate(() => !!document.activeElement.closest('dialog[open]')), 'Tab stays in the task dialog');
    }
    const buy = '[role=combobox][aria-label="Buy world"]';
    const sell = '[role=combobox][aria-label="Sell world"]';
    await page.click('label[for=fixture-buy-world]');
    assert(await page.$eval(buy, el => el === document.activeElement), 'clicking the picker label focuses its input');
    await page.waitForSelector('dialog [role=listbox]');
    console.log('Picker popup opened inside dialog');
    const buyId = await page.$eval(buy, el => el.getAttribute('aria-controls'));
    assert(await page.$eval(buy, el => !!document.getElementById(el.getAttribute('aria-controls'))), 'combobox owns a real popup');
    await page.type(buy, 'no-match');
    await page.waitForFunction(() => document.querySelector('[role=listbox] [role=status]')?.textContent.includes('No matching'));
    console.log('Picker no-match state passed');
    await page.keyboard.press('Escape');
    assert(await page.$eval(buy, el => el === document.activeElement), 'Escape retains combobox focus');
    assert(await page.$('dialog[open]'), 'closing a popup does not close its dialog');
    await page.click(buy);
    await page.keyboard.down('Control'); await page.keyboard.press('A'); await page.keyboard.up('Control');
    await page.type(buy, 'Beta');
    await page.keyboard.press('Enter');
    assert.equal(await page.$eval(buy, el => el.value), 'Beta', 'selection is the actual input value');
    assert(await page.$eval(buy, el => el === document.activeElement), 'selection retains input focus');
    await page.click(buy);
    assert.equal(await page.$eval(buy, el => el.value), 'Beta', 'reopening retains the selected value');
    await page.keyboard.press('Escape');
    await page.focus(sell);
    const sellId = await page.$eval(sell, el => el.getAttribute('aria-controls'));
    assert.notEqual(buyId, sellId, 'picker IDs are instance-specific');
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    await page.waitForSelector('dialog', {hidden: true});
    assert.equal(await page.evaluate(() => document.activeElement.id), 'open-fixture', 'dialog restores focus');
    console.log('Dialog, tooltip and picker checks passed');
    await page.focus('button[aria-label="Alpha: Move down"]');
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => [...document.querySelectorAll('.fixture-row-name')].map(el => el.textContent).join() === 'Beta,Alpha,Gamma');
    await page.waitForFunction(() => document.activeElement.getAttribute('aria-label') === 'Alpha: Move down');
    assert.match(await page.$eval('.reorderable-list [role=status]', el => el.textContent), /Alpha.*2\/3/);
    await page.focus('button[aria-label="Alpha: Move up"]');
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => [...document.querySelectorAll('.fixture-row-name')].map(el => el.textContent).join() === 'Alpha,Beta,Gamma');
    await page.waitForFunction(() => document.activeElement.getAttribute('aria-label') === 'Alpha: Move up');
    assert.equal(await page.$eval('button[aria-label="Alpha: Move up"]', el => el.getAttribute('aria-disabled')), 'true');
    await page.click('#fixture-error-toast');
    await page.waitForSelector('.toast-error[role=alert]');
    await new Promise(resolve => setTimeout(resolve, 5500));
    assert(await page.$('.toast-error[role=alert]'), 'errors remain until dismissed');
    await page.click('.toast-error button');
    console.log('Reordering and persistent error checks passed');
    await page.setViewport({width: 390, height: 844});
    await page.click('.mobile-bar button[aria-label="Search"]');
    await page.waitForSelector('dialog.search-overlay[open]');
    assert(await page.$eval('.search-overlay-close', el => { const rect = el.getBoundingClientRect(); return rect.width >= 44 && rect.height >= 44; }), 'mobile search has a visible 44px exit target');
    await page.screenshot({path: path.join(captures, 'after-search-mobile.png')});
    await page.keyboard.down('Shift'); await page.keyboard.press('Tab'); await page.keyboard.up('Shift');
    assert(await page.evaluate(() => !!document.activeElement.closest('dialog[open]')), 'reverse Tab stays in search');
    await page.click('.search-overlay-close');
    await page.click('.mobile-bar button[aria-label="Menu"]');
    await page.waitForSelector('dialog.nav-dialog[open]');
    for (let i = 0; i < 30; i++) {
      await page.keyboard.press('Tab');
      assert(await page.evaluate(() => !!document.activeElement.closest('dialog.nav-dialog')), 'Tab stays in mobile menu');
    }
    await page.keyboard.press('Escape');
    await page.waitForSelector('dialog.nav-dialog', {hidden: true});
    assert.equal(await page.evaluate(() => document.activeElement.getAttribute('aria-label')), 'Menu');
    await page.goto(`${BASE}/__test/virtual-grid?lang=en&cards=true`);
    await page.waitForSelector('.mobile-grid-cards article');
    assert(await page.$eval('.mobile-grid-cards', el => el.scrollWidth <= el.clientWidth + 1), 'compact results fit mobile width');
    const before = await page.$eval('.mobile-grid-cards article', el => el.textContent);
    await page.click('#fixture-update');
    await page.waitForFunction(before => document.querySelector('.mobile-grid-cards article').textContent !== before, {}, before);
    await page.setViewport({width: 320, height: 700});
    assert(await page.$eval('.mobile-grid-cards', el => el.scrollWidth <= el.clientWidth + 1), 'compact results fit 320px');
    await page.screenshot({path: path.join(captures, 'compact-results-320.png')});
    await page.focus('.mobile-grid-cards article button');
    await page.keyboard.press('Enter');
    assert.equal(await page.$$eval('.mobile-grid-cards article:first-child .mobile-grid-field', fields => fields.length), 60, 'details preserve every visible table column');
    await page.click('.mobile-grid-cards > button');
    await page.waitForFunction(() => document.querySelectorAll('.mobile-grid-cards article').length === 40);
    await page.click('.virtual-grid-shell > button');
    await page.waitForSelector('.virtual-grid', {visible: true});
    await page.click('.virtual-grid-shell > button');
    await page.waitForSelector('.mobile-grid-cards article');
    await page.click('#fixture-empty');
    await page.waitForSelector('.mobile-grid-cards article', {hidden: true});
    assert(await page.$('.mobile-grid-cards [role=status]'), 'empty compact results announce their state');
    if (process.env.AUDIT_CAPTURE === '1') {
      await page.setViewport({width: 390, height: 844});
      await page.goto(BASE + '/recipe-analyzer?v=1', {waitUntil: 'networkidle0', timeout: 120000});
      await page.waitForFunction(() => window.testHydrated === true);
      await page.screenshot({path: path.join(captures, 'after-recipes-mobile.png')});
    }
    assert.deepEqual(errors, []);
    console.log('Accessibility: dialogs, picker ownership, no matches, keyboard reordering, mobile search and compact results passed');
  } catch (error) {
    if (page) {
      await page.screenshot({path: path.join(captures, 'failure.png')}).catch(() => {});
      console.error(await page.evaluate(() => ({
        active: document.activeElement?.outerHTML.slice(0, 700),
        pickers: [...document.querySelectorAll('[role=combobox]')].map(el => ({value: el.value, expanded: el.getAttribute('aria-expanded')})),
        popups: [...document.querySelectorAll('[role=listbox]')].map(el => el.textContent.slice(0, 400)),
      })).catch(() => 'Page unavailable'));
    }
    throw error;
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
