// Sentry `beforeSend` noise filter for the Ultros browser client.
//
// Extracted into its own file (instead of living inline in the
// `error_reporting_script` format! string in lib.rs) so it can be unit
// tested with Node without standing up a browser — see
// `integration/error-filter.test.cjs`. The file is `include_str!`'d back
// into lib.rs and injected verbatim ahead of the Sentry `init`, where it
// defines `window.__ultrosShouldDropEvent(event)`.
//
// It references `window` and `navigator` as free identifiers: in the
// browser these are the globals; the test wraps the source in
// `new Function('window', 'navigator', src)` so the same code runs with
// injected stand-ins. Every predicate is wrapped in try/catch — a noise
// filter must NEVER throw, or it would drop (or duplicate) real events.
//
// We drop several independent categories of unactionable noise:
//
//   1. WASM bundle fetch / compile failures — users navigating away during
//      the streaming compile, ad blockers, corporate proxies, and stale
//      chunks 404ing in the seconds after a deploy. Shapes: a bare
//      "TypeError: Failed to fetch" from the wasm-bindgen glue; the
//      ESM-entry "Failed to fetch dynamically imported module: <pkg-url>"
//      thrown when the module preload/import() of our own bundle aborts;
//      "WebAssembly compilation aborted"; "TypeError: Failed to execute
//      'compile' on 'WebAssembly': HTTP status code is not ok" (the .wasm
//      came back non-OK); and "CompileError: ... extends past end of the
//      module" (a truncated download). GlitchTip issues #21, #2374, #2404,
//      #6755, #6762–#6767, and the count=1 flood of #62xx "Failed to fetch
//      dynamically imported module: .../pkg/<hash>/ultros.js".
//   2. A "Cannot read properties of undefined (reading 'document')"
//      TypeError thrown by an injected third-party script (Tencent QQ
//      Browser / UC / WeChat in-app WebViews on frozen Chrome 112).
//      GlitchTip issues #1, #7, #313, #770, #1047, #2776–#2812.
//   3. tachys hydration `unreachable!()` panics at
//      /tachys-*/src/hydration.rs:* triggered when an injected
//      auto-translation overlay wraps text nodes in <font> elements
//      before Leptos hydrates (see shell() in lib.rs for the full
//      chain). Three event shapes share this one root: the handled
//      `internal error: entered unreachable code` panic (tachys path in
//      contexts.rust_panic), its `RefCell already borrowed` cascade in
//      the wasm-bindgen-futures executor (a js-sys path — matched via a
//      tachys hydration breadcrumb instead), and the unhandled
//      `RuntimeError: unreachable` that reaches window.onerror with no
//      rust_panic context at all. Suppressed only when an injecting /
//      stale-population fingerprint is present: a <font> element in the
//      live DOM (which Ultros never emits, so it is necessarily
//      translation-injected), the full-page-translation class on <html>
//      (translated-ltr / translated-rtl, added by Google / Chrome
//      translate regardless of wrapper element or Chrome version), the
//      page-stability breadcrumb, or an implausibly stale Chrome major
//      (<= 124 — stuck in-app WebViews and version-pinned crawler fleets,
//      never self-updating real users; spans the 108/111/112/120 flood).
//      Genuine hydration mismatches on a clean, current browser have none
//      of these and still reach GlitchTip. Issues #4, #678, #707, #770,
//      #1307, #2277, #2775, #3005, #4905, #4911, #6406, #6456 and the
//      per-URL #65xx /item/<world>/<id> cluster.
//   4. "Non-Error promise rejection captured with value: undefined"
//      (and the null variant) — Sentry's synthetic wrapper for a promise
//      rejected with no reason. Zero diagnostic value (no message, no
//      stack), overwhelmingly third-party (gtag / funding-choices / ads)
//      or aborted fetches. The count=1 flood of #62xx "UnhandledRejection".
//   5. "ReferenceError: __RESOLVED_RESOURCES is not defined" (and the
//      __INCOMPLETE_CHUNKS / __PENDING_RESOURCES / __SERIALIZED_ERRORS
//      variants) — leptos's streaming-hydration bootstrap globals, which the
//      SSR shell ALWAYS emits and only leptos's generated wasm-bindgen static
//      accessors ever read. "not defined" means a proxy/crawler stripped or
//      truncated the streamed bootstrap before the wasm hydrated — the same
//      translation-proxy population behind category 3. GlitchTip issues
//      #6620, #6667, #6760, #6761.
//   6. The redundant "RuntimeError: unreachable" that every Rust panic
//      emits a SECOND time. The panic hook first reports an actionable
//      RustWasmPanic (kept — it carries contexts.rust_panic.location and a
//      stable per-location fingerprint, so it collapses to one issue per
//      panic site). Then Rust's abort() runs the wasm `unreachable`
//      instruction, whose trap the browser's global onerror re-captures as
//      a "RuntimeError: unreachable" with NO rust_panic context. That copy
//      never gets the stable fingerprint, and its
//      /pkg/<hash>/ultros.wasm:wasm-function[N] frame filename fragments it
//      into a NEW issue every deploy — the per-build #67xx/#68xx rotation
//      (#6781–#6828) that prior triage had to ignore by hand each release.
//      When the trap's stack carries one of our own pkg-bundle frames it is
//      provably our wasm, hence a guaranteed duplicate of the kept
//      RustWasmPanic, so it is dropped unconditionally — no injecting-
//      population fingerprint needed: a real hydration bug on a current
//      browser still reaches GlitchTip via the untouched RustWasmPanic. A
//      frameless or third-party-framed RuntimeError is left untouched.
(function () {
  var ULTROS_PKG_BUNDLE_RE = /\/pkg\/[a-f0-9]+\/ultros\.(?:js|wasm)(?:$|\?)/;
  // Like ULTROS_PKG_BUNDLE_RE but tolerant of the trailing
  // `:wasm-function[N]:0xADDR` the browser appends to a wasm trap's stack
  // frame, so `/pkg/<hash>/ultros.wasm:wasm-function[5501]` still counts as
  // originating in our bundle.
  var ULTROS_PKG_FRAME_RE = /\/pkg\/[a-f0-9]+\/ultros\.(?:js|wasm)\b/;
  var ULTROS_TACHYS_HYDRATION_RE = /\/tachys-[\d.]+\/src\/hydration\.rs:/;
  // Leptos's streaming-hydration bootstrap globals. The SSR shell ALWAYS emits
  // `window.__RESOLVED_RESOURCES = []` plus `__INCOMPLETE_CHUNKS` /
  // `__PENDING_RESOURCES` / `__SERIALIZED_ERRORS` (verified in the served HTML
  // of the failing /item/<world>/<id> URLs — see shell() in lib.rs), and they
  // are read ONLY by leptos's generated wasm-bindgen static accessors, never by
  // app code. So a "ReferenceError: <name> is not defined" for one of them can
  // only mean a proxy/crawler stripped or truncated the streamed bootstrap
  // before the wasm hydrated — the same translation-proxy population behind the
  // tachys flood. Anchored ^…$ so it matches the bare global name only.
  var ULTROS_HYDRATION_BOOTSTRAP_REFERR_RE =
    /^__(?:RESOLVED_RESOURCES|INCOMPLETE_CHUNKS|PENDING_RESOURCES|SERIALIZED_ERRORS) is not defined$/;
  // The wasm-bindgen-futures single-threaded executor. When the tachys
  // hydration panic unwinds through a running future poll, re-entering the
  // executor trips `RefCell already borrowed` HERE — the documented cascade of
  // the hydration panic (see shell() in lib.rs). `__ultrosReportRustPanic` sets
  // this exact path in contexts.rust_panic, so — unlike the tachys console
  // breadcrumb — it is reliably present on the event at client-side beforeSend.
  var ULTROS_JSSYS_EXECUTOR_RE =
    /\/js-sys-[\d.]+\/src\/futures\/task\/singlethread\.rs:/;
  var ULTROS_INJECTOR_BREADCRUMB = "检测页面稳定";
  // The stale-Chrome population behind the hydration flood: stuck in-app
  // WebViews (Tencent QQ / UC / WeChat, frozen near Chrome 112) and
  // version-pinned crawler fleets, none of them self-updating real users.
  // Chrome ships ~10 majors/year; current Chrome in mid-2026 is ~138, so any
  // major at or below this is well over a year stale. The observed flood spans
  // Chrome 108/111/112/120 (GlitchTip #4, #6456, #5918/#5919, #4936, #224/
  // #5392 and the per-URL /item/<world>/<id> #65xx cluster) — all comfortably
  // below — while real users sit at 130+. PR #764 only matched the single
  // version `Chrome/112.`, so 108/111/120 leaked through. This is consulted
  // ONLY for a recognized tachys hydration panic, so a genuine clean-page
  // mismatch on a current browser still reaches GlitchTip.
  var ULTROS_STALE_CHROME_MAX_MAJOR = 124;
  // Chrome major from a UA string ("…Chrome/120.0.0.0…") or a GlitchTip
  // `browser` tag ("Chrome 120.0.0"). Returns 0 when not Chrome/unknown.
  var ULTROS_CHROME_MAJOR_RE = /\bChrome[/ ](\d+)\./;

  // Live User-Agent. Read from `navigator` because the `browser`/`os` tags
  // shown in GlitchTip are derived SERVER-SIDE from the request UA header
  // and are NOT present on the event during client-side beforeSend.
  function userAgent() {
    try {
      return (typeof navigator !== "undefined" && navigator.userAgent) || "";
    } catch (_) {
      return "";
    }
  }

  function chromeMajor(str) {
    try {
      var m = ULTROS_CHROME_MAJOR_RE.exec(str || "");
      return m ? parseInt(m[1], 10) : 0;
    } catch (_) {
      return 0;
    }
  }

  function isStaleChromeMajor(major) {
    return major > 0 && major <= ULTROS_STALE_CHROME_MAX_MAJOR;
  }

  function firstException(event) {
    return (
      event &&
      event.exception &&
      event.exception.values &&
      event.exception.values[0]
    );
  }

  function isUltrosWasmFetchAbort(event) {
    try {
      var ex = firstException(event);
      if (!ex) return false;
      if (ex.type !== "TypeError" || typeof ex.value !== "string") return false;

      // "WebAssembly compilation aborted: ..." — always network/abort.
      if (ex.value.indexOf("WebAssembly compilation aborted") === 0) {
        return true;
      }

      // "TypeError: Failed to fetch" originating from the wasm-bindgen
      // glue loading /pkg/<hash>/ultros.{js,wasm}.
      if (ex.value === "Failed to fetch") {
        var frames = (ex.stacktrace && ex.stacktrace.frames) || [];
        for (var i = 0; i < frames.length; i++) {
          var fname = frames[i] && frames[i].filename;
          if (typeof fname === "string" && ULTROS_PKG_BUNDLE_RE.test(fname)) {
            return true;
          }
        }
      }

      // "Failed to fetch dynamically imported module: <url>" — the ESM
      // entry import() / modulepreload of our own bundle aborting. Unlike
      // the bare "Failed to fetch" above, the pkg URL is carried in the
      // message itself rather than a stack frame, so match the regex
      // against the value. Scoped to /pkg/<hash>/ultros.{js,wasm} so a
      // failed import of a genuine third-party module still reports.
      if (
        ex.value.indexOf("Failed to fetch dynamically imported module") === 0 &&
        ULTROS_PKG_BUNDLE_RE.test(ex.value)
      ) {
        return true;
      }
    } catch (_) {
      /* be defensive — never let the filter throw */
    }
    return false;
  }

  // Category 1 (cont.): the streaming COMPILE of our wasm bundle failing. Two
  // shapes, both network / deploy-race noise rather than an actionable bug:
  //   - a non-OK HTTP response for the .wasm — a stale chunk 404 in the seconds
  //     after a deploy — surfaces as `TypeError: Failed to execute 'compile' on
  //     'WebAssembly': HTTP status code is not ok`;
  //   - a truncated / aborted download surfaces as `CompileError:
  //     WebAssembly.instantiateStreaming(): section (...) extends past end of
  //     the module (...)`.
  // The CompileError match is scoped to the truncation signature so a genuinely
  // corrupt build (e.g. a bad opcode) still reports — that would be an
  // all-users flood worth seeing, not these count=1 blips. GlitchTip #6755,
  // #6762, #6763, #6764, #6766, #6767.
  function isUltrosWasmCompileFailure(event) {
    try {
      var ex = firstException(event);
      if (!ex || typeof ex.value !== "string") return false;
      if (
        ex.type === "TypeError" &&
        ex.value.indexOf(
          "Failed to execute 'compile' on 'WebAssembly': HTTP status code is not ok",
        ) === 0
      ) {
        return true;
      }
      if (
        ex.type === "CompileError" &&
        ex.value.indexOf("extends past end of the module") !== -1
      ) {
        return true;
      }
    } catch (_) {
      /* never let the filter throw */
    }
    return false;
  }

  // Category 5: a hydration accessor reading a leptos bootstrap global that the
  // page never defined (see ULTROS_HYDRATION_BOOTSTRAP_REFERR_RE). Scoped to our
  // own bundle so a same-named global thrown by some third-party script could
  // never be swept up: when stack frames are present, require one from the pkg
  // bundle or a `__wbg_static_accessor` frame; absent any frames the value alone
  // — a leptos-internal name no app code references — is already definitive.
  function isStrippedHydrationBootstrap(event) {
    try {
      var ex = firstException(event);
      if (!ex || ex.type !== "ReferenceError" || typeof ex.value !== "string") {
        return false;
      }
      if (!ULTROS_HYDRATION_BOOTSTRAP_REFERR_RE.test(ex.value)) return false;
      var frames = (ex.stacktrace && ex.stacktrace.frames) || [];
      if (frames.length === 0) return true;
      for (var i = 0; i < frames.length; i++) {
        var f = frames[i] || {};
        if (
          typeof f.filename === "string" &&
          ULTROS_PKG_BUNDLE_RE.test(f.filename)
        ) {
          return true;
        }
        if (
          typeof f.function === "string" &&
          f.function.indexOf("__wbg_static_accessor") === 0
        ) {
          return true;
        }
      }
    } catch (_) {
      /* never let the filter throw */
    }
    return false;
  }

  function isInjectedDocumentTypeError(event) {
    try {
      var ex = firstException(event);
      if (!ex) return false;
      if (ex.type !== "TypeError") return false;
      if (ex.value !== "Cannot read properties of undefined (reading 'document')")
        return false;
      var frames = (ex.stacktrace && ex.stacktrace.frames) || [];
      if (frames.length !== 1) return false;
      return frames[0] && frames[0].function === "HTMLDocument.c";
    } catch (_) {
      /* never let the filter throw */
    }
    return false;
  }

  function breadcrumbList(event) {
    // The Sentry SDK passes in-flight breadcrumbs as a bare array; the
    // server/envelope shape nests them under `.values`. Handle both.
    return (
      (event && event.breadcrumbs && event.breadcrumbs.values) ||
      (event && event.breadcrumbs) ||
      []
    );
  }

  // Is this event the tachys hydration panic, in any of its shapes? One
  // page-load emits up to three Sentry events from this single root:
  //   - the handled `internal error: entered unreachable code` panic, whose
  //     contexts.rust_panic points at the tachys hydration path;
  //   - its `RefCell already borrowed` cascade, whose contexts.rust_panic
  //     points at the js-sys wasm-bindgen-futures executor;
  //   - the unhandled `RuntimeError: unreachable` wasm trap that reaches
  //     window.onerror with NO rust_panic context at all.
  //
  // Recognition must lean on signals reliably present at client-side
  // beforeSend. The `panicked at .../tachys-*/hydration.rs:` console breadcrumb
  // that the stored GlitchTip payload shows is NOT in the breadcrumb array the
  // SDK passes to beforeSend, so the breadcrumb scan (prong b) misfires there.
  // Proof from prod release e59476b: on a single stale-Chrome (<=124) load the
  // root panic was dropped (recognized via its tachys rust_panic location) yet
  // the SAME load's RefCell cascade leaked (GlitchTip #6661/#4908 with no paired
  // internal-error issue) — same UA, same fingerprint, so only recognition
  // differed. Prongs (a) and (c) below therefore key off event-level fields
  // (rust_panic.location, exception type/value); prong (b) stays as a best-
  // effort fallback for paths where the breadcrumb IS attached.
  function isTachysHydrationPanicEvent(event) {
    // (a) The root panic and its RefCell cascade both carry an explicit
    //     contexts.rust_panic location (set by __ultrosReportRustPanic): the
    //     tachys hydration path, or the js-sys futures executor it cascades to.
    var ctx = event && event.contexts && event.contexts.rust_panic;
    var loc = ctx && ctx.location;
    if (
      typeof loc === "string" &&
      loc.indexOf("/usr/local/cargo/registry/src/index.crates.io-") === 0 &&
      (ULTROS_TACHYS_HYDRATION_RE.test(loc) ||
        ULTROS_JSSYS_EXECUTOR_RE.test(loc))
    ) {
      return true;
    }
    // (c) The unhandled wasm trap at window.onerror has no rust_panic context;
    //     its only event-level signal is the exact RuntimeError "unreachable"
    //     value. (A genuine wasm `unreachable` on a clean current browser still
    //     reports — this is gated behind the injecting-population fingerprint in
    //     isInjectedTachysHydrationPanic.) Matched exactly so other RuntimeError
    //     values, e.g. "table index is out of bounds", are untouched.
    var ex = firstException(event);
    if (ex && ex.type === "RuntimeError" && ex.value === "unreachable") {
      return true;
    }
    // (b) Best-effort fallback: the original tachys hydration panic console
    //     breadcrumb, when the SDK path does attach it.
    var crumbs = breadcrumbList(event);
    if (Array.isArray(crumbs)) {
      for (var i = 0; i < crumbs.length; i++) {
        var msg = crumbs[i] && crumbs[i].message;
        if (typeof msg === "string" && ULTROS_TACHYS_HYDRATION_RE.test(msg)) {
          return true;
        }
      }
    }
    return false;
  }

  // Ultros never emits <font>: no `view!` produces it and item text is
  // escaped to text nodes, not elements. So any <font> in the live document
  // was injected by a translation overlay rewriting text nodes — the exact
  // mutation that shifts tachys' hydration cursor (see shell() in lib.rs).
  // This catches the modern, self-updating Chrome population (CN data-center
  // users reading the English UI) that ignores the notranslate trifecta and
  // so is missed by the frozen-Chrome-112 UA check. Ads and the
  // funding-choices consent dialog render in iframes, which
  // getElementsByTagName does not traverse, so they cannot match.
  function hasInjectedTranslationFont() {
    try {
      var doc = typeof window !== "undefined" && window.document;
      if (!doc || typeof doc.getElementsByTagName !== "function") return false;
      var fonts = doc.getElementsByTagName("font");
      return !!fonts && fonts.length > 0;
    } catch (_) {
      return false;
    }
  }

  // Google / Chrome built-in full-page translation tags <html> with
  // class="translated-ltr" (or "translated-rtl") when it rewrites the page.
  // That marker is added regardless of Chrome version OR the wrapper element
  // used, so it catches the translation population whose injector leaves no
  // <font> the snapshot above can see. Matched exactly (classList.contains, not
  // a substring scan) so the SSR-emitted "notranslate" class — which contains
  // the substring "translate" — is never mistaken for an active translation.
  function hasTranslatedHtmlClass() {
    try {
      var el =
        typeof window !== "undefined" &&
        window.document &&
        window.document.documentElement;
      if (!el || !el.classList || typeof el.classList.contains !== "function") {
        return false;
      }
      return (
        el.classList.contains("translated-ltr") ||
        el.classList.contains("translated-rtl")
      );
    } catch (_) {
      return false;
    }
  }

  function isInjectedTachysHydrationPanic(event) {
    try {
      if (!isTachysHydrationPanicEvent(event)) return false;

      // Only suppress when an injecting-population fingerprint is present, so
      // a genuine hydration mismatch on a clean page still reaches GlitchTip.

      // Page-stability detector breadcrumb (the injected overlay's own log).
      var crumbs = breadcrumbList(event);
      if (Array.isArray(crumbs)) {
        for (var i = 0; i < crumbs.length; i++) {
          var msg = crumbs[i] && crumbs[i].message;
          if (
            typeof msg === "string" &&
            msg.indexOf(ULTROS_INJECTOR_BREADCRUMB) !== -1
          ) {
            return true;
          }
        }
      }

      // Injected <font> in the live DOM — the precise translation mutation,
      // independent of which tool or browser injected it.
      if (hasInjectedTranslationFont()) return true;

      // Full-page translation class on <html> — the same translation overlay,
      // detected independently of the wrapper element (catches the current-
      // browser translation population whose injector leaves no <font>).
      if (hasTranslatedHtmlClass()) return true;

      // Stale, version-pinned Chrome population (stuck in-app WebViews and
      // crawler fleets). The live navigator UA is the signal that actually
      // fires client-side; the server-derived `browser` tag is normally ABSENT
      // during beforeSend, but parse it too for the server/envelope shape.
      var tags = event.tags || {};
      if (isStaleChromeMajor(chromeMajor(tags.browser))) return true;
      if (isStaleChromeMajor(chromeMajor(userAgent()))) return true;
    } catch (_) {
      /* never let the filter throw */
    }
    return false;
  }

  // Category 6: the redundant onerror copy of a Rust panic. See the header.
  // An onerror "RuntimeError: unreachable" whose stack carries one of OUR
  // pkg-bundle frames is the abort()-propagation of a panic the hook already
  // reported as an actionable RustWasmPanic — a guaranteed duplicate that
  // fragments per deploy. Drop it. Unlike the category-3 onerror prong (which
  // is fingerprint-gated to preserve a possible real bug), this is safe to drop
  // UNCONDITIONALLY because the actionable copy is retained: the panic hook is
  // browser-agnostic, so even a clean current browser still emits the kept
  // RustWasmPanic. Scoped to our bundle (a frameless or third-party-framed
  // RuntimeError is preserved) and to the `unreachable` value (other wasm traps
  // from our bundle, e.g. "memory access out of bounds", still report). The
  // value is matched loosely so SpiderMonkey's "unreachable executed" and JSC's
  // "Unreachable code should not be executed" are covered too.
  function isRedundantWasmUnreachableTrap(event) {
    try {
      var ex = firstException(event);
      if (!ex) return false;
      if (ex.type !== "RuntimeError" || typeof ex.value !== "string")
        return false;
      if (!/unreachable/i.test(ex.value)) return false;
      var frames = (ex.stacktrace && ex.stacktrace.frames) || [];
      for (var i = 0; i < frames.length; i++) {
        var fname = frames[i] && frames[i].filename;
        if (typeof fname === "string" && ULTROS_PKG_FRAME_RE.test(fname)) {
          return true;
        }
      }
    } catch (_) {
      /* never let the filter throw */
    }
    return false;
  }

  function isEmptyPromiseRejection(event) {
    try {
      var ex = firstException(event);
      if (!ex || typeof ex.value !== "string") return false;
      // Sentry's synthetic wrapper for a promise rejected with no reason.
      // value: undefined / null carries no message and no stack — there is
      // nothing to act on, and it is overwhelmingly third-party (gtag /
      // funding-choices / ads) or an aborted fetch. Exact-match keeps it
      // narrow: a rejection carrying a real value renders "with value: ..."
      // and is preserved.
      return (
        ex.value ===
          "Non-Error promise rejection captured with value: undefined" ||
        ex.value === "Non-Error promise rejection captured with value: null"
      );
    } catch (_) {
      /* never let the filter throw */
    }
    return false;
  }

  window.__ultrosShouldDropEvent = function (event) {
    return (
      isUltrosWasmFetchAbort(event) ||
      isUltrosWasmCompileFailure(event) ||
      isStrippedHydrationBootstrap(event) ||
      isInjectedDocumentTypeError(event) ||
      isInjectedTachysHydrationPanic(event) ||
      isRedundantWasmUnreachableTrap(event) ||
      isEmptyPromiseRejection(event)
    );
  };
})();
