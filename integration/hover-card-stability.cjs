// Regression probe: a hover card that slides across the screen after it opens.
//
// `HoverCard` (components/hover_card.rs) positions its overlay from the
// overlay's own measured size (`use_element_size`, a ResizeObserver) and the
// anchor's rect. The overlay is `position: fixed` with `width: auto`, so its
// shrink-to-fit width depends on how much room lies to the right of its
// `left` — and `left` is computed from that width. Every ResizeObserver cycle
// then produces a new width, which produces a new `left`, which produces a
// new width: the card visibly slides (and reflows) for dozens of frames until
// the loop converges on a fixed point.
//
// The recipe planner's "Share plan" clipboard is the cleanest trigger: its
// tooltip is `Copy '<full share URL>' to clipboard`, a long, wrappable text
// anchored at the right edge of the viewport. The clipboard buttons on the
// analyzer rows show the same thing with any text that has to wrap.
//
// Browser-only by construction: the loop lives between CSS shrink-to-fit
// layout and a ResizeObserver, neither of which the `ssr` unit tests can see.
//
// Asserts the outcome — once the overlay is visible, its box stops moving
// within a frame or two — rather than the mechanism.
const puppeteer = require('puppeteer');

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD = process.env.WORLD || 'Gilgamesh';
// A real bundled-data recipe with a long share URL, from the bug report.
const RECIPE = process.env.RECIPE || '37872';
const ROUTE =
  `/recipe/${RECIPE}?world=${WORLD}&lang=en&require-hq=false&shards-exclude=true` +
  `&buy-scope=region&craft=44033%3A5652&visits=1&quantity=1`;
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
// Frames to sample after the overlay first becomes visible. The pre-fix loop
// moves the card on nearly every frame for well over a second.
const SAMPLE_FRAMES = 90;
// The overlay is allowed one settle step after its first visible paint (the
// measured size arrives a ResizeObserver cycle after the first layout).
const MAX_MOVES = 1;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * Sample the tooltip's box once per animation frame and report every frame on
 * which it changed. Runs in the page.
 */
function sampleOverlay(frames) {
  return new Promise((resolve) => {
    const samples = [];
    let n = 0;
    const tick = () => {
      const t = document.querySelector('[role="tooltip"]');
      if (t) {
        const r = t.getBoundingClientRect();
        samples.push({
          f: n,
          hidden: getComputedStyle(t).visibility === 'hidden',
          x: Math.round(r.x),
          y: Math.round(r.y),
          w: Math.round(r.width),
          h: Math.round(r.height),
        });
      }
      if (++n < frames) requestAnimationFrame(tick);
      else resolve(samples);
    };
    requestAnimationFrame(tick);
  });
}

/** Frames on which the box changed, counted from the first visible sample. */
function movesAfterVisible(samples) {
  const visible = samples.filter((s) => !s.hidden);
  const moves = [];
  for (let i = 1; i < visible.length; i += 1) {
    const a = visible[i - 1];
    const b = visible[i];
    if (a.x !== b.x || a.y !== b.y || a.w !== b.w || a.h !== b.h) moves.push(b);
  }
  return { visible, moves };
}

async function probeButton(page, index, label, viewport) {
  await page.setViewport({ width: viewport.width, height: viewport.height, deviceScaleFactor: 1 });
  // Leave any previous card and let the layout settle at the new width.
  await page.mouse.move(1, 1);
  await sleep(200);

  const handle = (await page.$$('button.clipboard'))[index];
  if (!handle) throw new Error(`button.clipboard[${index}] (${label}) is not on the page`);
  await handle.evaluate((el) => el.scrollIntoView({ block: 'center' }));
  await sleep(100);

  // Hover the anchor and start sampling in the same frame the card opens.
  await handle.hover();
  const samples = await page.evaluate(sampleOverlay, SAMPLE_FRAMES);

  if (!samples.length) {
    throw new Error(`hovering ${label} at ${viewport.width}px opened no hover card`);
  }
  const { visible, moves } = movesAfterVisible(samples);
  if (!visible.length) {
    throw new Error(
      `${label} at ${viewport.width}px: the card stayed hidden for ${samples.length} frames`,
    );
  }
  const first = visible[0];
  const last = visible[visible.length - 1];
  console.log(
    `[info] ${label} @${viewport.width}px: visible from frame ${first.f}, ` +
      `${moves.length} move(s) over ${visible.length} frames, ` +
      `first ${first.x},${first.y} ${first.w}x${first.h} -> last ${last.x},${last.y} ${last.w}x${last.h}`,
  );
  await page.mouse.move(1, 1);
  await sleep(100);
  return { label, viewport: viewport.width, moves, first, last };
}

async function main() {
  const browser = await puppeteer.launch({
    headless: true,
    // Both scrollbar regimes hit the loop; the overlay-scrollbar one
    // (Windows 11 / macOS) is the one that never converges before the
    // card reaches its full width, so probe without classic scrollbars.
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--hide-scrollbars'],
  });
  const failures = [];

  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(TIMEOUT_MS);
    const { hostname } = new URL(BASE_URL);
    await page.setCookie({ name: 'HIDE_ADS', value: 'true', domain: hostname, path: '/' });
    await page.evaluateOnNewDocument(() => {
      window.__hydrated = false;
      window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; }, { once: true });
    });
    await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
    await page.goto(`${BASE_URL}${ROUTE}`, { waitUntil: 'domcontentloaded', timeout: TIMEOUT_MS });
    await page.waitForFunction(() => window.__hydrated === true);
    await page.waitForSelector('button.clipboard');

    const labels = await page.$$eval('button.clipboard', (els) =>
      els.map((el) => (el.getAttribute('aria-label') || '').slice(0, 40)),
    );
    console.log(`[info] ${ROUTE}: ${labels.length} clipboard button(s): ${JSON.stringify(labels)}`);

    // "Share plan" sits at the right edge (the clamped regime); the plan copy
    // button sits mid-page (the centered regime). Probe both at two widths.
    const cases = [];
    for (const width of [1280, 1600]) {
      for (let i = 0; i < labels.length; i += 1) {
        cases.push({ index: i, label: `clipboard[${i}] "${labels[i]}"`, viewport: { width, height: 800 } });
      }
    }
    for (const c of cases) {
      const result = await probeButton(page, c.index, c.label, c.viewport);
      if (result.moves.length > MAX_MOVES) {
        const path = result.moves
          .slice(0, 6)
          .map((m) => `f${m.f}:${m.x},${m.y} ${m.w}x${m.h}`)
          .join(' -> ');
        failures.push(
          `${result.label} @${result.viewport}px moved ${result.moves.length} times after ` +
            `becoming visible (${path}${result.moves.length > 6 ? ' -> ...' : ''})`,
        );
      }
    }
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error('[fail] hover cards slide after opening:');
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log('[done] hover cards settle in place');
}

main().catch((err) => {
  console.error(`[fail] ${err && err.stack ? err.stack : err}`);
  process.exit(1);
});
