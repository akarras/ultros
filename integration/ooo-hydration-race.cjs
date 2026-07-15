// Diagnostic for GlitchTip #6831 — "RustWasmPanic: internal error: entered
// unreachable code" at tachys `hydration.rs:163` (`failed_to_cast_element`).
//
// WHAT IT MEASURES
// Out-of-order SSR streaming emits each resolved <Suspense> as a <template>
// plus an inline script that relocates the fragment into place
// (`insertBefore(tpl.content.cloneNode(true), close)`). Those relocations
// reshape the DOM that `hydrate_body()` is about to walk. This script counts
// the relocations per page load (by hooking `Node.prototype.insertBefore` for
// DocumentFragment inserts at document-start) and correlates them with the
// hydration panic.
//
// MEASURED AGAINST PROD (release ultros@586c819, 12 loads, ads disabled):
//   relocations >= 4 -> panic 8/8
//   relocations <= 1 -> panic 0/3
// i.e. the panic is caused by out-of-order streaming, NOT by page translation
// or by AdSense (it reproduces with ads fully off and zero <font> injection).
//
// USE
//   BASE_URL=https://ultros.app node ./ooo-hydration-race.cjs
//   ITEM_PATH=/item/Sargatanas/16970 N=12 node ./ooo-hydration-race.cjs
//
// Expected after the SsrMode::InOrder wiring fix: relocations 0 on every load,
// and no panics.
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'https://ultros.app';
const ITEM_PATH = process.env.ITEM_PATH || '/item/%E7%90%A5%E7%8F%80%E5%8E%9F/16970';
const N = Number(process.env.N || 10);

// Count DocumentFragment insertions — the signature of a <template> relocation.
const initScript = () => {
  window.__ooo = 0;
  const orig = Node.prototype.insertBefore;
  Node.prototype.insertBefore = function (node, ref) {
    if (node && node.nodeType === 11 /* DOCUMENT_FRAGMENT_NODE */) window.__ooo++;
    return orig.call(this, node, ref);
  };
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
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36'
    );
    // Disable ads two ways, so AdSense cannot confound the measurement.
    const { hostname } = new URL(BASE_URL);
    await page.setCookie({ name: 'HIDE_ADS', value: 'true', domain: hostname, path: '/' });
    await page.evaluateOnNewDocument(initScript);
    await page.setRequestInterception(true);
    page.on('request', (req) => {
      const u = req.url();
      if (/googlesyndication|doubleclick|adtrafficquality|googletagservices|google-analytics/.test(u)) {
        return req.abort().catch(() => {});
      }
      req.continue().catch(() => {});
    });

    let panicked = false;
    page.on('console', (m) => {
      if (/hydration\.rs:163/.test(m.text())) panicked = true;
    });
    page.on('pageerror', (e) => {
      if (/unreachable/i.test(String(e))) panicked = true;
    });

    // Cache-bust so each trial gets a fresh server render.
    await page.goto(`${BASE_URL}${ITEM_PATH}?cb=${Date.now()}-${i}`, {
      waitUntil: 'domcontentloaded',
      timeout: 60000,
    });
    await new Promise((r) => setTimeout(r, 9000));

    const ooo = await page.evaluate(() => window.__ooo);
    rows.push({ ooo, panicked });
    console.log(`trial ${String(i).padStart(2)}: relocations=${String(ooo).padStart(2)} panicked=${panicked}`);
    await page.close();
  }
  await browser.close();

  const withT = rows.filter((r) => r.ooo > 0);
  const noT = rows.filter((r) => r.ooo === 0);
  const pct = (a) => (a.length ? Math.round((a.filter((r) => r.panicked).length / a.length) * 100) + '%' : 'n/a');
  console.log(`\nrelocations present (n=${withT.length}): panic ${pct(withT)}`);
  console.log(`relocations absent  (n=${noT.length}): panic ${pct(noT)}`);

  const panics = rows.filter((r) => r.panicked).length;
  console.log(`\ntotal: ${panics}/${rows.length} loads panicked`);
  process.exit(panics > 0 ? 1 : 0);
})();
