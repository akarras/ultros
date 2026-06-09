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

// Load the filter with a given UA and return its `shouldDrop` predicate.
function loadFilter(userAgent) {
  const win = {};
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
    name: "hydration panic on a CURRENT browser is preserved (real bugs still reach GlitchTip)",
    ua: CURRENT_CHROME,
    event: rustPanic(HYDRATION_LOC, {
      breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    }),
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
    const shouldDrop = loadFilter(c.ua);
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
