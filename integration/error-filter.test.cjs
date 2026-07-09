// Unit tests for the Sentry `beforeSend` noise filter defined in
// ../ultros-frontend/ultros-app/src/error_filter.js.
//
// The filter source references `window` and `navigator` as free
// identifiers. We load it by wrapping the source in
// `new Function('window', 'navigator', src)` and invoking it with a fresh
// fake `window` and a `navigator` carrying the User-Agent for the case.
// That mirrors how the file runs in the browser (top-level globals) while
// letting each case pick its own UA.
//
// Run with: node integration/error-filter.test.cjs
//   (or: npm --prefix integration run test:error-filter)

const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const FILTER_PATH = path.join(
  __dirname,
  "..",
  "ultros-frontend",
  "ultros-app",
  "src",
  "error_filter.js",
);
const SRC = fs.readFileSync(FILTER_PATH, "utf8");

// Load the filter with a given UA (and optional fake document) and return
// its `shouldDrop` predicate. The font-injection fingerprint reads
// `window.document.getElementsByTagName("font")`, so cases that exercise it
// pass a fake document; omitting it mirrors a browser with no <font> nodes.
function loadFilter(userAgent, documentObj) {
  const win = {};
  if (documentObj !== undefined) win.document = documentObj;
  // eslint-disable-next-line no-new-func
  const factory = new Function("window", "navigator", SRC);
  factory(win, { userAgent: userAgent || "" });
  assert.strictEqual(
    typeof win.__ultrosShouldDropEvent,
    "function",
    "error_filter.js must define window.__ultrosShouldDropEvent",
  );
  return win.__ultrosShouldDropEvent;
}

// A minimal document stand-in whose getElementsByTagName("font") reports
// `fontCount` injected <font> nodes (any other tag reports zero).
function fakeDocument(fontCount) {
  return {
    getElementsByTagName(tag) {
      return { length: tag === "font" ? fontCount : 0 };
    },
  };
}

// A richer document stand-in exposing both the injected <font> count and the
// <html> class list, so the translation-class fingerprint (html.translated-ltr
// / html.translated-rtl) can be exercised. `htmlClass` is the space-separated
// className on <html>.
function fakeDocumentEx(opts) {
  const o = opts || {};
  const fontCount = o.fontCount || 0;
  const htmlClass = o.htmlClass || "";
  const classes = htmlClass ? htmlClass.split(/\s+/) : [];
  return {
    getElementsByTagName(tag) {
      return { length: tag === "font" ? fontCount : 0 };
    },
    documentElement: {
      className: htmlClass,
      classList: {
        contains(c) {
          return classes.indexOf(c) !== -1;
        },
      },
    },
  };
}

// A stale, version-pinned Chrome UA (e.g. 108/111/112/120) — the stuck in-app
// WebView / version-pinned crawler population behind the flood.
function staleChromeUA(major) {
  return (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 " +
    "(KHTML, like Gecko) Chrome/" +
    major +
    ".0.0.0 Safari/537.36"
  );
}

// Real Chrome/112 WebView UA from GlitchTip issue #707's request payload.
const FROZEN_CHROME_112 =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 " +
  "(KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36";
// A current, non-frozen browser.
const CURRENT_CHROME =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 " +
  "(KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";

const HYDRATION_LOC =
  "/usr/local/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/" +
  "tachys-0.2.15/src/hydration.rs:227:9";

// Where the secondary `RefCell already borrowed` cascade panics — the
// wasm-bindgen-futures executor, NOT a tachys path. Such events are only
// recognized as the hydration flood via the tachys hydration breadcrumb.
const JS_SYS_SINGLETHREAD_LOC =
  "/usr/local/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/" +
  "js-sys-0.3.99/src/futures/task/singlethread.rs:142:37";

// The console breadcrumb every shape of the flood carries: the original
// hydration panic that kicked off the cascade.
const TACHYS_PANIC_BREADCRUMB = {
  category: "console",
  message: "panicked at " + HYDRATION_LOC + ":\ninternal error: entered unreachable code",
};

function rustPanic(location, extra) {
  return Object.assign(
    {
      contexts: { rust_panic: { location } },
      exception: {
        values: [
          {
            type: "RustWasmPanic",
            value: "internal error: entered unreachable code",
          },
        ],
      },
    },
    extra,
  );
}

const cases = [
  // ── Category 3: frozen-Chrome-112 translate-overlay hydration panic ──
  // This is the #707 / #2775 / #4951 / #4905 cluster. The `browser` tag
  // is derived server-side by GlitchTip from the UA, so it is ABSENT in
  // the client-side beforeSend event — the only client-visible signal is
  // the live navigator UA. No Chinese stability breadcrumb on this event.
  {
    name: "frozen Chrome 112 tachys hydration panic (no breadcrumb, no browser tag) is dropped",
    ua: FROZEN_CHROME_112,
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    }),
    expectDrop: true,
  },
  {
    name: "frozen Chrome 112 hydration panic via Chinese stability breadcrumb is dropped",
    ua: FROZEN_CHROME_112,
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: {
        values: [{ category: "console", message: "检测页面稳定 ok" }],
      },
    }),
    expectDrop: true,
  },
  {
    name: "hydration panic on a CURRENT browser with NO injected <font> is preserved (real bugs still reach GlitchTip)",
    ua: CURRENT_CHROME,
    document: fakeDocument(0),
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    }),
    expectDrop: false,
  },
  // The modern-Chrome CN-translation population that ignores the notranslate
  // trifecta: a current browser, but the live DOM has translation-injected
  // <font> wrappers. This is the #3005/#4911/#6406 flood PR #760 still missed.
  {
    name: "tachys hydration panic on a current browser WITH injected <font> (translation overlay) is dropped",
    ua: CURRENT_CHROME,
    document: fakeDocument(7),
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    }),
    expectDrop: true,
  },
  // The secondary cascade: `RefCell already borrowed` panics in the js-sys
  // executor (not a tachys path), so it is recognized only via the tachys
  // hydration breadcrumb. With injected <font> present it is the flood.
  {
    name: "RefCell-already-borrowed cascade (js-sys location) with tachys breadcrumb + injected <font> is dropped",
    ua: CURRENT_CHROME,
    document: fakeDocument(4),
    event: {
      contexts: { rust_panic: { location: JS_SYS_SINGLETHREAD_LOC } },
      exception: {
        values: [{ type: "RustWasmPanic", value: "RefCell already borrowed" }],
      },
      breadcrumbs: {
        values: [
          { category: "console", message: "app run!" },
          TACHYS_PANIC_BREADCRUMB,
          {
            category: "console",
            message: "panicked at " + JS_SYS_SINGLETHREAD_LOC + ":\nRefCell already borrowed",
          },
        ],
      },
    },
    expectDrop: true,
  },
  // Same cascade shape on a CLEAN (untranslated) page. This is NOT an
  // independent bug: the `RefCell already borrowed` in the js-sys executor is
  // the secondary cascade of the PRIMARY hydration panic (shown here by
  // TACHYS_PANIC_BREADCRUMB), which is reported as its own event carrying the
  // actionable tachys rust_panic.location and is preserved on clean browsers
  // (see the "hydration panic on a CURRENT browser ... is preserved" case
  // above). The executor twin points only at singlethread.rs — identical for
  // every panic — so it adds nothing the retained primary doesn't already show.
  // Dropped unconditionally as a redundant twin (Category 7), exactly like the
  // RuntimeError onerror twin in Category 6 (PR #921).
  {
    name: "RefCell-already-borrowed executor cascade on a clean page is dropped (redundant twin of the retained primary)",
    ua: CURRENT_CHROME,
    document: fakeDocument(0),
    event: {
      contexts: { rust_panic: { location: JS_SYS_SINGLETHREAD_LOC } },
      exception: {
        values: [{ type: "RustWasmPanic", value: "RefCell already borrowed" }],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }, TACHYS_PANIC_BREADCRUMB] },
    },
    expectDrop: true,
  },
  // The unhandled wasm trap that reaches window.onerror: type RuntimeError,
  // no rust_panic context at all — recognized via the tachys breadcrumb.
  {
    name: "unhandled RuntimeError unreachable (window.onerror, no rust_panic) with tachys breadcrumb + injected <font> is dropped",
    ua: CURRENT_CHROME,
    document: fakeDocument(1),
    event: {
      exception: { values: [{ type: "RuntimeError", value: "unreachable" }] },
      breadcrumbs: { values: [TACHYS_PANIC_BREADCRUMB] },
    },
    expectDrop: true,
  },
  // A non-hydration RuntimeError with injected <font> present must NOT be
  // swept up: the font fingerprint only suppresses tachys hydration panics.
  {
    name: "a non-hydration RuntimeError is preserved even when <font> is present",
    ua: CURRENT_CHROME,
    document: fakeDocument(5),
    event: {
      exception: { values: [{ type: "RuntimeError", value: "table index is out of bounds" }] },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: false,
  },

  // ── Category 3b: the stale-Chrome / crawler flood with NO visible <font> ──
  // The dominant backlog (GlitchTip #4 ≈1059, #6456 ≈40, plus the #5918/#5919
  // BLU, #4936 MCH, #224/#5392 VPR clusters and the per-URL /item/<world>/<id>
  // #65xx flood). A clean modern browser hydrates these exact URLs fine, the
  // SSR already ships the notranslate trifecta, and the population is uniformly
  // a stale, version-pinned Chrome (108/111/112/120) on data-center IPs whose
  // pre-hydration DOM mutation leaves no <font> the filter can see. PR #764's
  // single-version `Chrome/112.` check missed 108/111/120, so they leaked.
  {
    name: "stale Chrome 111 tachys hydration panic (no font, no translate class) is dropped",
    ua: staleChromeUA(111),
    document: fakeDocumentEx({ fontCount: 0 }),
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    }),
    expectDrop: true,
  },
  {
    name: "stale Chrome 108 unhandled RuntimeError unreachable (window.onerror) via tachys breadcrumb is dropped",
    ua: staleChromeUA(108),
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: { values: [{ type: "RuntimeError", value: "unreachable" }] },
      breadcrumbs: { values: [TACHYS_PANIC_BREADCRUMB] },
    },
    expectDrop: true,
  },
  {
    name: "stale Chrome 120 RefCell cascade (js-sys loc) via tachys breadcrumb, no font, is dropped",
    ua: staleChromeUA(120),
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      contexts: { rust_panic: { location: JS_SYS_SINGLETHREAD_LOC } },
      exception: {
        values: [{ type: "RustWasmPanic", value: "RefCell already borrowed" }],
      },
      breadcrumbs: {
        values: [
          { category: "console", message: "app run!" },
          TACHYS_PANIC_BREADCRUMB,
          {
            category: "console",
            message:
              "panicked at " + JS_SYS_SINGLETHREAD_LOC + ":\nRefCell already borrowed",
          },
        ],
      },
    },
    expectDrop: true,
  },
  // Guard: a current, self-updating browser must NOT be swept up by the stale
  // check just because it hit a clean-page hydration mismatch — those are the
  // genuine bugs the filter exists to preserve.
  {
    name: "stale check does not drop a CURRENT-Chrome clean-page hydration panic (no font, no translate class)",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    }),
    expectDrop: false,
  },
  // Guard: the stale-Chrome drop is gated behind hydration-panic recognition,
  // so a stale-Chrome NON-hydration error still reports.
  {
    name: "stale Chrome non-hydration RuntimeError is preserved",
    ua: staleChromeUA(108),
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: {
        values: [{ type: "RuntimeError", value: "table index is out of bounds" }],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: false,
  },

  // ── Category 3c: full-page translate class on <html>, injector-agnostic ──
  // Google / Chrome built-in full-page translation adds class="translated-ltr"
  // (or "translated-rtl") to <html> regardless of Chrome version or the wrapper
  // element it uses, so it catches the current-browser translation population
  // even when no <font> is visible at beforeSend.
  {
    name: "current Chrome hydration panic with html.translated-ltr (no font) is dropped",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0, htmlClass: "notranslate translated-ltr" }),
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    }),
    expectDrop: true,
  },
  {
    name: "current Chrome hydration panic with html.translated-rtl (no font) is dropped",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0, htmlClass: "translated-rtl" }),
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    }),
    expectDrop: true,
  },
  // Guard: the SSR-emitted "notranslate" class contains the substring
  // "translate" but must NOT be mistaken for an active translation.
  {
    name: "current Chrome hydration panic with only the SSR notranslate class is preserved",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0, htmlClass: "notranslate" }),
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    }),
    expectDrop: false,
  },

  // ── Category 3d: breadcrumb-INDEPENDENT recognition of the cascade ──
  // The cascade shapes (`RefCell already borrowed` in the js-sys executor, and
  // the unhandled `RuntimeError: unreachable` at window.onerror) were only ever
  // recognized via the tachys hydration *breadcrumb*. But at real client-side
  // beforeSend that breadcrumb is NOT in the array the SDK hands the filter —
  // only the explicitly-set `contexts.rust_panic` survives. Proof from prod
  // (release e59476b): on a single stale-Chrome (<=124) page-load the root
  // `internal error` panic — recognized via its tachys rust_panic location —
  // was dropped, yet the SAME load's `RefCell already borrowed` cascade leaked
  // (GlitchTip #6661 Chrome 106, #4908 Chrome 104, with no paired internal-error
  // issue). Same load => same UA => same fingerprint, so the only difference is
  // recognition: the breadcrumb prong does not fire at beforeSend. These cases
  // model that reality (no tachys breadcrumb present) and require recognition
  // from event-level signals that ARE reliably present: the js-sys executor
  // rust_panic location, and the exact RuntimeError "unreachable" value.
  {
    name: "stale Chrome RefCell cascade (js-sys loc, NO tachys breadcrumb) is dropped",
    ua: staleChromeUA(106),
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      contexts: { rust_panic: { location: JS_SYS_SINGLETHREAD_LOC } },
      exception: {
        values: [{ type: "RustWasmPanic", value: "RefCell already borrowed" }],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: true,
  },
  {
    name: "stale Chrome onerror RuntimeError unreachable (NO tachys breadcrumb) is dropped",
    ua: staleChromeUA(106),
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: { values: [{ type: "RuntimeError", value: "unreachable" }] },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: true,
  },
  {
    name: "RefCell cascade (js-sys loc) with injected <font>, NO tachys breadcrumb, is dropped",
    ua: CURRENT_CHROME,
    document: fakeDocument(3),
    event: {
      contexts: { rust_panic: { location: JS_SYS_SINGLETHREAD_LOC } },
      exception: {
        values: [{ type: "RustWasmPanic", value: "RefCell already borrowed" }],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: true,
  },
  // The RefCell executor cascade on a clean, current browser (no font, no
  // translate class, no stale UA, no tachys breadcrumb) — the real #6758 shape
  // (Chrome 131, no client-side translation fingerprint at all). It is dropped
  // unconditionally as a redundant twin (Category 7): its rust_panic.location
  // is the js-sys executor, so it is provably the secondary cascade, never a
  // primary fault. The genuine hydration bug it cascades from is still reported
  // via the PRIMARY `internal error` panic at the tachys location, which is
  // preserved on a clean browser (see the cases above) — so nothing actionable
  // is lost. This is the same reasoning as the Category 6 RuntimeError twin
  // (PR #921), which drops its onerror copy even on a clean browser.
  {
    name: "RefCell executor cascade (js-sys loc) on a current clean browser is dropped (Category 7 redundant twin)",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      contexts: { rust_panic: { location: JS_SYS_SINGLETHREAD_LOC } },
      exception: {
        values: [{ type: "RustWasmPanic", value: "RefCell already borrowed" }],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: true,
  },
  {
    name: "onerror RuntimeError unreachable on a current clean browser with no fingerprint is preserved",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: { values: [{ type: "RuntimeError", value: "unreachable" }] },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: false,
  },

  // ── Category 6: redundant onerror wasm `unreachable` trap (dedup) ──
  // The onerror "RuntimeError: unreachable" the browser captures after Rust's
  // abort() runs the wasm `unreachable` instruction. When its stack carries one
  // of our pkg-bundle frames it is provably OUR wasm trap — the guaranteed
  // duplicate of the actionable RustWasmPanic the panic hook already reported —
  // so it is dropped UNCONDITIONALLY (no font / translate / stale-Chrome
  // fingerprint), which the gated category-3 prong above could not do. This is
  // the #6781–#6828 per-deploy fragmenting fleet. The frameless variant above
  // is left preserved: only an attributable-to-our-bundle trap is a known dup.
  {
    name: "onerror RuntimeError unreachable WITH our pkg frames is dropped on a current clean browser (real #6827)",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: {
        values: [
          {
            type: "RuntimeError",
            value: "unreachable",
            mechanism: {
              type: "auto.browser.global_handlers.onerror",
              handled: false,
            },
            stacktrace: {
              frames: [
                { filename: "/pkg/2b494b1/ultros.js", function: "c" },
                {
                  filename:
                    "/pkg/2b494b1/ultros.wasm:wasm-function[5501]:0x5e8bff",
                  function: "?",
                },
              ],
            },
          },
        ],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: true,
  },
  {
    name: "Firefox phrasing 'unreachable executed' from our wasm frame is dropped",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: {
        values: [
          {
            type: "RuntimeError",
            value: "unreachable executed",
            stacktrace: {
              frames: [
                {
                  filename:
                    "/pkg/2b494b1/ultros.wasm:wasm-function[5501]:0x5e8bff",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "RuntimeError unreachable from a THIRD-PARTY wasm module (no pkg frame) is preserved",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: {
        values: [
          {
            type: "RuntimeError",
            value: "unreachable",
            stacktrace: {
              frames: [
                {
                  filename:
                    "https://cdn.example.com/widget.wasm:wasm-function[12]:0x100",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: false,
  },
  {
    // GlitchTip #6848 (Mediapartners-Google crawler hitting the #6831 hydration
    // panic on /item/Zeromus/34430, release 8ccc782). Same redundant onerror
    // trap as #6827 above, but the browser named EVERY wasm frame with the
    // engine-internal `wasm://wasm/<hash>:wasm-function[N]` scheme instead of the
    // `/pkg/<hash>/ultros.wasm` source-URL form — so the pkg-frame check never
    // fired and the twin leaked. A RuntimeError "unreachable" whose stack is
    // ENTIRELY wasm-module frames is a wasm abort trap; the only wasm on an
    // Ultros page is our bundle, so it is the guaranteed duplicate of the kept
    // RustWasmPanic and safe to drop.
    name: "onerror RuntimeError unreachable with ONLY wasm://wasm module frames is dropped (real #6848 crawler variant)",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: {
        values: [
          {
            type: "RuntimeError",
            value: "unreachable",
            mechanism: {
              type: "auto.browser.global_handlers.onerror",
              handled: false,
            },
            stacktrace: {
              frames: [
                {
                  filename:
                    "wasm://wasm/02e8784e:wasm-function[14091]:0x844cae",
                  function: "?",
                },
                {
                  filename: "wasm://wasm/02e8784e:wasm-function[5503]:0x5eaa4c",
                  function: "?",
                },
              ],
            },
          },
        ],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: true,
  },
  {
    // Over-drop guard: the all-wasm-module rule must require EVERY frame to be a
    // wasm-module frame. A stack that reaches even one third-party JS frame is
    // not provably ours (and could be a genuine error worth seeing), so it is
    // preserved — mirroring the "any app/pkg frame preserves" spirit of the
    // pkg-frame branch and category 8.
    name: "RuntimeError unreachable with a wasm://wasm frame BUT also a third-party JS frame is preserved",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: {
        values: [
          {
            type: "RuntimeError",
            value: "unreachable",
            stacktrace: {
              frames: [
                { filename: "wasm://wasm/deadbeef:wasm-function[3]:0x40" },
                {
                  filename: "https://cdn.example.com/widget.js",
                  function: "w",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: false,
  },
  {
    // Value-scope guard for the wasm://wasm branch: only the `unreachable` abort
    // signature is a known RustWasmPanic twin. Another wasm trap value (e.g.
    // "memory access out of bounds") from wasm://wasm frames is a distinct fault
    // with no retained copy, so it still reports.
    name: "a non-unreachable RuntimeError from only wasm://wasm frames is preserved",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: {
        values: [
          {
            type: "RuntimeError",
            value: "memory access out of bounds",
            stacktrace: {
              frames: [
                { filename: "wasm://wasm/02e8784e:wasm-function[42]:0x1234" },
              ],
            },
          },
        ],
      },
    },
    expectDrop: false,
  },
  {
    name: "a genuine non-unreachable RuntimeError from our bundle is preserved",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: {
        values: [
          {
            type: "RuntimeError",
            value: "memory access out of bounds",
            stacktrace: {
              frames: [{ filename: "/pkg/2b494b1/ultros.js" }],
            },
          },
        ],
      },
    },
    expectDrop: false,
  },

  // ── Category 7: the RefCell-already-borrowed executor cascade (dedup) ──
  // A handled Rust panic whose value is "RefCell already borrowed" AND whose
  // contexts.rust_panic.location is the wasm-bindgen-futures single-threaded
  // executor (js-sys .../futures/task/singlethread.rs) is never a primary,
  // actionable fault. That executor RefCell only trips "already borrowed" when
  // run() is re-entered while a panic is unwinding through a future poll, so it
  // is ALWAYS the secondary cascade of a primary panic the hook already
  // reported with its own actionable location. Dropped UNCONDITIONALLY (no
  // injecting-population fingerprint) — the retained primary keeps the bug
  // visible. This is #6758 (23k+ events), the single largest issue, the
  // RefCell twin of the per-deploy RuntimeError flood Category 6 / PR #921
  // dedups. Scoped tightly: keyed on the executor location, so an APP-code
  // double-borrow (which panics at an app/leptos path, NOT singlethread.rs)
  // still reports, and a RefCell event with no rust_panic context is preserved.
  {
    name: "genuine APP RefCell double-borrow (leptos location, not the executor) is preserved",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      contexts: {
        rust_panic: {
          location:
            "/usr/local/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/" +
            "reactive_graph-0.2.5/src/signal/guards.rs:120:14",
        },
      },
      exception: {
        values: [{ type: "RustWasmPanic", value: "RefCell already borrowed" }],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: false,
  },
  {
    name: "RefCell already borrowed with NO rust_panic context is preserved (cannot prove it is the executor cascade)",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      exception: {
        values: [{ type: "RustWasmPanic", value: "RefCell already borrowed" }],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: false,
  },
  {
    name: "a DIFFERENT panic value at the js-sys executor location is preserved (only the RefCell cascade is a known twin)",
    ua: CURRENT_CHROME,
    document: fakeDocumentEx({ fontCount: 0 }),
    event: {
      contexts: { rust_panic: { location: JS_SYS_SINGLETHREAD_LOC } },
      exception: {
        values: [
          { type: "RustWasmPanic", value: "some other executor invariant" },
        ],
      },
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    },
    expectDrop: false,
  },

  // ── Category 1: WASM/bundle fetch aborts ──
  {
    name: 'TypeError "Failed to fetch dynamically imported module" of the pkg bundle is dropped',
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value:
              "Failed to fetch dynamically imported module: " +
              "https://ultros.app/pkg/c994ea6/ultros.js",
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "dynamic import failure of the pkg bundle on the www host is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value:
              "Failed to fetch dynamically imported module: " +
              "https://www.ultros.app/pkg/c994ea6/ultros.js",
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "bare Failed to fetch from a pkg-bundle stack frame is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value: "Failed to fetch",
            stacktrace: {
              frames: [{ filename: "/pkg/c994ea6/ultros.js" }],
            },
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "WebAssembly compilation aborted is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value: "WebAssembly compilation aborted: aborted",
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "dynamic import failure of a NON-pkg (third-party) module is preserved",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value:
              "Failed to fetch dynamically imported module: " +
              "https://cdn.example.com/widget.js",
          },
        ],
      },
    },
    expectDrop: false,
  },
  {
    name: "bare Failed to fetch from an API call (non-pkg frame) is preserved",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value: "Failed to fetch",
            stacktrace: {
              frames: [{ filename: "https://ultros.app/api/v1/listings" }],
            },
          },
        ],
      },
    },
    expectDrop: false,
  },

  // ── Category 2: injected document TypeError ──
  {
    name: "injected HTMLDocument.c document TypeError is dropped",
    ua: FROZEN_CHROME_112,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value: "Cannot read properties of undefined (reading 'document')",
            stacktrace: { frames: [{ function: "HTMLDocument.c" }] },
          },
        ],
      },
    },
    expectDrop: true,
  },

  // ── Category 4: empty (undefined/null) promise rejections ──
  {
    name: "Non-Error promise rejection with value undefined is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "UnhandledRejection",
            value: "Non-Error promise rejection captured with value: undefined",
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "Non-Error promise rejection with value null is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "UnhandledRejection",
            value: "Non-Error promise rejection captured with value: null",
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "Non-Error promise rejection carrying a real object is preserved",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "UnhandledRejection",
            value:
              "Non-Error promise rejection captured with value: [object Object]",
          },
        ],
      },
    },
    expectDrop: false,
  },

  // ── Category 1 (cont.): streaming WASM compile failures of our bundle ──
  // A non-OK HTTP response for the .wasm (a stale chunk 404 in the seconds
  // after a deploy) surfaces as a compile TypeError; a truncated download
  // surfaces as a CompileError. Both are network / deploy-race noise, never an
  // actionable code bug. GlitchTip #6755, #6762, #6763, #6764, #6766, #6767.
  {
    name: "WASM compile TypeError 'HTTP status code is not ok' (stale chunk after deploy) is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value:
              "Failed to execute 'compile' on 'WebAssembly': HTTP status code is not ok",
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "WASM CompileError 'extends past end of the module' (truncated download) is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "CompileError",
            value:
              'WebAssembly.instantiateStreaming(): section (code 10, "Code") ' +
              "extends past end of the module (length 10426626, remaining bytes " +
              "10359267) @+126488",
          },
        ],
      },
    },
    expectDrop: true,
  },
  // Guard: a CompileError that is NOT a truncation (e.g. a genuinely corrupt
  // build we shipped) must still report — that is a real bug worth seeing.
  {
    name: "a non-truncation WASM CompileError is preserved",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "CompileError",
            value:
              "WebAssembly.compile(): function body must end with END opcode @+1234",
          },
        ],
      },
    },
    expectDrop: false,
  },

  // ── Category 5: stripped leptos streaming-hydration bootstrap ──
  // The SSR shell ALWAYS emits window.__RESOLVED_RESOURCES / __INCOMPLETE_CHUNKS
  // / __PENDING_RESOURCES / __SERIALIZED_ERRORS (verified in the served HTML of
  // the failing /item/<world>/<id> URLs), and they are read only by leptos's
  // generated wasm-bindgen static accessors, never by app code. So a
  // "ReferenceError: <name> is not defined" can only mean a proxy stripped or
  // truncated the streamed bootstrap before the wasm hydrated — the same
  // translation-proxy population behind the tachys flood. GlitchTip #6620,
  // #6667, #6760, #6761.
  {
    name: "ReferenceError __RESOLVED_RESOURCES not defined from the pkg accessor is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "ReferenceError",
            value: "__RESOLVED_RESOURCES is not defined",
            stacktrace: {
              frames: [
                { filename: "/pkg/a2d6028/ultros.js", function: "c" },
                {
                  filename: "/pkg/a2d6028/ultros.js",
                  function:
                    "__wbg_static_accessor___RESOLVED_RESOURCES_64c55267f5301918",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "ReferenceError __INCOMPLETE_CHUNKS not defined from the pkg accessor is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "ReferenceError",
            value: "__INCOMPLETE_CHUNKS is not defined",
            stacktrace: {
              frames: [
                {
                  filename: "/pkg/a2d6028/ultros.js",
                  function:
                    "__wbg_static_accessor___INCOMPLETE_CHUNKS_69295e643f835e34",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "ReferenceError __PENDING_RESOURCES not defined (accessor fn, no pkg filename) is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "ReferenceError",
            value: "__PENDING_RESOURCES is not defined",
            stacktrace: {
              frames: [
                { function: "__wbg_static_accessor___PENDING_RESOURCES_abc123" },
              ],
            },
          },
        ],
      },
    },
    expectDrop: true,
  },
  {
    name: "ReferenceError __SERIALIZED_ERRORS not defined with NO frames falls back to value match and is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "ReferenceError",
            value: "__SERIALIZED_ERRORS is not defined",
          },
        ],
      },
    },
    expectDrop: true,
  },
  // Guard: a same-named global thrown from a THIRD-PARTY (non-pkg) script must
  // NOT be swept up — the bundle scoping keeps it narrow.
  {
    name: "ReferenceError __RESOLVED_RESOURCES from a third-party (non-pkg) frame is preserved",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "ReferenceError",
            value: "__RESOLVED_RESOURCES is not defined",
            stacktrace: {
              frames: [
                {
                  filename: "https://cdn.example.com/widget.js",
                  function: "init",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: false,
  },
  // Guard: an ordinary application ReferenceError from our own bundle must
  // always report — only the leptos hydration globals are matched.
  {
    name: "a genuine application ReferenceError from the pkg bundle is preserved",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "ReferenceError",
            value: "someAppHelper is not defined",
            stacktrace: {
              frames: [{ filename: "/pkg/a2d6028/ultros.js", function: "c" }],
            },
          },
        ],
      },
    },
    expectDrop: false,
  },

  // ── Category 8: third-party analytics / ads / CDN-telemetry script noise ──
  // The Cloudflare Web Analytics beacon throwing on an ancient browser that
  // lacks Array.prototype.at — GlitchTip #6836 (Chrome 90 / Android 5). All
  // frames live on static.cloudflareinsights.com (host carried in absPath; the
  // filename is path-only). Not our code, not fixable here.
  {
    name: "Cloudflare Web Analytics beacon TypeError (all frames third-party) is dropped",
    ua:
      "Mozilla/5.0 (Linux; Android 5.0; SM-G900P Build/LRX21T) " +
      "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/90.0.4430.93 " +
      "Mobile Safari/537.36",
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value: "t.entries.at is not a function",
            mechanism: {
              type: "auto.browser.global_handlers.onerror",
              handled: false,
            },
            stacktrace: {
              frames: [
                {
                  filename: "/beacon.min.js/v4513226cdae34746b4dedf0b4dfa099e",
                  absPath:
                    "https://static.cloudflareinsights.com/beacon.min.js/v4513226cdae34746b4dedf0b4dfa099e",
                  function: "o",
                },
                {
                  filename: "/beacon.min.js/v4513226cdae34746b4dedf0b4dfa099e",
                  absPath:
                    "https://static.cloudflareinsights.com/beacon.min.js/v4513226cdae34746b4dedf0b4dfa099e",
                  function: "B",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: true,
  },
  // A gtag / Google-Analytics onerror whose whole stack is on Google
  // analytics hosts — likewise unactionable external noise.
  {
    name: "a gtag / Google-Analytics onerror (all frames third-party) is dropped",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value: "Cannot read properties of null (reading 'v')",
            stacktrace: {
              frames: [
                {
                  filename: "/gtag/js",
                  absPath:
                    "https://www.googletagmanager.com/gtag/js?id=G-WYVZLM39M3",
                  function: "?",
                },
                {
                  filename: "/g/collect",
                  absPath: "https://www.google-analytics.com/g/collect",
                  function: "?",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: true,
  },
  // SAFETY: a mixed stack that reaches even one of our own frames is a real
  // Ultros bug (a third-party callback into our code, or vice-versa) and MUST
  // report — the all-frames-third-party gate preserves it.
  {
    name: "a mixed stack with even one app/pkg frame is preserved (real Ultros bug never swept up)",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value: "x is not a function",
            stacktrace: {
              frames: [
                {
                  filename: "/pkg/97f9168/ultros.js",
                  absPath: "https://ultros.app/pkg/97f9168/ultros.js",
                  function: "c",
                },
                {
                  filename: "/beacon.min.js/v451",
                  absPath:
                    "https://static.cloudflareinsights.com/beacon.min.js/v451",
                  function: "o",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: false,
  },
  // SAFETY: our own origin (ultros.app) is deliberately NOT on the third-party
  // host list, so an inline page-script error still reports.
  {
    name: "an error from an inline ultros.app page script is preserved (our origin is not third-party)",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [
          {
            type: "TypeError",
            value: "boom",
            stacktrace: {
              frames: [
                {
                  filename: "/item/Excalibur/2465",
                  absPath: "https://ultros.app/item/Excalibur/2465",
                  function: "onclick",
                },
              ],
            },
          },
        ],
      },
    },
    expectDrop: false,
  },
  // SAFETY: a frameless error carries no proof of origin — never assume
  // third-party, always preserve.
  {
    name: "a third-party-looking TypeError with NO stack frames is preserved",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [{ type: "TypeError", value: "t.entries.at is not a function" }],
      },
    },
    expectDrop: false,
  },

  // ── Genuine application errors must always survive ──
  {
    name: "a genuine application Error is preserved",
    ua: CURRENT_CHROME,
    event: {
      exception: {
        values: [{ type: "Error", value: "list rename failed: 500" }],
      },
    },
    expectDrop: false,
  },
  {
    name: "an empty event never throws and is preserved",
    ua: CURRENT_CHROME,
    event: {},
    expectDrop: false,
  },
];

for (const c of cases) {
  test(c.name, () => {
    const shouldDrop = loadFilter(c.ua, c.document);
    const dropped = shouldDrop(c.event) === true;
    assert.strictEqual(
      dropped,
      c.expectDrop,
      c.expectDrop
        ? "expected this event to be dropped as noise"
        : "expected this event to be PRESERVED (filter is over-suppressing)",
    );
  });
}
