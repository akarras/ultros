// Sentry `beforeSend` wasm symbolicator for the Ultros browser client.
//
// Browsers list wasm frames as `.../pkg/<hash>/<module>.wasm:wasm-function[N]:0x...`
// with no function name, because the shipped modules have no `name` section.
// `N` is that module's function index, and the Docker build writes a sibling
// `<module>.symbols` (`index:name` per line, see the `wasm-symbols` crate)
// from that very section before stripping it. `<module>` is `ultros` or, for
// a lazy route built by `cargo leptos build --split`, one of the ~100
// `chunk_N` modules — indices are per-module, so each frame is resolved
// against the map of the module it names. GlitchTip cannot
// symbolicate wasm itself, so this hook does the lookup in the browser:
// fetch the map, fill `frame.function`, mark our own crates `in_app`, and
// trim the panic machinery off the top so the first frame is the site.
//
// Production wasm is built with `panic = "immediate-abort"` (Dockerfile):
// a panic runs no hook and formats no message, it executes the wasm
// `unreachable` instruction on the spot, and the browser's global onerror
// reports it as "RuntimeError: unreachable". That event is the whole panic
// report, so after symbolication it also gets a fingerprint built from its
// top frames (its frame filenames carry `wasm-function[N]:0x…`, which
// changes every deploy — default grouping would open a new issue per
// release) and a value naming the site, so the issue list is readable.
//
// Like error_filter.js this is `include_str!`'d into lib.rs and injected
// verbatim. It defines `window.__ultrosSymbolicateEvent(event)`, which
// returns a Promise for the (possibly mutated) event — Sentry's `beforeSend`
// accepts a promise. It reaches fetch / AbortController / timers through
// `window` so `integration/wasm-symbolicate.test.cjs` can run it with a fake
// window. Nothing here may throw or reject: a symbolication failure must
// send the event exactly as it arrived, never lose it.
(function () {
  // `<module url>:wasm-function[<index>]...` for any module in our pkg dir
  // (`ultros.wasm` or a `--split` `chunk_N.wasm`). Group 1 is the module URL
  // the map is derived from — taken from the frame rather than the SDK
  // release so a tab that outlived a deploy can never fetch the wrong map,
  // and scoped to `/pkg/<hash>/` so a third-party wasm is never fetched for.
  var WASM_FRAME = /^(.*\/pkg\/[^/]+\/[^/:]+\.wasm):wasm-function\[(\d+)\]/;
  // Our crates, whether the name is a plain path (`ultros_app::x::f`) or a
  // trait impl (`<ultros_app::x::T as Trait>::f`, sometimes `<<…`).
  var IN_APP = /^<*(ultros|xiv_gen)/;
  // V8 "unreachable", SpiderMonkey "unreachable executed", JSC "Unreachable
  // code should not be executed".
  var TRAP_VALUE = /unreachable/i;
  var TRAP_FINGERPRINT_DEPTH = 3;
  var FETCH_TIMEOUT_MS = 10000;
  // Frames above the panicking site: Rust's panic runtime, our hook, the
  // Error() capture and the wasm-bindgen glue that performs it.
  var MACHINERY = [
    "std::panicking::",
    "core::panicking::",
    "std::sys::backtrace::",
    "rust_panic",
    "rust_begin_unwind",
    "__rust_start_panic",
    "console_error_panic_hook::",
    "ultros_client::set_panic_hook",
    "ultros_client::report_rust_panic",
    "ultros_client::error_stack",
  ];

  // module URL -> Promise<string | null>. Failures are memoized as null so a
  // crash loop cannot hammer the server for a map that is not there.
  var maps = {};

  function fetchMap(url) {
    if (maps[url]) return maps[url];
    var p;
    try {
      var controller = new window.AbortController();
      var timer = window.setTimeout(function () {
        controller.abort();
      }, FETCH_TIMEOUT_MS);
      p = window
        .fetch(url, { signal: controller.signal, credentials: "omit" })
        .then(function (res) {
          if (!res.ok) return null;
          return res.text();
        })
        .then(
          function (text) {
            window.clearTimeout(timer);
            return typeof text === "string" ? text : null;
          },
          function () {
            window.clearTimeout(timer);
            return null;
          },
        );
    } catch (_) {
      p = Promise.resolve(null);
    }
    maps[url] = p;
    return p;
  }

  // `.../<module>.wasm` -> `.../<module>.symbols`, the sibling map
  // `wasm-symbols` wrote for that module.
  function mapUrl(moduleUrl) {
    return moduleUrl.replace(/\.wasm$/, ".symbols");
  }

  function isWasmFrame(frame) {
    var target = frame && (frame.abs_path || frame.filename);
    if (typeof target !== "string") return null;
    var m = WASM_FRAME.exec(target);
    if (!m) return null;
    var fn = frame.function;
    if (typeof fn === "string" && fn !== "" && fn !== "?") return null;
    return { url: m[1], index: m[2] };
  }

  function stacktraces(event) {
    var out = [];
    var values = event && event.exception && event.exception.values;
    if (!Array.isArray(values)) return out;
    for (var i = 0; i < values.length; i++) {
      var frames =
        values[i] && values[i].stacktrace && values[i].stacktrace.frames;
      if (Array.isArray(frames) && frames.length) out.push(frames);
    }
    return out;
  }

  // Pull just the wanted indices out of the map text. The file is large
  // (one line per function in the module) and an event needs a dozen.
  function lookup(text, wanted) {
    var names = {};
    var start = 0;
    while (start < text.length) {
      var nl = text.indexOf("\n", start);
      if (nl === -1) nl = text.length;
      var colon = text.indexOf(":", start);
      if (colon !== -1 && colon < nl) {
        var index = text.substring(start, colon);
        if (wanted[index]) names[index] = text.substring(colon + 1, nl);
      }
      start = nl + 1;
    }
    return names;
  }

  function isMachinery(frame) {
    var target = frame && (frame.abs_path || frame.filename);
    if (typeof target === "string" && /\/ultros\.js(?::|$)/.test(target)) {
      return true;
    }
    var fn = frame && frame.function;
    if (typeof fn !== "string") return false;
    for (var i = 0; i < MACHINERY.length; i++) {
      if (fn.indexOf(MACHINERY[i]) === 0) return true;
    }
    return false;
  }

  // Sentry stores frames oldest-first, so the top of the stack is the end
  // of the array. Never empty the stack: an all-machinery trace still says
  // more than nothing.
  function trim(frames) {
    while (frames.length > 1 && isMachinery(frames[frames.length - 1])) {
      frames.pop();
    }
  }

  // `names` is {module url: {index: name}} — a frame is only ever resolved
  // against the map of the module it names, because indices are per-module.
  // Returns how many frames were resolved.
  function apply(frames, names) {
    var resolved = 0;
    for (var i = 0; i < frames.length; i++) {
      var w = isWasmFrame(frames[i]);
      if (!w) continue;
      var module = names[w.url];
      if (!module) continue;
      var name = module[w.index];
      if (typeof name !== "string") continue;
      frames[i].function = name;
      frames[i].in_app = IN_APP.test(name);
      resolved++;
    }
    // Only touch the stack when this trace actually gained names. A map that
    // failed to load must leave its event byte-for-byte as it arrived.
    if (resolved) trim(frames);
    return resolved;
  }

  // A name without its generic arguments: `a::b<T, U>::f::{closure#3}` ->
  // `a::b::f::{closure#3}`. Trait-impl names (`<T as Trait>::f`) are kept
  // whole — their angle brackets are structure, not arguments. Used for the
  // trap's fingerprint and title only (the frames keep their full names), so
  // one bug in generic code is one issue, not one per instantiation.
  function shortName(name) {
    if (name.charAt(0) === "<") return name;
    var out = "";
    var depth = 0;
    for (var i = 0; i < name.length; i++) {
      var c = name.charAt(i);
      if (c === "<") {
        depth++;
      } else if (c === ">" && depth > 0) {
        depth--;
      } else if (depth === 0) {
        out += c;
      }
    }
    return out;
  }

  function isTrap(event) {
    var ex = event.exception.values[0];
    return (
      ex &&
      ex.type === "RuntimeError" &&
      typeof ex.value === "string" &&
      TRAP_VALUE.test(ex.value)
    );
  }

  // Fingerprint a symbolicated trap by its top frames (newest first) and
  // name the site in the value. Only when something resolved: an unresolved
  // trap keeps default grouping rather than collapsing every panic into one
  // "rust-wasm-trap" issue. An explicit fingerprint set upstream wins.
  function labelTrap(event) {
    var ex = event.exception.values[0];
    var frames = ex.stacktrace.frames;
    var names = [];
    var site = null;
    for (var i = frames.length - 1; i >= 0; i--) {
      var fn = frames[i] && frames[i].function;
      if (typeof fn !== "string" || fn === "" || fn === "?") continue;
      fn = shortName(fn);
      if (names.length < TRAP_FINGERPRINT_DEPTH) names.push(fn);
      if (!site && frames[i].in_app === true) site = fn;
    }
    if (!names.length) return;
    if (!Array.isArray(event.fingerprint)) {
      event.fingerprint = ["rust-wasm-trap"].concat(names);
    }
    ex.value = ex.value + " in " + (site || names[0]);
  }

  window.__ultrosSymbolicateEvent = function (event) {
    try {
      var traces = stacktraces(event);
      // module URL -> set of function indices wanted from THAT module.
      var wanted = {};
      var urls = [];
      for (var t = 0; t < traces.length; t++) {
        for (var i = 0; i < traces[t].length; i++) {
          var w = isWasmFrame(traces[t][i]);
          if (!w) continue;
          if (!wanted[w.url]) {
            wanted[w.url] = {};
            urls.push(w.url);
          }
          wanted[w.url][w.index] = true;
        }
      }
      if (!urls.length) return Promise.resolve(event);
      // One fetch per module (memoized across events). A module whose map is
      // missing resolves to null and simply leaves its own frames alone.
      var fetches = [];
      for (var u = 0; u < urls.length; u++) {
        fetches.push(
          fetchMap(mapUrl(urls[u])).then(
            function (text) {
              return text;
            },
            function () {
              return null;
            },
          ),
        );
      }
      return Promise.all(fetches).then(
        function (texts) {
          try {
            var names = {};
            for (var u = 0; u < urls.length; u++) {
              if (typeof texts[u] !== "string") continue;
              names[urls[u]] = lookup(texts[u], wanted[urls[u]]);
            }
            var resolved = 0;
            for (var t = 0; t < traces.length; t++) {
              resolved += apply(traces[t], names);
            }
            if (resolved && isTrap(event)) labelTrap(event);
          } catch (_) {}
          return event;
        },
        function () {
          return event;
        },
      );
    } catch (_) {
      return Promise.resolve(event);
    }
  };
})();
