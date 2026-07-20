// #6831 residual probe: is hydration starting before the (1.3MB) SSR body has
// finished parsing? Hooks console in page context so we can snapshot
// document.readyState + body child count SYNCHRONOUSLY at the moment wasm logs
// "hydrating body" and at the moment it logs the hydration.rs panic.
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'https://ultros.app';
const ITEM_PATH = process.env.ITEM_PATH || '/item/Twintania/13114';
const N = Number(process.env.N || 10);
const CPU = Number(process.env.CPU || 1);
const MOBILE = (process.env.MOBILE || 'on') === 'on';

const initScript = () => {
  window.__marks = [];
  const snap = (label) => {
    window.__marks.push({
      label,
      t: Math.round(performance.now()),
      readyState: document.readyState,
      bodyKids: document.body ? document.body.childNodes.length : -1,
      // Has the SSR document finished? The last body child on a complete page
      // is a <script>; while streaming, the tail is still missing.
      lastKid: document.body && document.body.lastChild
        ? (document.body.lastChild.nodeName || '?')
        : '?',
    });
  };
  const wrap = (fn) =>
    function (...args) {
      try {
        const s = args.map((a) => (typeof a === 'string' ? a : '')).join(' ');
        if (s.indexOf('hydrating body') !== -1) snap('hydrating-body');
        if (s.indexOf('SSR document truncated') !== -1) snap('GUARD-skipped');
        if (s.indexOf('hydration.rs:163') !== -1) snap('PANIC-163');
        if (s.indexOf('RefCell already borrowed') !== -1) snap('refcell');
      } catch (_) {}
      return fn.apply(this, args);
    };
  console.info = wrap(console.info);
  console.log = wrap(console.log);
  console.error = wrap(console.error);
  document.addEventListener('DOMContentLoaded', () => snap('DCL'));
  window.addEventListener('load', () => snap('load'));
};

(async () => {
  const browser = await puppeteer.launch({
    headless: 'new',
    args: ['--no-sandbox', '--disable-dev-shm-usage'],
  });
  const rows = [];
  for (let i = 0; i < N; i++) {
    const page = await browser.newPage();
    await page.setUserAgent(
      'Mozilla/5.0 (Linux; Android 4.0.4; Galaxy Nexus Build/IMM76B) AppleWebKit/537.36 (KHTML, like Gecko; Mediapartners-Google) Chrome/149.0.7827.200 Mobile Safari/537.36'
    );
    if (MOBILE) await page.setViewport({ width: 360, height: 640, isMobile: true });
    await page.evaluateOnNewDocument(initScript);
    const { hostname } = new URL(BASE_URL);
    await page.setCookie({ name: 'HIDE_ADS', value: 'true', domain: hostname, path: '/' });
    await page.setRequestInterception(true);
    page.on('request', (req) => {
      if (/googlesyndication|doubleclick|adtrafficquality|googletagservices|google-analytics|fundingchoices/.test(req.url()))
        return req.abort().catch(() => {});
      req.continue().catch(() => {});
    });
    const client = await page.target().createCDPSession();
    if (CPU > 1) await client.send('Emulation.setCPUThrottlingRate', { rate: CPU });

    // Ground-truth detector (same one that caught panics in ad-race.cjs), kept
    // alongside the in-page console wrapper so a missed wrap can't read as clean.
    let cdpPanic = false;
    page.on('console', (m) => { if (/hydration\.rs:163/.test(m.text())) cdpPanic = true; });
    page.on('pageerror', (e) => { if (/unreachable/i.test(String(e))) cdpPanic = true; });

    try {
      await page.goto(`${BASE_URL}${ITEM_PATH}?cb=${Date.now()}-${i}`, {
        waitUntil: 'domcontentloaded',
        timeout: 60000,
      });
    } catch (_) {}
    await new Promise((r) => setTimeout(r, Number(process.env.SETTLE || 12000)));

    let marks = [];
    try { marks = await page.evaluate(() => window.__marks); } catch (_) {}
    const hyd = marks.find((m) => m.label === 'hydrating-body');
    const guard = marks.find((m) => m.label === 'GUARD-skipped');
    const panic = marks.find((m) => m.label === 'PANIC-163') || cdpPanic;
    rows.push({ panicked: !!panic, hyd, guard: !!guard });
    console.log(
      `trial ${String(i).padStart(2)}: panic=${panic ? 'YES' : 'no '} guardSkipped=${guard ? 'YES' : 'no '} ` +
        `| at hydrating-body: readyState=${hyd ? hyd.readyState : '?'} bodyKids=${hyd ? hyd.bodyKids : '?'} lastKid=${hyd ? hyd.lastKid : '?'} t=${hyd ? hyd.t : '?'}ms`
    );
    await page.close();
  }
  await browser.close();

  const grp = (f) => rows.filter(f);
  const summarize = (label, list) => {
    if (!list.length) return console.log(`${label}: n=0`);
    const states = {};
    const kids = {};
    for (const r of list) {
      const s = r.hyd ? r.hyd.readyState : '?';
      states[s] = (states[s] || 0) + 1;
      const k = r.hyd ? r.hyd.bodyKids : -1;
      kids[k] = (kids[k] || 0) + 1;
    }
    console.log(`${label}: n=${list.length} readyState@hydrate=${JSON.stringify(states)} bodyKids@hydrate=${JSON.stringify(kids)}`);
  };
  console.log('');
  summarize('PANICKED ', grp((r) => r.panicked));
  summarize('CLEAN    ', grp((r) => !r.panicked));
  console.log(`\ntotal: ${grp((r) => r.panicked).length}/${rows.length} panicked  (CPU=${CPU}x MOBILE=${MOBILE})`);
  process.exit(0);
})();
