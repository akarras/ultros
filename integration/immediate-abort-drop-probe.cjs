// Probe: after `panic = "immediate-abort"`, does a NON-hydration Rust panic
// still reach GlitchTip for the populations rule 3 gates on?
const fs = require("node:fs");
const path = require("node:path");

const SRC = fs.readFileSync(
  path.join(__dirname, "..", "ultros-frontend", "ultros-app", "src", "error_filter.js"),
  "utf8",
);

function loadFilter(userAgent, documentObj) {
  const win = {};
  if (documentObj !== undefined) win.document = documentObj;
  const factory = new Function("window", "navigator", SRC);
  factory(win, { userAgent: userAgent || "" });
  return win.__ultrosShouldDropEvent;
}

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
        contains: (c) => classes.indexOf(c) !== -1,
      },
    },
  };
}

const staleChromeUA = (m) =>
  `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${m}.0.0.0 Safari/537.36`;
const CURRENT_CHROME =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";

// What an immediate-abort panic looks like at window.onerror in prod:
// a bare V8 RuntimeError, NO contexts.rust_panic, wasm frames unresolved
// (beforeSend's drop check runs BEFORE wasm_symbolicate.js).
// This one is NOT a hydration panic -- it is, say, an unwrap in the analyzer.
function immediateAbortTrap() {
  return {
    exception: {
      values: [
        {
          type: "RuntimeError",
          value: "unreachable",
          stacktrace: {
            frames: [
              {
                filename: "https://ultros.app/pkg/abc1234/ultros.wasm",
                abs_path: "https://ultros.app/pkg/abc1234/ultros.wasm",
                function: "wasm-function[54321]",
              },
            ],
          },
        },
      ],
    },
    breadcrumbs: { values: [{ category: "console", message: "app run!" }] },
    tags: {},
  };
}

const cases = [
  ["stale Chrome 112 (in-app WebView), clean DOM", staleChromeUA(112), fakeDocumentEx({})],
  ["stale Chrome 108 (crawler/WebView), clean DOM", staleChromeUA(108), fakeDocumentEx({})],
  ["current Chrome + translation overlay (<font>)", CURRENT_CHROME, fakeDocumentEx({ fontCount: 3 })],
  ["current Chrome + translated-ltr on <html>", CURRENT_CHROME, fakeDocumentEx({ htmlClass: "translated-ltr" })],
  ["current Chrome, clean DOM (control)", CURRENT_CHROME, fakeDocumentEx({})],
];

console.log(
  "A NON-hydration Rust panic in prod (immediate-abort trap). dropped=true means the panic is SILENTLY LOST:\n",
);
for (const [name, ua, doc] of cases) {
  const shouldDrop = loadFilter(ua, doc);
  const dropped = shouldDrop(immediateAbortTrap());
  console.log(`  dropped=${String(dropped).padEnd(5)}  ${name}`);
}
