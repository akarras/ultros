// Diagnostic for GlitchTip #6831 — "RustWasmPanic: internal error: entered
// unreachable code" at tachys `hydration.rs:163` (`failed_to_cast_element`),
// third recurrence (Cause C).
//
// HYPOTHESIS
// SSR always renders xiv-gen game data in ENGLISH; the client swaps
// `xiv_gen_db` to the visitor's `i18n_pref_locale` *before* `hydrate()` runs.
// `RelatedItems` picks which items to render by matching the page item's
// `.name` prefix/suffix against every other item's `.name`, so the two sides
// select a *different set* of items — a structural DOM mismatch, not just a
// text one. Japanese/Chinese/Korean item names carry no ASCII space at all, so
// `split_once(' ')` yields `None` on the client and the whole prefix/suffix
// contribution vanishes.
//
// WHAT IT MEASURES
// Same URL, same everything, two arms: `i18n_pref_locale=en` vs a non-English
// locale. Counts `hydration.rs:163` panics per arm, plus the number of related
// item links present in the SSR HTML vs after hydration.
//
// USE
//   node ./locale-hydration.cjs
//   ITEM_PATH=/item/Sargatanas/23539 LOCALE=fr N=6 node ./locale-hydration.cjs
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'https://ultros.app';
const ITEM_PATH = process.env.ITEM_PATH || '/item/Sargatanas/23539';
const LOCALE = process.env.LOCALE || 'ja';
const N = Number(process.env.N || 5);

async function trial(browser, hostname, locale, i) {
  const page = await browser.newPage();
  await page.setUserAgent(
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36'
  );
  await page.setCookie(
    { name: 'HIDE_ADS', value: 'true', domain: hostname, path: '/' },
    { name: 'i18n_pref_locale', value: locale, domain: hostname, path: '/' }
  );
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
    if (/hydration\.rs:163|entered unreachable code/.test(m.text())) panicked = true;
  });
  page.on('pageerror', (e) => {
    if (/unreachable/i.test(String(e))) panicked = true;
  });

  const url = `${BASE_URL}${ITEM_PATH}?cb=${Date.now()}-${i}`;
  let ssrLinks = null;
  try {
    const res = await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 });
    const html = await res.text();
    ssrLinks = (html.match(/href="\/item\//g) || []).length;
  } catch (e) {
    // A blocking <script> in <head> can hold DCL open on prod; not a signal.
  }
  await new Promise((r) => setTimeout(r, 9000));
  const domLinks = await page
    .evaluate(() => document.querySelectorAll('a[href^="/item/"]').length)
    .catch(() => null);
  await page.close();
  return { panicked, ssrLinks, domLinks };
}

(async () => {
  const browser = await puppeteer.launch({
    headless: 'new',
    args: ['--no-sandbox', '--disable-dev-shm-usage'],
  });
  const { hostname } = new URL(BASE_URL);
  const results = {};
  for (const locale of ['en', LOCALE]) {
    results[locale] = [];
    for (let i = 0; i < N; i++) {
      const r = await trial(browser, hostname, locale, i);
      results[locale].push(r);
      console.log(
        `- ${locale} trial ${i}: panicked=${r.panicked} ssrItemLinks=${r.ssrLinks} domItemLinks=${r.domLinks}`
      );
    }
  }
  await browser.close();

  console.log('');
  for (const [locale, rows] of Object.entries(results)) {
    const p = rows.filter((r) => r.panicked).length;
    console.log(`- locale=${locale}: panic ${p}/${rows.length}`);
  }
  const enP = results.en.filter((r) => r.panicked).length;
  const altP = results[LOCALE].filter((r) => r.panicked).length;
  console.log(`\nverdict: en=${enP}/${N} ${LOCALE}=${altP}/${N}`);
  process.exit(altP > enP ? 1 : 0);
})();
