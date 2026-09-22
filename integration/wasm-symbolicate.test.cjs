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

function wasmFrame(index, name) {
  const filename = `/pkg/9b93be1/ultros.wasm:wasm-function[${index}]:0xca0ed2`;
  const f = { filename, abs_path: `${WASM}:wasm-function[${index}]:0xca0ed2` };
  if (name !== undefined) f.function = name;
  else f.function = "?";
  return f;
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

test("a throwing fetch implementation cannot break the event", async () => {
  const { symbolicate } = loadSymbolicator(() => {
    throw new Error("boom");
  });
  const out = await symbolicate(panicEvent());
  assert.deepStrictEqual(out, panicEvent());
});
