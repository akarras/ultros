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
].join("\n");

const WASM = "https://ultros.app/pkg/9b93be1/ultros.wasm";

// A `--split` build ships route modules and shared chunks next to the main
// module; each gets its own `.symbols` sibling from `wasm-symbols`.
const SPLIT_SYMBOLS = [
  "3:ultros_app::routes::analyzer::AnalyzerWorld::{{closure}}",
  "5:ultros_app::components::virtual_grid::VirtualGrid::render",
].join("\n");
const CHUNK_SYMBOLS = [
  "0:alloc::raw_vec::RawVec<T>::grow_one",
  // v0 demangling (what a `--split` build yields) writes trait impls as
  // `<Type as Trait>::method`; our crates still count as in_app there.
  "1:<ultros_api_types::listings::ActiveListing as core::clone::Clone>::clone",
  "2:<alloc::vec::Vec<ultros_app::global_state::LocalWorldData> as core::clone::Clone>::clone",
].join("\n");

function wasmFrame(index, name, module = "ultros.wasm") {
  const filename = `/pkg/9b93be1/${module}:wasm-function[${index}]:0xca0ed2`;
  const f = {
    filename,
    abs_path: `https://ultros.app/pkg/9b93be1/${module}:wasm-function[${index}]:0xca0ed2`,
  };
  if (name !== undefined) f.function = name;
  else f.function = "?";
  return f;
}

function splitFrame(index) {
  return wasmFrame(index, undefined, "split___analyzer_world.wasm");
}

function chunkFrame(index) {
  return wasmFrame(index, undefined, "chunk_17.wasm");
}

// Scripted fetch for a split bundle: every module URL answers with its own
// map so a test can prove frames were resolved against the right one.
function splitBundle(url) {
  if (url.endsWith("/ultros.symbols")) return SYMBOLS;
  if (url.endsWith("/split___analyzer_world.symbols")) return SPLIT_SYMBOLS;
  if (url.endsWith("/chunk_17.symbols")) return CHUNK_SYMBOLS;
  return 404;
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

test("resolves split route and chunk frames from their own maps", async () => {
  const { symbolicate, calls } = loadSymbolicator(splitBundle);
  const ev = panicEvent();
  // A panic inside a lazily loaded analyzer route: the shared chunk called
  // from the route module, called from the main module.
  frames(ev).splice(2, 0, splitFrame(3), splitFrame(5), chunkFrame(0));
  const out = await symbolicate(ev);
  assert.deepStrictEqual(calls.slice().sort(), [
    "https://ultros.app/pkg/9b93be1/chunk_17.symbols",
    "https://ultros.app/pkg/9b93be1/split___analyzer_world.symbols",
    "https://ultros.app/pkg/9b93be1/ultros.symbols",
  ]);
  const fns = frames(out).map((f) => [f.function, f.in_app]);
  assert.deepStrictEqual(fns, [
    ["__wbg_adapter_50", undefined],
    ["leptos::callback::Callback::run", false],
    ["ultros_app::routes::analyzer::AnalyzerWorld::{{closure}}", true],
    ["ultros_app::components::virtual_grid::VirtualGrid::render", true],
    ["alloc::raw_vec::RawVec<T>::grow_one", false],
    ["ultros_app::routes::item_view::ItemView::{{closure}}", true],
    ["core::option::unwrap_failed", false],
  ]);
});

test("v0 trait-impl forms of the panic machinery are trimmed too", async () => {
  // Real entries from a `--split` build's ultros.symbols: the runtime's
  // payload impls and our hook's FnOnce impl render as `<X as Trait>::m`.
  const v0 = [
    "7:<std::panicking::begin_panic::Payload<&str> as alloc::panicking::PanicPayload>::get",
    "9:core::panicking::panic_fmt",
    "12:<ultros_client::set_panic_hook::{closure#0} as core::ops::function::FnOnce<(&std::panic::PanicHookInfo,)>>::call_once",
    "13:console_error_panic_hook::hook",
    "20:core::option::unwrap_failed",
    "42:ultros_app::routes::item_view::__component_item_view_content::{closure#10}",
    "43:leptos::callback::Callback::run",
  ].join("\n");
  const { symbolicate } = loadSymbolicator(() => v0);
  const out = await symbolicate(panicEvent());
  assert.deepStrictEqual(
    frames(out).map((f) => f.function),
    [
      "__wbg_adapter_50",
      "leptos::callback::Callback::run",
      "ultros_app::routes::item_view::__component_item_view_content::{closure#10}",
      "core::option::unwrap_failed",
    ],
  );
});

test("v0 trait-impl paths of our crates are in_app", async () => {
  const { symbolicate } = loadSymbolicator(splitBundle);
  const ev = {
    exception: {
      values: [
        {
          type: "RustWasmPanic",
          value: "x",
          stacktrace: { frames: [chunkFrame(0), chunkFrame(1), chunkFrame(2)] },
        },
      ],
    },
  };
  const out = await symbolicate(ev);
  assert.deepStrictEqual(
    frames(out).map((f) => f.in_app),
    // Vec<LocalWorldData>::clone is alloc's code, not ours.
    [false, true, false],
  );
});

test("the same index in two modules resolves to two different names", async () => {
  const { symbolicate } = loadSymbolicator(splitBundle);
  const ev = {
    exception: {
      values: [
        {
          type: "RustWasmPanic",
          value: "x",
          stacktrace: { frames: [wasmFrame(9), splitFrame(3)] },
        },
      ],
    },
  };
  const out = await symbolicate(ev);
  assert.deepStrictEqual(
    frames(out).map((f) => f.function),
    [
      "core::panicking::panic_fmt",
      "ultros_app::routes::analyzer::AnalyzerWorld::{{closure}}",
    ],
  );
});

test("maps are cached per module URL", async () => {
  const { symbolicate, calls } = loadSymbolicator(splitBundle);
  const ev = () => {
    const e = panicEvent();
    frames(e).splice(2, 0, splitFrame(3), chunkFrame(0));
    return e;
  };
  await symbolicate(ev());
  await symbolicate(ev());
  // A third event touching only the chunk still fetches nothing new.
  await symbolicate({
    exception: {
      values: [{ type: "RustWasmPanic", value: "y", stacktrace: { frames: [chunkFrame(0)] } }],
    },
  });
  assert.strictEqual(calls.length, 3);
  assert.strictEqual(new Set(calls).size, 3);
});

test("a missing chunk map leaves only that module's frames unresolved", async () => {
  const { symbolicate } = loadSymbolicator((url) =>
    url.endsWith("/chunk_17.symbols") ? 404 : splitBundle(url),
  );
  const ev = panicEvent();
  frames(ev).splice(2, 0, splitFrame(3), chunkFrame(0));
  const out = await symbolicate(ev);
  assert.deepStrictEqual(
    frames(out).map((f) => f.function),
    [
      "__wbg_adapter_50",
      "leptos::callback::Callback::run",
      "ultros_app::routes::analyzer::AnalyzerWorld::{{closure}}",
      "?",
      "ultros_app::routes::item_view::ItemView::{{closure}}",
      "core::option::unwrap_failed",
    ],
  );
});

test("a wasm URL outside /pkg/<hash>/ is not a frame we own", async () => {
  const { symbolicate, calls } = loadSymbolicator(splitBundle);
  const ev = {
    exception: {
      values: [
        {
          type: "RuntimeError",
          value: "x",
          stacktrace: {
            frames: [
              {
                filename: "https://cdn.example/other/thing.wasm:wasm-function[3]:0x1",
                abs_path: "https://cdn.example/other/thing.wasm:wasm-function[3]:0x1",
                function: "?",
              },
            ],
          },
        },
      ],
    },
  };
  const out = await symbolicate(ev);
  assert.strictEqual(calls.length, 0);
  assert.strictEqual(frames(out)[0].function, "?");
});

test("a throwing fetch implementation cannot break the event", async () => {
  const { symbolicate } = loadSymbolicator(() => {
    throw new Error("boom");
  });
  const out = await symbolicate(panicEvent());
  assert.deepStrictEqual(out, panicEvent());
});
