// Unit tests for the Sentry `beforeSend` wasm symbolicator defined in
// ../ultros-frontend/ultros-app/src/wasm_symbolicate.js.
//
// The source references `window` as a free identifier and reaches fetch /
// AbortController / timers through it, so each case builds a fake `window`
// with a scripted `fetch` and loads the file via
// `new Function('window', src)` — the same shape error-filter.test.cjs uses.
//
// Run with: node --test integration/wasm-symbolicate.test.cjs
//   (or: npm --prefix integration run test:wasm-symbolicate)

const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const SRC = fs.readFileSync(
  path.join(
    __dirname,
    "..",
    "ultros-frontend",
    "ultros-app",
    "src",
    "wasm_symbolicate.js",
  ),
  "utf8",
);

const SYMBOLS = [
  "7:std::panicking::rust_panic_with_hook",
  "9:core::panicking::panic_fmt",
  "12:ultros_client::set_panic_hook::{{closure}}",
  "13:console_error_panic_hook::hook",
  "20:core::option::unwrap_failed",
  "42:ultros_app::routes::item_view::ItemView::{{closure}}",
  "43:leptos::callback::Callback::run",
  "44:xiv_gen_db::data",
  "45:reactive_graph::signal::read::ReadSignal<T>::try_get",
  "46:<ultros_app::routes::item_view::ItemView as core::ops::function::FnOnce<()>>::call_once",
  "47:<<ultros_ui_grid::components::virtual_grid::VirtualGrid<u32> as tachys::view::Render>::State as tachys::view::Mountable>::unmount",
  "48:ultros_ui_grid::components::virtual_grid::query_grid::__component_query_grid<(usize, alloc::sync::Arc<ultros_app::routes::recipe_analyzer::RecipeProfitData>), xiv_gen::RecipeId>::{closure#3}",
].join("\n");

const WASM = "https://ultros.app/pkg/9b93be1/ultros.wasm";

function wasmFrame(index, name) {
  const filename = `/pkg/9b93be1/ultros.wasm:wasm-function[${index}]:0xca0ed2`;
  const f = { filename, abs_path: `${WASM}:wasm-function[${index}]:0xca0ed2` };
  if (name !== undefined) f.function = name;
  else f.function = "?";
  return f;
}

// A frame from a second module in the pkg dir rather than from
// `ultros.wasm` — what `cargo leptos build --split` emits per lazy route.
function chunkFrame(chunk, index) {
  const path = `/pkg/9b93be1/chunk_${chunk}.wasm`;
  return {
    filename: `${path}:wasm-function[${index}]:0x40`,
    abs_path: `https://ultros.app${path}:wasm-function[${index}]:0x40`,
    function: "?",
  };
}

function glueFrame(fn) {
  return {
    filename: "/pkg/9b93be1/ultros.js",
    abs_path: "https://ultros.app/pkg/9b93be1/ultros.js",
    function: fn,
    lineno: 2,
    colno: 4169,
  };
}

// Oldest-first, like Sentry stores them: the panic site is in the middle,
// the panic machinery and the Error() glue are at the END (top of stack).
function panicEvent() {
  return {
    exception: {
      values: [
        {
          type: "RustWasmPanic",
          value: "called `Option::unwrap()` on a `None` value",
          stacktrace: {
            frames: [
              glueFrame("__wbg_adapter_50"),
              wasmFrame(43),
              wasmFrame(42),
              wasmFrame(20),
              wasmFrame(9),
              wasmFrame(7),
              wasmFrame(12),
              wasmFrame(13),
              glueFrame("__wbg_new_8a6f238a6ece86ea"),
            ],
          },
        },
      ],
    },
  };
}

// Builds a window whose fetch replies per `script(url)`: a string is a 200
// body, a number is a non-OK status, an Error rejects, and "hang" never
// settles (so the abort timeout fires).
function loadSymbolicator(script) {
  const calls = [];
  const timers = [];
  const win = {
    fetch(url, opts) {
      calls.push(url);
      const reply = script(url);
      if (reply instanceof Error) return Promise.reject(reply);
      if (reply === "hang") {
        return new Promise((_, reject) => {
          opts.signal.addEventListener("abort", () =>
            reject(new Error("aborted")),
          );
        });
      }
      if (typeof reply === "number") {
        return Promise.resolve({ ok: false, status: reply, text: () => "" });
      }
      return Promise.resolve({
        ok: true,
        status: 200,
        text: () => Promise.resolve(reply),
      });
    },
    AbortController: class {
      constructor() {
        const listeners = [];
        this.signal = {
          addEventListener: (_, cb) => listeners.push(cb),
        };
        this.abort = () => listeners.forEach((cb) => cb());
      }
    },
    setTimeout(cb, ms) {
      timers.push({ cb, ms });
      return timers.length;
    },
    clearTimeout(id) {
      if (timers[id - 1]) timers[id - 1].cleared = true;
    },
  };
  // eslint-disable-next-line no-new-func
  new Function("window", SRC)(win);
  assert.strictEqual(
    typeof win.__ultrosSymbolicateEvent,
    "function",
    "wasm_symbolicate.js must define window.__ultrosSymbolicateEvent",
  );
  return { symbolicate: win.__ultrosSymbolicateEvent, calls, timers };
}

function frames(event) {
  return event.exception.values[0].stacktrace.frames;
}

test("fills function names from the sibling symbols file", async () => {
  const { symbolicate, calls } = loadSymbolicator(() => SYMBOLS);
  const out = await symbolicate(panicEvent());
  assert.deepStrictEqual(calls, [
    "https://ultros.app/pkg/9b93be1/ultros.symbols",
  ]);
  const fns = frames(out).map((f) => f.function);
  assert.deepStrictEqual(fns, [
    "__wbg_adapter_50",
    "leptos::callback::Callback::run",
    "ultros_app::routes::item_view::ItemView::{{closure}}",
    "core::option::unwrap_failed",
  ]);
});

test("flags ultros and xiv_gen frames as in_app", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const ev = panicEvent();
  frames(ev).splice(1, 0, wasmFrame(44), wasmFrame(45));
  const out = await symbolicate(ev);
  const inApp = frames(out).map((f) => [f.function, f.in_app]);
  assert.deepStrictEqual(inApp, [
    ["__wbg_adapter_50", undefined],
    ["xiv_gen_db::data", true],
    ["reactive_graph::signal::read::ReadSignal<T>::try_get", false],
    ["leptos::callback::Callback::run", false],
    ["ultros_app::routes::item_view::ItemView::{{closure}}", true],
    ["core::option::unwrap_failed", false],
  ]);
});

test("trait-impl names starting with < are in_app too", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const ev = panicEvent();
  ev.exception.values[0].stacktrace.frames = [
    wasmFrame(46),
    wasmFrame(47),
    wasmFrame(45),
  ];
  const out = await symbolicate(ev);
  assert.deepStrictEqual(
    frames(out).map((f) => f.in_app),
    [true, true, false],
  );
});

test("filename is left alone; only function and in_app change", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const out = await symbolicate(panicEvent());
  const site = frames(out)[2];
  assert.strictEqual(
    site.filename,
    "/pkg/9b93be1/ultros.wasm:wasm-function[42]:0xca0ed2",
  );
  assert.strictEqual(site.abs_path, `${WASM}:wasm-function[42]:0xca0ed2`);
});

test("unknown indices keep their placeholder name", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const ev = panicEvent();
  frames(ev).splice(1, 0, wasmFrame(99999));
  const out = await symbolicate(ev);
  assert.strictEqual(frames(out)[1].function, "?");
});

test("fetches the map once across events", async () => {
  const { symbolicate, calls } = loadSymbolicator(() => SYMBOLS);
  await symbolicate(panicEvent());
  await symbolicate(panicEvent());
  assert.strictEqual(calls.length, 1);
});

test("memoizes a failed fetch too", async () => {
  const { symbolicate, calls } = loadSymbolicator(() => 404);
  const a = await symbolicate(panicEvent());
  const b = await symbolicate(panicEvent());
  assert.strictEqual(calls.length, 1);
  assert.deepStrictEqual(a, panicEvent());
  assert.deepStrictEqual(b, panicEvent());
});

test("network rejection leaves the event untouched", async () => {
  const { symbolicate } = loadSymbolicator(() => new Error("offline"));
  const out = await symbolicate(panicEvent());
  assert.deepStrictEqual(out, panicEvent());
});

test("a hung fetch is aborted after the timeout", async () => {
  const { symbolicate, timers } = loadSymbolicator(() => "hang");
  const pending = symbolicate(panicEvent());
  assert.strictEqual(timers.length, 1);
  assert.strictEqual(timers[0].ms, 10000);
  timers[0].cb();
  const out = await pending;
  assert.deepStrictEqual(out, panicEvent());
});

test("events without wasm frames never fetch", async () => {
  const { symbolicate, calls } = loadSymbolicator(() => SYMBOLS);
  const ev = {
    exception: {
      values: [
        {
          type: "TypeError",
          value: "x is not a function",
          stacktrace: { frames: [glueFrame("a"), glueFrame("b")] },
        },
      ],
    },
  };
  const out = await symbolicate(ev);
  assert.strictEqual(calls.length, 0);
  assert.deepStrictEqual(out, ev);
});

test("events with no exception pass through", async () => {
  const { symbolicate, calls } = loadSymbolicator(() => SYMBOLS);
  const ev = { message: "hello" };
  assert.strictEqual(await symbolicate(ev), ev);
  assert.strictEqual(calls.length, 0);
});

test("frames that already carry a name are not rewritten", async () => {
  const { symbolicate, calls } = loadSymbolicator(() => SYMBOLS);
  const ev = {
    exception: {
      values: [
        {
          type: "RuntimeError",
          value: "unreachable",
          stacktrace: {
            frames: [
              wasmFrame(42, "ultros_app::routes::item_view::ItemView::{{closure}}::h0123"),
            ],
          },
        },
      ],
    },
  };
  const out = await symbolicate(ev);
  assert.strictEqual(calls.length, 0);
  assert.strictEqual(
    frames(out)[0].function,
    "ultros_app::routes::item_view::ItemView::{{closure}}::h0123",
  );
});

test("never trims the last remaining frame", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const ev = panicEvent();
  ev.exception.values[0].stacktrace.frames = [wasmFrame(9), wasmFrame(7)];
  const out = await symbolicate(ev);
  assert.deepStrictEqual(
    frames(out).map((f) => f.function),
    ["core::panicking::panic_fmt"],
  );
});

test("derives the map URL from the frame's own pkg hash", async () => {
  const { symbolicate, calls } = loadSymbolicator(() => SYMBOLS);
  const ev = panicEvent();
  for (const f of frames(ev)) {
    f.filename = f.filename.replace("9b93be1", "0ldh4sh");
    f.abs_path = f.abs_path.replace("9b93be1", "0ldh4sh");
  }
  await symbolicate(ev);
  assert.deepStrictEqual(calls, [
    "https://ultros.app/pkg/0ldh4sh/ultros.symbols",
  ]);
});

test("symbolicates every exception in a chain", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const ev = panicEvent();
  ev.exception.values.push({
    type: "RustWasmPanic",
    value: "second",
    stacktrace: { frames: [wasmFrame(44)] },
  });
  const out = await symbolicate(ev);
  assert.strictEqual(
    out.exception.values[1].stacktrace.frames[0].function,
    "xiv_gen_db::data",
  );
});

test("a throwing fetch implementation cannot break the event", async () => {
  const { symbolicate } = loadSymbolicator(() => {
    throw new Error("boom");
  });
  const out = await symbolicate(panicEvent());
  assert.deepStrictEqual(out, panicEvent());
});

// ── Trap events: production panics under panic=immediate-abort ──
// No hook runs, so the panic surfaces as the browser's own
// "RuntimeError: unreachable" from window.onerror. After symbolication the
// event gets a fingerprint from its top frames (filenames carry
// `wasm-function[N]:0x…`, which changes every deploy, so default grouping
// would open a new issue per release) and a title naming the site.

function trapEvent(value, frames) {
  return {
    exception: {
      values: [
        {
          type: "RuntimeError",
          value: value,
          mechanism: { type: "onerror", handled: false },
          stacktrace: {
            frames: frames || [
              glueFrame("__wbg_adapter_50"),
              wasmFrame(43),
              wasmFrame(42),
              wasmFrame(20),
              wasmFrame(9),
            ],
          },
        },
      ],
    },
  };
}

test("trap: fingerprint is the top three frames, top first", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const out = await symbolicate(trapEvent("unreachable"));
  assert.deepStrictEqual(out.fingerprint, [
    "rust-wasm-trap",
    "core::option::unwrap_failed",
    "ultros_app::routes::item_view::ItemView::{{closure}}",
    "leptos::callback::Callback::run",
  ]);
});

test("trap: value names the topmost in_app frame", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const out = await symbolicate(trapEvent("unreachable"));
  assert.strictEqual(
    out.exception.values[0].value,
    "unreachable in ultros_app::routes::item_view::ItemView::{{closure}}",
  );
  assert.strictEqual(out.exception.values[0].type, "RuntimeError");
});

test("trap: falls back to the top frame when nothing is in_app", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const out = await symbolicate(
    trapEvent("unreachable", [wasmFrame(45), wasmFrame(43)]),
  );
  assert.strictEqual(
    out.exception.values[0].value,
    "unreachable in leptos::callback::Callback::run",
  );
  assert.deepStrictEqual(out.fingerprint, [
    "rust-wasm-trap",
    "leptos::callback::Callback::run",
    "reactive_graph::signal::read::ReadSignal::try_get",
  ]);
});

test("trap: Firefox and JSC phrasings are recognised", async () => {
  for (const value of [
    "unreachable executed",
    "Unreachable code should not be executed",
  ]) {
    const { symbolicate } = loadSymbolicator(() => SYMBOLS);
    const out = await symbolicate(trapEvent(value));
    assert.strictEqual(out.fingerprint[0], "rust-wasm-trap", value);
    assert.strictEqual(
      out.exception.values[0].value,
      value + " in ultros_app::routes::item_view::ItemView::{{closure}}",
    );
  }
});

test("trap: untouched when the map is unavailable", async () => {
  const { symbolicate } = loadSymbolicator(() => 404);
  const out = await symbolicate(trapEvent("unreachable"));
  assert.strictEqual(out.fingerprint, undefined);
  assert.strictEqual(out.exception.values[0].value, "unreachable");
});

test("trap: untouched when no frame resolved", async () => {
  const { symbolicate } = loadSymbolicator(() => "1:nothing_useful\n");
  const out = await symbolicate(trapEvent("unreachable"));
  assert.strictEqual(out.fingerprint, undefined);
  assert.strictEqual(out.exception.values[0].value, "unreachable");
});

test("trap: an existing fingerprint is respected", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const ev = trapEvent("unreachable");
  ev.fingerprint = ["custom"];
  const out = await symbolicate(ev);
  assert.deepStrictEqual(out.fingerprint, ["custom"]);
});

test("trap: other RuntimeErrors and RustWasmPanic are not retitled", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const oob = await symbolicate(trapEvent("memory access out of bounds"));
  assert.strictEqual(oob.fingerprint, undefined);
  assert.strictEqual(
    oob.exception.values[0].value,
    "memory access out of bounds",
  );
  const hooked = await symbolicate(panicEvent());
  assert.strictEqual(hooked.fingerprint, undefined);
  assert.strictEqual(
    hooked.exception.values[0].value,
    "called `Option::unwrap()` on a `None` value",
  );
});

test("trap: generic arguments are cut from the fingerprint and title", async () => {
  const { symbolicate } = loadSymbolicator(() => SYMBOLS);
  const out = await symbolicate(
    trapEvent("unreachable", [wasmFrame(43), wasmFrame(47), wasmFrame(48)]),
  );
  assert.strictEqual(
    out.exception.values[0].value,
    "unreachable in ultros_ui_grid::components::virtual_grid::query_grid::__component_query_grid::{closure#3}",
  );
  assert.deepStrictEqual(out.fingerprint, [
    "rust-wasm-trap",
    "ultros_ui_grid::components::virtual_grid::query_grid::__component_query_grid::{closure#3}",
    "<<ultros_ui_grid::components::virtual_grid::VirtualGrid<u32> as tachys::view::Render>::State as tachys::view::Mountable>::unmount",
    "leptos::callback::Callback::run",
  ]);
  // Frames keep their full names; only the labels are shortened.
  assert.strictEqual(
    frames(out)[2].function,
    "ultros_ui_grid::components::virtual_grid::query_grid::__component_query_grid<(usize, alloc::sync::Arc<ultros_app::routes::recipe_analyzer::RecipeProfitData>), xiv_gen::RecipeId>::{closure#3}",
  );
});

// ── Several modules in one trace ──
// Function indices are per-module, so a frame must be resolved against the
// map of the module its own filename names. The current build emits one
// module; `cargo leptos build --split` emits a chunk per lazy route (that
// pilot was reverted in #1588, so these guard the property rather than
// describe today's bundle).

test("chunk frames resolve against that chunk's own map", async () => {
  const CHUNK = "3:ultros_app::routes::analyzer::AnalyzerWorld::{closure#1}";
  const { symbolicate, calls } = loadSymbolicator((url) =>
    url.indexOf("chunk_7") === -1 ? SYMBOLS : CHUNK,
  );
  const ev = trapEvent("unreachable", [wasmFrame(43), chunkFrame(7, 3)]);
  const out = await symbolicate(ev);
  assert.deepStrictEqual(calls.sort(), [
    "https://ultros.app/pkg/9b93be1/chunk_7.symbols",
    "https://ultros.app/pkg/9b93be1/ultros.symbols",
  ]);
  assert.deepStrictEqual(
    frames(out).map((f) => f.function),
    [
      "leptos::callback::Callback::run",
      "ultros_app::routes::analyzer::AnalyzerWorld::{closure#1}",
    ],
  );
  assert.strictEqual(
    out.exception.values[0].value,
    "unreachable in ultros_app::routes::analyzer::AnalyzerWorld::{closure#1}",
  );
});

test("each module's map is fetched once, and one 404 does not sink the rest", async () => {
  const { symbolicate, calls } = loadSymbolicator((url) =>
    url.indexOf("chunk_7") === -1 ? SYMBOLS : 404,
  );
  const ev = trapEvent("unreachable", [
    wasmFrame(43),
    chunkFrame(7, 3),
    wasmFrame(42),
  ]);
  const out = await symbolicate(ev);
  assert.strictEqual(calls.length, 2);
  assert.deepStrictEqual(
    frames(out).map((f) => f.function),
    [
      "leptos::callback::Callback::run",
      "?",
      "ultros_app::routes::item_view::ItemView::{{closure}}",
    ],
  );
  await symbolicate(trapEvent("unreachable", [chunkFrame(7, 3)]));
  assert.strictEqual(calls.length, 2);
});

test("indices are not mixed up between modules", async () => {
  // Index 43 exists in both maps with different names; each frame must take
  // the name from ITS OWN module.
  const CHUNK = "43:ultros_app::routes::lists::ListView::render";
  const { symbolicate } = loadSymbolicator((url) =>
    url.indexOf("chunk_2") === -1 ? SYMBOLS : CHUNK,
  );
  const out = await symbolicate(
    trapEvent("unreachable", [wasmFrame(43), chunkFrame(2, 43)]),
  );
  assert.deepStrictEqual(
    frames(out).map((f) => f.function),
    [
      "leptos::callback::Callback::run",
      "ultros_app::routes::lists::ListView::render",
    ],
  );
});

test("a non-pkg wasm module is ignored", async () => {
  const { symbolicate, calls } = loadSymbolicator(() => SYMBOLS);
  const ev = trapEvent("unreachable", [
    {
      filename: "https://cdn.example.com/thirdparty.wasm:wasm-function[3]:0x40",
      abs_path: "https://cdn.example.com/thirdparty.wasm:wasm-function[3]:0x40",
      function: "?",
    },
  ]);
  const out = await symbolicate(ev);
  assert.strictEqual(calls.length, 0);
  assert.strictEqual(out.fingerprint, undefined);
});
