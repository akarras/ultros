// Sentry `beforeSend` wasm symbolicator for the Ultros browser client.
//
// Browsers list wasm frames as `.../pkg/<hash>/<module>.wasm:wasm-function[N]:0x...`
// with no function name, because the shipped modules have no `name` section.
// `N` is a function index *within that module*, and the Docker build writes
// a sibling `/pkg/<hash>/<module>.symbols` (`index:name` per line, see the
// `wasm-symbols` crate) for every module from that very section before
// stripping it. A `--split` build has several: `ultros.wasm` plus the
// `split___*.wasm` route modules and `chunk_N.wasm` shared modules a lazy
// route pulls in, so one panic stack can span three maps. GlitchTip cannot
// symbolicate wasm itself, so this hook does the lookup in the browser:
// fetch each module's map, fill `frame.function`, mark our own crates
// `in_app`, and trim the panic machinery off the top so the first frame is
// the site.
//
// Like error_filter.js this is `include_str!`'d into lib.rs and injected
// verbatim. It defines `window.__ultrosSymbolicateEvent(event)`, which
// returns a Promise for the (possibly mutated) event — Sentry's `beforeSend`
// accepts a promise. It reaches fetch / AbortController / timers through
// `window` so `integration/wasm-symbolicate.test.cjs` can run it with a fake
// window. Nothing here may throw or reject: a symbolication failure must
// send the event exactly as it arrived, never lose it.
(function () {
  // `<module url>:wasm-function[<index>]...`. Group 1 is the module URL the
  // map is derived from — taken from the frame rather than the SDK release
  // so a tab that outlived a deploy can never fetch the wrong map. Only
  // modules under our `/pkg/<hash>/` dir count; anything else (an extension,
  // a third-party wasm) has no map to fetch.
  var WASM_FRAME = /^(.*\/pkg\/[^/]+\/[^/:]+\.wasm):wasm-function\[(\d+)\]/;
  // Our crates, whether the path starts with them or is a v0-demangled
  // trait impl (`<ultros_api_types::X as core::clone::Clone>::clone`). A
  // foreign impl merely *generic over* one of our types (`<alloc::vec::Vec<
  // ultros_app::T> as Clone>::clone`) is not ours: the `<` must be at the
  // very start of the name.
  var IN_APP = /^<?(ultros|xiv_gen)/;
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

  // symbols URL -> Promise<string | null>, one entry per module. Failures
  // are memoized as null so a crash loop cannot hammer the server for a map
  // that is not there.
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
    // v0 demangling writes trait impls as `<std::panicking::X as T>::m`;
    // the path we prefix-match starts after the `<`.
    if (fn.charAt(0) === "<") fn = fn.substring(1);
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

  // `namesByModule` is module URL -> { index -> name }; a module whose map
  // failed to load is simply absent and its frames keep their placeholder.
  function apply(frames, namesByModule) {
    for (var i = 0; i < frames.length; i++) {
      var w = isWasmFrame(frames[i]);
      if (!w) continue;
      var names = namesByModule[w.url];
      var name = names && names[w.index];
      if (typeof name !== "string") continue;
      frames[i].function = name;
      frames[i].in_app = IN_APP.test(name);
    }
    trim(frames);
  }

  function symbolsUrl(moduleUrl) {
    return moduleUrl.replace(/\.wasm$/, ".symbols");
  }

  window.__ultrosSymbolicateEvent = function (event) {
    try {
      var traces = stacktraces(event);
      // module URL -> { index -> true }. Indices are per module, so a
      // `wasm-function[3]` in a chunk and in the main module are unrelated.
      var wantedByModule = {};
      var moduleUrls = [];
      for (var t = 0; t < traces.length; t++) {
        for (var i = 0; i < traces[t].length; i++) {
          var w = isWasmFrame(traces[t][i]);
          if (!w) continue;
          if (!wantedByModule[w.url]) {
            wantedByModule[w.url] = {};
            moduleUrls.push(w.url);
          }
          wantedByModule[w.url][w.index] = true;
        }
      }
      if (moduleUrls.length === 0) return Promise.resolve(event);
      var fetches = [];
      for (var m = 0; m < moduleUrls.length; m++) {
        fetches.push(fetchMap(symbolsUrl(moduleUrls[m])));
      }
      return Promise.all(fetches).then(
        function (texts) {
          try {
            var namesByModule = {};
            var loaded = 0;
            for (var m = 0; m < moduleUrls.length; m++) {
              if (texts[m] === null) continue;
              namesByModule[moduleUrls[m]] = lookup(
                texts[m],
                wantedByModule[moduleUrls[m]],
              );
              loaded++;
            }
            // No map at all: send the event exactly as it arrived (no
            // trimming either — an unsymbolicated stack is better whole).
            if (loaded === 0) return event;
            for (var t = 0; t < traces.length; t++) {
              apply(traces[t], namesByModule);
            }
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
