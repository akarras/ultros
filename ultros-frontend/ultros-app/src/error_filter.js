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
//   1. WASM bundle fetch aborts — users navigating away during the
//      streaming compile, ad blockers, corporate proxies. Two shapes:
//      a bare "TypeError: Failed to fetch" from the wasm-bindgen glue,
//      and the ESM-entry "Failed to fetch dynamically imported module:
//      <pkg-url>" thrown when the module preload/import() of our own
//      bundle aborts. GlitchTip issues #21, #2374, #2404, and the
//      count=1 flood of #62xx "Failed to fetch dynamically imported
//      module: .../pkg/<hash>/ultros.js".
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
//      rust_panic context at all. Suppressed only when an injecting
//      fingerprint is present: a <font> element in the live DOM (which
//      Ultros never emits, so it is necessarily translation-injected —
//      this catches the modern-Chrome CN population that ignores the
//      notranslate trifecta), the page-stability breadcrumb, or the
//      frozen Chrome 112 WebView UA. Genuine hydration mismatches on a
//      clean page have no <font> and still reach GlitchTip. Issues #678,
//      #707, #770, #1307, #2277, #2775, #3005, #4905, #4911, #6406.
//   4. "Non-Error promise rejection captured with value: undefined"
//      (and the null variant) — Sentry's synthetic wrapper for a promise
//      rejected with no reason. Zero diagnostic value (no message, no
//      stack), overwhelmingly third-party (gtag / funding-choices / ads)
//      or aborted fetches. The count=1 flood of #62xx "UnhandledRejection".
(function () {
  var ULTROS_PKG_BUNDLE_RE = /\/pkg\/[a-f0-9]+\/ultros\.(?:js|wasm)(?:$|\?)/;
  var ULTROS_TACHYS_HYDRATION_RE = /\/tachys-[\d.]+\/src\/hydration\.rs:/;
  var ULTROS_INJECTOR_BREADCRUMB = "检测页面稳定";
  // The frozen-Chrome-112 in-app WebView population (Tencent QQ / UC /
  // WeChat) that injects the translation + page-stability overlays. Chrome
  // 112 shipped April 2023; any live, self-updating browser is many majors
  // past it, so matching the 112 UA targets the stuck WebViews without
  // catching real users on current browsers.
  var ULTROS_FROZEN_CHROME_RE = /\bChrome\/112\./;

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

  // Is this event the tachys hydration panic, in any of its three shapes?
  function isTachysHydrationPanicEvent(event) {
    // (a) The handled root panic carries the tachys path in rust_panic.
    var ctx = event && event.contexts && event.contexts.rust_panic;
    var loc = ctx && ctx.location;
    if (
      typeof loc === "string" &&
      loc.indexOf("/usr/local/cargo/registry/src/index.crates.io-") === 0 &&
      ULTROS_TACHYS_HYDRATION_RE.test(loc)
    ) {
      return true;
    }
    // (b) The `RefCell already borrowed` cascade reports with a js-sys
    //     executor location, and the unhandled `RuntimeError: unreachable`
    //     (window.onerror) has no rust_panic context at all — but both still
    //     carry the original `panicked at .../tachys-*/src/hydration.rs:`
    //     console breadcrumb that kicked off the cascade.
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

      // Frozen Chrome 112 in-app WebView population. The server-derived
      // `browser` tag is normally ABSENT during client-side beforeSend, so
      // the live navigator UA is the signal that actually fires here.
      var tags = event.tags || {};
      if (tags.browser === "Chrome 112.0.0") return true;
      if (ULTROS_FROZEN_CHROME_RE.test(userAgent())) return true;
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
      isInjectedDocumentTypeError(event) ||
      isInjectedTachysHydrationPanic(event) ||
      isEmptyPromiseRejection(event)
    );
  };
})();
