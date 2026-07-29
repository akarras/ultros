// Regression test for GlitchTip issue #5767:
//   "NotAllowedError: Clipboard write was blocked due to lack of user activation."
//
// Root cause: the `<Clipboard>` component (and `copy_invite_url`) called
//   `let _ = clipboard.write_text(&text);`
// which DROPS the Promise returned by `navigator.clipboard.writeText()`.
// That Promise rejects whenever the browser blocks the write (Firefox revokes
// transient user activation when an ad iframe steals focus, the document is not
// focused, permissions, etc.). A dropped rejecting Promise becomes an
// *unhandled rejection*, which Sentry/GlitchTip captures via its
// `onunhandledrejection` global handler (mechanism
// `auto.browser.global_handlers.onunhandledrejection`, handled: false) and
// reports as an error.
//
// The fix awaits the Promise on a spawned task and swallows the error, so a
// blocked best-effort copy no longer surfaces as an unhandled rejection.
//
// This test models the two code patterns at the Promise level (the semantics
// are identical in the browser and in Node) and asserts the dropped pattern
// produces an unhandled rejection while the awaited/handled pattern does not.
//
// Run with: node --test integration/clipboard-rejection.test.cjs

const test = require("node:test");
const assert = require("node:assert");

// A stub of `navigator.clipboard.writeText()` that rejects asynchronously,
// exactly like a real browser blocking the write (the rejection settles after
// an async round-trip, NOT synchronously).
function writeTextThatRejects() {
  return new Promise((_resolve, reject) => {
    setTimeout(
      () =>
        reject(
          new Error(
            "Clipboard write was blocked due to lack of user activation.",
          ),
        ),
      0,
    );
  });
}

// Capture unhandled rejections that occur while running `fn`. Returns the list
// of rejection reasons seen after the micro/macrotask queues have drained.
//
// During the measurement window we detach any other `unhandledRejection`
// listeners (notably the `node --test` runner's own handler, which would
// otherwise fail the BUG test on the *intentional* leak) so only our collector
// observes the rejection, then restore them.
async function collectUnhandledRejections(fn) {
  const seen = [];
  const onUnhandled = (reason) => seen.push(reason);
  const prev = process.listeners("unhandledRejection");
  process.removeAllListeners("unhandledRejection");
  process.on("unhandledRejection", onUnhandled);
  try {
    fn();
    // Let the rejection settle and Node's unhandled-rejection detection run.
    await new Promise((r) => setTimeout(r, 10));
  } finally {
    process.off("unhandledRejection", onUnhandled);
    for (const l of prev) process.on("unhandledRejection", l);
  }
  return seen;
}

test("BUG (pre-fix pattern): dropping the writeText Promise leaks an unhandled rejection", async () => {
  // Mirrors `let _ = clipboard.write_text(&text);` — the Promise is discarded.
  const seen = await collectUnhandledRejections(() => {
    const _promise = writeTextThatRejects();
    void _promise; // dropped, no handler attached
  });
  assert.strictEqual(
    seen.length,
    1,
    "the discarded rejecting clipboard Promise should surface as an unhandled rejection (what GlitchTip reports)",
  );
  assert.match(seen[0].message, /lack of user activation/);
});

test("FIX (spawn_local + JsFuture::from(promise).await): the rejection is consumed", async () => {
  // Mirrors the fix:
  //   let promise = clipboard.write_text(&text);
  //   spawn_local(async move {
  //       if JsFuture::from(promise).await.is_err() { /* best-effort, log */ }
  //   });
  // The handler is attached on a spawned task (a later microtask), but the
  // clipboard rejection settles truly-async, so the handler is in place first.
  const seen = await collectUnhandledRejections(() => {
    const promise = writeTextThatRejects();
    // The spawned task awaits the promise and swallows the error.
    (async () => {
      try {
        await promise;
      } catch (_e) {
        // best-effort copy; nothing to recover. (Rust logs a dev-console warn.)
      }
    })();
  });
  assert.strictEqual(
    seen.length,
    0,
    "awaiting and swallowing the rejection must not produce an unhandled rejection",
  );
});
