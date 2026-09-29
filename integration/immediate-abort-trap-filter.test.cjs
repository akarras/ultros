// End-to-end tests for how production's `beforeSend` treats the bare
// `RuntimeError: unreachable` trap once the wasm is built with
// `panic = "immediate-abort"`.
//
// Under immediate-abort EVERY Rust panic reaches window.onerror as that bare
// trap, with no contexts.rust_panic and no message. Error-filter rule 3 used
// to drop that shape outright for the injecting populations (translation
// overlays, stale Chrome) because only the tachys hydration panic produced
// it; now it would swallow every panic those users hit. So a trap from those
// populations is symbolicated first and dropped only when its top frames are
// tachys hydration code.
//
// This loads the REAL error_filter.js and wasm_symbolicate.js into one fake
// window and runs the REAL beforeSend body, extracted from lib.rs's shell
// script (un-escaping its `{{ }}` format braces), with a scripted fetch
// serving the `.symbols` map. So the wiring is under test, not a copy of it.
//
// Run with: node --test integration/immediate-abort-trap-filter.test.cjs

const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const SRC_DIR = path.join(__dirname, "..", "ultros-frontend", "ultros-app", "src");
const FILTER_SRC = fs.readFileSync(path.join(SRC_DIR, "error_filter.js"), "utf8");
const SYMBOLICATE_SRC = fs.readFileSync(path.join(SRC_DIR, "wasm_symbolicate.js"), "utf8");

// The body of `config.beforeSend = function(event, hint) {{ ... }};` in
// lib.rs, with the Rust format-string brace escapes undone.
function extractBeforeSend() {
  const lib = fs.readFileSync(path.join(SRC_DIR, "lib.rs"), "utf8");
  const start = lib.indexOf("config.beforeSend = function(event, hint) {{");
  assert.ok(start !== -1, "lib.rs must assign config.beforeSend");
  const bodyStart = lib.indexOf("{{", start) + 2;
  // The function ends at the first `}};` at the wrapper's indentation.
  const end = lib.indexOf("\n    }};", bodyStart);
  assert.ok(end !== -1, "could not find the end of beforeSend in lib.rs");
  return lib.slice(bodyStart, end).replace(/\{\{/g, "{").replace(/\}\}/g, "}");
}
const BEFORE_SEND_BODY = extractBeforeSend();

const MODULE = "https://ultros.app/pkg/abc1234/ultros.wasm";
const MAP_URL = "https://ultros.app/pkg/abc1234/ultros.symbols";

// Function names exactly as the wasm-symbols map carries them (v0 crate
// disambiguators stripped). The tachys hydration names are the ones the
// debug name section shows for tachys 0.2 (`failed_to_cast_*`, `Cursor::*`).
const SYMBOLS = {
  10: "tachys::hydration::failed_to_cast_element",
  11: "<tachys::hydration::Cursor>::next_placeholder",
  12: "tachys::hydration::failed_to_cast_marker_node::{closure#0}",
  13: "<tachys::html::element::HtmlElement<E, At, Ch> as tachys::view::Render>::hydrate",
  14: "<tachys::view::keyed::Keyed<T, I, K, KF, VF, VFS, V> as tachys::view::Render>::hydrate",
  15: "tachys::renderer::dom::Dom::remove_node",
  20: "ultros_app::routes::recipe_analyzer::RecipeAnalyzer::{closure#3}",
  21: "core::option::unwrap_failed",
  22: "<ultros_app::routes::item_view::ItemView as tachys::view::Render>::hydrate",
  23: "leptos::hydration::hydrate_body",
  24: "reactive_graph::effect::render_effect::RenderEffect<T>::new",
  // The top three frames of GlitchTip #7964 (prod build 60d983f, Chrome 120),
  // verbatim: a <meta> in <head> failing to hydrate, where the trapping
  // function resolves to tachys's `hydrate_async` body rather than `hydrate`.
  30: "<tachys::html::element::HtmlElement<_, _, _> as tachys::view::RenderHtml>::hydrate_async::{closure#0}::inner_1",
  31: "<leptos_meta::RegisteredMetaTag<tachys::html::element::elements::Meta, alloc::vec::Vec<tachys::html::attribute::any_attribute::AnyAttribute>, ()> as tachys::view::RenderHtml>::hydrate::<true>",
  32: "<_ as tachys::view::any_view::IntoAny>::into_any::hydrate_from_server::<leptos_meta::RegisteredMetaTag<tachys::html::element::elements::Meta, alloc::vec::Vec<tachys::html::attribute::any_attribute::AnyAttribute>, ()>>",
  33: "ultros_client::hydrate::{closure#0}",
};
const SYMBOLS_TEXT = Object.entries(SYMBOLS)
  .map(([i, n]) => `${i}:${n}`)
  .join("\n");

const STALE_CHROME =
  "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.5615.136 Mobile Safari/537.36";
// Far ahead of the clock-relative stale ceiling, so it never rots into it.
const CURRENT_CHROME =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/999.0.0.0 Safari/537.36";

function doc({ fontCount = 0, htmlClass = "" } = {}) {
  const classes = htmlClass ? htmlClass.split(/\s+/) : [];
  return {
    getElementsByTagName: (tag) => ({ length: tag === "font" ? fontCount : 0 }),
    documentElement: {
      className: htmlClass,
      classList: { contains: (c) => classes.includes(c) },
    },
  };
}

// A fake browser window carrying both scripts and the beforeSend under test.
// `mapStatus` null = the map fetch fails at the network layer.
function load({ ua, document, mapStatus = 200, withSymbolicator = true }) {
  const fetches = [];
  const win = {
    document,
    AbortController: class {
      constructor() {
        this.signal = {};
      }
      abort() {}
    },
    setTimeout: () => 0,
    clearTimeout: () => {},
    fetch: (url) => {
      fetches.push(url);
      if (mapStatus === null) return Promise.reject(new Error("offline"));
      return Promise.resolve({
        ok: mapStatus === 200,
        text: () => Promise.resolve(url === MAP_URL ? SYMBOLS_TEXT : ""),
      });
    },
  };
  // eslint-disable-next-line no-new-func
  new Function("window", "navigator", FILTER_SRC)(win, { userAgent: ua });
  if (withSymbolicator) {
    // eslint-disable-next-line no-new-func
    new Function("window", SYMBOLICATE_SRC)(win);
  }
  // eslint-disable-next-line no-new-func
  const beforeSend = new Function(
    "window",
    "existingBeforeSend",
    `return function(event, hint) {${BEFORE_SEND_BODY}};`,
  )(win, undefined);
  return { beforeSend, fetches };
}

// A prod immediate-abort trap: bare V8 value, no rust_panic context, frames
// oldest-first with the trapping function LAST (top of stack), unresolved.
function trap(topDownIndices) {
  const frames = topDownIndices
    .slice()
    .reverse()
    .map((i) => ({
      filename: `/pkg/abc1234/ultros.wasm:wasm-function[${i}]:0x1f00`,
      abs_path: `${MODULE}:wasm-function[${i}]:0x1f00`,
      function: "?",
    }));
  frames.unshift({
    filename: "/pkg/abc1234/ultros.js",
    abs_path: "https://ultros.app/pkg/abc1234/ultros.js",
    function: "__wbg_adapter_50",
  });
  return {
    exception: {
      values: [{ type: "RuntimeError", value: "unreachable", stacktrace: { frames } }],
    },
    breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    tags: {},
  };
}

async function outcome(env, event) {
  const result = await Promise.resolve(env.beforeSend(event, {}));
  return result === null ? "dropped" : "sent";
}

const POPULATIONS = [
  ["stale Chrome 112 WebView", { ua: STALE_CHROME, document: doc() }],
  ["current Chrome + <font> overlay", { ua: CURRENT_CHROME, document: doc({ fontCount: 3 }) }],
  ["current Chrome + translated-ltr", { ua: CURRENT_CHROME, document: doc({ htmlClass: "translated-ltr" }) }],
];

// ── The bug #1585 had: a non-hydration panic from these users was lost ──

for (const [label, env] of POPULATIONS) {
  test(`${label}: an app panic (unwrap in the recipe analyzer) is SENT`, async () => {
    assert.strictEqual(await outcome(load(env), trap([21, 20, 23])), "sent");
  });
  test(`${label}: an app panic during hydration (app frame on top of tachys hydrate) is SENT`, async () => {
    assert.strictEqual(await outcome(load(env), trap([22, 13, 23])), "sent");
  });
  test(`${label}: the tachys hydration panic is still DROPPED`, async () => {
    assert.strictEqual(await outcome(load(env), trap([10, 13, 23])), "dropped");
  });
}

// ── Which frames count as the hydration panic ──

test("Cursor::next_placeholder on top is the hydration panic", async () => {
  const env = load({ ua: STALE_CHROME, document: doc() });
  assert.strictEqual(await outcome(env, trap([11, 14, 23])), "dropped");
});

test("a failed_to_cast closure on top is the hydration panic", async () => {
  const env = load({ ua: STALE_CHROME, document: doc() });
  assert.strictEqual(await outcome(env, trap([12, 13, 23])), "dropped");
});

test("a tachys Cursor fn inlined into a tachys `hydrate` impl on top is the hydration panic", async () => {
  const env = load({ ua: STALE_CHROME, document: doc() });
  assert.strictEqual(await outcome(env, trap([13, 14, 23])), "dropped");
});

// GlitchTip #7964: 95 events in the first 90 minutes after #1585 shipped, all
// the stale-Chrome population, all a head <meta> failing to hydrate.
test("#7964: a tachys `hydrate_async` body on top (head <meta> mismatch) is the hydration panic", async () => {
  const env = load({
    ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    document: doc(),
  });
  assert.strictEqual(await outcome(env, trap([30, 31, 32, 33])), "dropped");
});

// #7964 again on 2026-09-25, same head <meta> frames, from a Tencent Cloud
// crawler fleet pinned at Chrome 131: over the old fixed <=124 ceiling, so it
// leaked. The ceiling now tracks the clock, and 131 is well over a year stale.
test("#7964: the same trap from a Chrome 131 crawler fleet is the stale population", async () => {
  const env = load({
    ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    document: doc(),
  });
  assert.strictEqual(await outcome(env, trap([30, 31, 32, 33])), "dropped");
});

test("#7964's frames from a clean current browser are still SENT", async () => {
  const env = load({ ua: CURRENT_CHROME, document: doc() });
  assert.strictEqual(await outcome(env, trap([30, 31, 32, 33])), "sent");
});

test("tachys code on top but no hydration in the top three frames is SENT", async () => {
  const env = load({ ua: STALE_CHROME, document: doc() });
  assert.strictEqual(await outcome(env, trap([15, 24, 20, 13])), "sent");
});

test("a hydration frame deeper than the top three does not drop an app panic", async () => {
  const env = load({ ua: STALE_CHROME, document: doc() });
  assert.strictEqual(await outcome(env, trap([20, 24, 21, 10])), "sent");
});

// ── The flood gate is unchanged where it matters ──

test("a clean current browser sends every trap, hydration included (never a candidate)", async () => {
  const env = load({ ua: CURRENT_CHROME, document: doc() });
  assert.strictEqual(await outcome(env, trap([10, 13, 23])), "sent");
  assert.strictEqual(await outcome(env, trap([21, 20])), "sent");
});

test("a missing map keeps the old suppression for the flagged population", async () => {
  const env = load({ ua: STALE_CHROME, document: doc(), mapStatus: 404 });
  assert.strictEqual(await outcome(env, trap([21, 20])), "dropped");
});

test("a map fetch that fails outright also keeps the old suppression", async () => {
  const env = load({ ua: STALE_CHROME, document: doc(), mapStatus: null });
  assert.strictEqual(await outcome(env, trap([21, 20])), "dropped");
});

test("with no symbolicator loaded, a candidate trap is dropped as before", async () => {
  const env = load({ ua: STALE_CHROME, document: doc(), withSymbolicator: false });
  assert.strictEqual(await outcome(env, trap([21, 20])), "dropped");
});

// ── What the kept event looks like, and what it costs ──

test("a sent app trap arrives symbolicated, fingerprinted and titled", async () => {
  const env = load({ ua: STALE_CHROME, document: doc() });
  const ev = await Promise.resolve(env.beforeSend(trap([21, 20, 23]), {}));
  assert.notStrictEqual(ev, null);
  const ex = ev.exception.values[0];
  assert.match(ex.value, /^unreachable in ultros_app::routes::recipe_analyzer::RecipeAnalyzer/);
  assert.deepStrictEqual(ev.fingerprint.slice(0, 2), [
    "rust-wasm-trap",
    "core::option::unwrap_failed",
  ]);
});

test("the map is fetched once per session, not once per trap", async () => {
  const env = load({ ua: STALE_CHROME, document: doc() });
  await outcome(env, trap([10, 13]));
  await outcome(env, trap([21, 20]));
  await outcome(env, trap([11, 14]));
  assert.deepStrictEqual(env.fetches, [MAP_URL]);
});

test("a non-trap event that the sync filter drops never fetches the map", async () => {
  const env = load({ ua: STALE_CHROME, document: doc() });
  const empty = {
    exception: {
      values: [{ type: "UnhandledRejection", value: "Non-Error promise rejection captured with value: undefined" }],
    },
  };
  assert.strictEqual(await outcome(env, empty), "dropped");
  assert.deepStrictEqual(env.fetches, []);
});
