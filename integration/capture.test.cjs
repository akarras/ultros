// Unit tests for the serialized screenshot helper in ./capture.cjs.
//
// The bug it exists for: under puppeteer 25 a `Page.captureScreenshot` on a
// page that is not the foreground tab never returns, and puppeteer guards
// `screenshot()` behind a browser-wide mutex, so one wedged capture stalls
// every other page's screenshot and the whole run hangs. `capture()` takes the
// page to the front and screenshots it under one process-wide lock so nothing
// can foreground another tab in between.
//
// These cases use fake pages that record their calls — no browser involved.
//
// Run with: node --test integration/capture.test.cjs

// Read before requiring ./capture.cjs: the module snapshots this at load.
process.env.SCREENSHOT_TIMEOUT_MS = "200";

const test = require("node:test");
const assert = require("node:assert");
const { capture } = require("./capture.cjs");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * A stand-in for a puppeteer Page that appends to a shared call log.
 * `screenshotImpl` decides how (and whether) each screenshot settles.
 */
function fakePage(name, log, screenshotImpl) {
  const page = {
    async bringToFront() {
      log.push(`front:${name}`);
    },
    async screenshot(options) {
      log.push(`shot:${name}`);
      return screenshotImpl ? screenshotImpl(options) : `png:${name}`;
    },
  };
  return page;
}

test("brings the page to the front before screenshotting it", async () => {
  const log = [];
  const page = fakePage("a", log);
  const result = await capture(page, { path: "a.png" });
  assert.deepStrictEqual(log, ["front:a", "shot:a"]);
  assert.strictEqual(result, "png:a");
});

test("passes the options through to page.screenshot", async () => {
  let seen = null;
  const page = fakePage("a", [], (options) => {
    seen = options;
    return "png";
  });
  await capture(page, { path: "a.png", fullPage: true });
  assert.deepStrictEqual(seen, { path: "a.png", fullPage: true });
});

test("serializes concurrent captures so no page is foregrounded mid-capture", async () => {
  const log = [];
  // Each screenshot takes a moment, which is exactly the window in which an
  // unserialized second capture would call bringToFront and background this
  // page out from under the in-flight capture.
  const slow = () => sleep(20).then(() => "png");
  const pages = ["a", "b", "c"].map((n) => fakePage(n, log, slow));

  await Promise.all(pages.map((p, i) => capture(p, { path: `${i}.png` })));

  assert.strictEqual(log.length, 6);
  for (let i = 0; i < log.length; i += 2) {
    const [frontVerb, frontName] = log[i].split(":");
    const [shotVerb, shotName] = log[i + 1].split(":");
    assert.strictEqual(frontVerb, "front");
    assert.strictEqual(shotVerb, "shot");
    // The screenshot must belong to the page that was just brought forward.
    assert.strictEqual(shotName, frontName);
  }
});

test("a rejected capture does not wedge the queue", async () => {
  const log = [];
  const bad = fakePage("bad", log, () => Promise.reject(new Error("boom")));
  const good = fakePage("good", log);

  const first = capture(bad, {});
  const second = capture(good, {});

  await assert.rejects(first, /boom/);
  assert.strictEqual(await second, "png:good");
  assert.deepStrictEqual(log, ["front:bad", "shot:bad", "front:good", "shot:good"]);
});

test("time spent waiting in the queue does not count toward the timeout", async () => {
  // At CONCURRENCY=16 every worker calls capture() at once. A capture that
  // waits out several healthy ones ahead of it must still get its full budget
  // when its turn comes, or a busy queue would fail captures that are fine.
  const slow = () => sleep(80).then(() => "png");
  const pages = ["a", "b", "c"].map((n) => fakePage(n, [], slow));
  const results = await Promise.all(
    pages.map((p, i) => capture(p, { path: `${i}.png` })),
  );
  // 3 x 80ms of queued work exceeds the 200ms timeout in total; none of them
  // may reject on that basis.
  assert.deepStrictEqual(results, ["png", "png", "png"]);
});

test("a capture that never settles times out instead of hanging", async () => {
  const hung = fakePage("hung", [], () => new Promise(() => {}));
  const started = Date.now();
  await assert.rejects(capture(hung, {}), /timed out after 200ms/);
  // Guard against the timeout silently not firing and something else
  // resolving the assertion.
  assert.ok(Date.now() - started >= 150, "should have waited for the timeout");
});

test("a timed-out capture releases the lock for the next one", async () => {
  const log = [];
  const hung = fakePage("hung", log, () => new Promise(() => {}));
  const good = fakePage("good", log);

  const first = capture(hung, {});
  const second = capture(good, {});

  await assert.rejects(first, /timed out/);
  // The next capture must not have to wait out a second full timeout behind
  // the wedged one — it is released as soon as the first one times out.
  const started = Date.now();
  assert.strictEqual(await second, "png:good");
  assert.ok(
    Date.now() - started < 200,
    "second capture should run as soon as the first times out",
  );
  assert.deepStrictEqual(log, ["front:hung", "shot:hung", "front:good", "shot:good"]);
});
