"use strict";

/**
 * Serialized screenshot capture.
 *
 * Puppeteer 25's `Page.captureScreenshot` never returns for a page that is not
 * the browser's foreground tab — it just hangs, with no timeout of its own.
 * Puppeteer also guards `screenshot()` behind a *browser-wide* mutex, so the
 * one wedged capture blocks every other page's screenshot too and the whole
 * run stalls until the harness is killed. Puppeteer 22 did not have this
 * behaviour, so it only showed up when the dependency was bumped: the runner's
 * workers each hold their own page, and whichever page happens not to be in
 * front when it reaches its screenshot takes the suite down with it.
 *
 * `capture()` fixes that by taking the page to the front and screenshotting it
 * under a single process-wide lock, so nothing can foreground another tab in
 * between. `bringToFront()` on its own is not enough — two workers racing it
 * still end up capturing a backgrounded page.
 *
 * Any script that can have more than one page open at a time must go through
 * here rather than calling `page.screenshot()` directly. Single-page scripts
 * are unaffected either way.
 */

// Screenshots are serialized by puppeteer regardless, so the lock costs no
// parallelism the harness would otherwise have had.
let chain = Promise.resolve();

// A capture that hangs anyway must not wedge the run: the callers all treat a
// screenshot as a diagnostic, so time it out and let them log and move on.
const DEFAULT_TIMEOUT_MS = Number(process.env.SCREENSHOT_TIMEOUT_MS || 30000);

function withTimeout(promise, ms, label) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`${label} timed out after ${ms}ms`)),
      ms,
    );
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

/**
 * Screenshot `page` with `options`, serialized against every other capture in
 * this process. Rejects like `page.screenshot()` does, plus on timeout.
 */
async function capture(page, options) {
  // The timeout is armed here, inside the queued work, rather than around the
  // queue wait: at CONCURRENCY=16 every worker calls capture() at roughly the
  // same moment, and a timer started at call time would fire on captures that
  // were merely waiting their turn behind healthy ones.
  const run = () =>
    withTimeout(
      (async () => {
        await page.bringToFront();
        return await page.screenshot(options);
      })(),
      DEFAULT_TIMEOUT_MS,
      "screenshot",
    );
  // Queue behind whatever capture is in flight, whether it settled or threw.
  const queued = chain.then(run, run);
  // Release the lock once this one settles — including on timeout, so a
  // capture that hung does not make every later one wait out its own timeout.
  chain = queued.then(
    () => {},
    () => {},
  );
  return queued;
}

module.exports = { capture };
