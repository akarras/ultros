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
//      rust_panic context at all. That last shape is ambiguous in production:
//      the wasm is built with `panic = "immediate-abort"`, so EVERY panic
//      arrives as it. beforeSend therefore symbolicates such a trap first and
//      drops it only when its top frames are tachys hydration code
//      (__ultrosIsInjectedTrapCandidate / __ultrosShouldDropSymbolicatedTrap).
//      Suppressed only when an injecting /
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
//   6. (Retired.) The onerror "RuntimeError: unreachable" used to be dropped
//      as the redundant twin of a hook-reported RustWasmPanic. The production
//      wasm is now built with `panic = "immediate-abort"` (Dockerfile): a
//      panic runs NO hook and formats NO message, it executes the wasm
//      `unreachable` instruction on the spot. That trap, caught by the
//      browser's global onerror / unhandledrejection, is therefore THE panic
//      report — wasm_symbolicate.js resolves its `wasm-function[N]` frames
//      to Rust function names and fingerprints it by the top frames. It must
//      never be dropped on frame shape alone. (Debug and local release
//      builds keep the hook, so RustWasmPanic still exists there.)
//   7. The "RefCell already borrowed" panic in the wasm-bindgen-futures
//      single-threaded executor (js-sys .../futures/task/singlethread.rs).
//      Every Rust panic that unwinds through a running future poll re-enters
//      that executor while its task-queue RefCell is still held, tripping a
//      SECOND, handled "RefCell already borrowed" panic. It is therefore
//      always the cascade of a PRIMARY panic the hook already reported (e.g.
//      the tachys hydration unreachable! at hydration.rs, kept with its own
//      actionable contexts.rust_panic.location and stable per-location
//      fingerprint), and its own location — singlethread.rs — is identical
//      for every panic, so it carries no diagnostic signal of its own. Dropped
//      UNCONDITIONALLY (no injecting-population fingerprint) when
//      contexts.rust_panic.location is that executor path: the actionable
//      primary copy is always retained, so a genuine hydration bug on a clean
//      current browser still reaches GlitchTip via the primary. This is the
//      RefCell sibling of the category-6 RuntimeError onerror twin (PR #921);
//      it is #6758 (23k+ events), the single largest issue. Scoped to the
//      executor location and the exact "RefCell already borrowed" value, so an
//      APP-code double-borrow (which panics at an app/leptos path, never
//      singlethread.rs) and a RefCell with no rust_panic context both still
//      report.
//   8. An error thrown entirely inside a third-party analytics / ads / consent
//      / CDN-telemetry script that Ultros loads but does not control and cannot
//      fix — the JS sibling of an adblocked request. Cloudflare Web Analytics'
//      beacon.min.js throws "TypeError: t.entries.at is not a function" on
//      ancient browsers missing Array.prototype.at (GlitchTip #6836); gtag /
//      AdSense / funding-choices surface onerror noise the same way. The beacon
//      URL carries a rotating version hash, so each occurrence fragments into a
//      fresh count=1 issue. Dropped ONLY when the exception has stack frames and
//      EVERY one is on a known third-party host — any app/pkg frame (our bundle,
//      an inline page script, or SSR HTML on ultros.app) preserves the event, so
//      a real Ultros bug is never swept up.
(function () {
  var ULTROS_PKG_BUNDLE_RE = /\/pkg\/[a-f0-9]+\/ultros\.(?:js|wasm)(?:$|\?)/;
  // Third-party analytics / ads / consent / CDN-telemetry hosts. Ultros loads
  // scripts from these (Cloudflare Web Analytics' beacon, Google Analytics /
  // gtag, AdSense, the funding-choices consent frame, ad-traffic-quality), but
  // does not control their code and cannot fix a bug inside it. Matched against
  // a frame's absPath / filename (the `//host/` in the URL). Anchored to `//`
  // so a same-named path segment can't match; our own origin (ultros.app) and
  // pkg bundle are deliberately absent, so any frame in our code fails the test
  // and the whole event is preserved (see isThirdPartyScriptError).
  var ULTROS_THIRD_PARTY_SCRIPT_HOST_RE =
    /\/\/(?:static\.cloudflareinsights\.com|(?:www\.)?google-analytics\.com|(?:www\.)?googletagmanager\.com|pagead2\.googlesyndication\.com|[a-z0-9-]+\.googlesyndication\.com|[a-z0-9.-]*\.doubleclick\.net|fundingchoicesmessages\.google\.com|[a-z0-9.-]*\.adtrafficquality\.google|adservice\.google\.[a-z.]+)\//i;
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
    // (c) The bare `RuntimeError: unreachable` trap is NOT recognized here.
    //     Production wasm is built with `panic = "immediate-abort"`, so EVERY
    //     panic — not just the tachys hydration one — reaches window.onerror in
    //     exactly that shape. It is classified by its symbolicated frames
    //     instead: see isBareWasmTrap / __ultrosIsTachysHydrationTrap below.
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

  // Diagnostic snapshot of the live DOM at the moment a hydration-class panic is
  // reported. #6831 — the tachys `failed_to_cast_element` hydration panic
  // (tachys-0.2.18/src/hydration.rs:163, the release `unreachable!()` behind
  // "the framework expected an HTML <TAG> element, but found this instead") — is
  // the single largest issue by orders of magnitude, yet a clean, current
  // browser hydrates the failing /item/<world>/<id> URLs with no error (verified
  // live). The crash only fires once an external translation / injection tool
  // mutates the SSR DOM before Leptos hydrates. The four existing injector
  // fingerprints — a live <font>, html.translated-ltr/rtl, the page-stability
  // breadcrumb, an implausibly stale Chrome (<=124) — all MISS the current
  // flood: it is modern Chrome (131) carrying NONE of them, so we cannot yet
  // write a precise fingerprint for whatever tool this population runs. This
  // captures the DOM's injection surface — tag names and class attributes only,
  // never any text content — onto the event as contexts.dom_injection so the
  // injector's actual signature becomes visible on the events that still reach
  // GlitchTip and a targeted fingerprint can follow. Diagnostic only: it changes
  // nothing about what is dropped.
  function domInjectionSnapshot() {
    var snap = {};
    try {
      var doc = typeof window !== "undefined" && window.document;
      if (!doc) return snap;
      try {
        var fonts =
          doc.getElementsByTagName && doc.getElementsByTagName("font");
        snap.fontCount = fonts ? fonts.length : 0;
      } catch (_) {
        /* ignore */
      }
      try {
        var html = doc.documentElement;
        if (html && typeof html.className === "string") {
          snap.htmlClass = html.className.slice(0, 200);
        }
      } catch (_) {
        /* ignore */
      }
      var body = doc.body;
      if (body) {
        try {
          if (typeof body.className === "string") {
            snap.bodyClass = body.className.slice(0, 200);
          }
        } catch (_) {
          /* ignore */
        }
        try {
          var kids = body.children || [];
          var tags = [];
          for (var i = 0; i < kids.length && tags.length < 40; i++) {
            var t = kids[i] && kids[i].tagName;
            if (typeof t === "string") tags.push(t.toLowerCase());
          }
          snap.bodyChildTags = tags;
          snap.bodyChildCount = kids.length;
        } catch (_) {
          /* ignore */
        }
      }
    } catch (_) {
      /* never let the filter throw */
    }
    return snap;
  }

  function isInjectedTachysHydrationPanic(event) {
    try {
      // Only suppress when an injecting-population fingerprint is present, so
      // a genuine hydration mismatch on a clean page still reaches GlitchTip.
      return isTachysHydrationPanicEvent(event) && hasInjectingFingerprint(event);
    } catch (_) {
      /* never let the filter throw */
    }
    return false;
  }

  // The populations whose hydration panics are injected, not ours: a
  // translation overlay (breadcrumb, <font>, translated-* class on <html>) or
  // a stale, version-pinned Chrome.
  function hasInjectingFingerprint(event) {
    try {
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

  // The unhandled wasm trap at window.onerror: no rust_panic context, value
  // exactly "unreachable" (V8's wording — the population rule 3 targets is
  // Chrome). Matched exactly so other RuntimeError values, e.g. "table index
  // is out of bounds", are untouched. Must be read BEFORE symbolication, which
  // rewrites the value to "unreachable in <site>".
  function isBareWasmTrap(event) {
    var ex = firstException(event);
    var ctx = event && event.contexts && event.contexts.rust_panic;
    return !!(
      ex &&
      ex.type === "RuntimeError" &&
      ex.value === "unreachable" &&
      !ctx
    );
  }

  var TACHYS_FRAME_RE = /^<*tachys::/;
  // `hydrate_async` too: tachys's `HtmlElement::hydrate` and `hydrate_async`
  // each define an identical, non-generic `inner_1` (cursor step + inlined
  // `failed_to_cast_element`), the optimizer folds the two into one function,
  // and the name map can carry the async twin's name. GlitchTip #7964 is the
  // sync hydrate of a head <meta> trapping in
  // `<tachys::html::element::HtmlElement<_, _, _> as
  // tachys::view::RenderHtml>::hydrate_async::{closure#0}::inner_1`.
  var TACHYS_HYDRATION_FRAME_RE =
    /tachys::hydration::|^<*tachys::.*::hydrate(?:_async)?\b/;
  var WASM_FRAME_FILE_RE = /\.wasm:wasm-function\[\d+\]/;
  var TRAP_CLASSIFY_DEPTH = 3;

  // Classify a SYMBOLICATED trap: is it the tachys hydration panic?
  //   true  — the top wasm frame is tachys code and one of the top three is in
  //           tachys::hydration (`failed_to_cast_*`, `Cursor::*`) or a tachys
  //           `hydrate` impl. Under immediate-abort the trap fires in the
  //           panicking function itself, so there is no panic machinery above.
  //   false — resolved, and something else: an app panic during hydration has
  //           an `ultros_*` frame on top and is reported.
  //   null  — the top frame did not resolve (map missing or failed to load),
  //           so the trap cannot be told apart.
  function tachysHydrationTrap(event) {
    var ex = firstException(event);
    var frames = ex && ex.stacktrace && ex.stacktrace.frames;
    if (!Array.isArray(frames)) return null;
    var wasm = [];
    for (var i = frames.length - 1; i >= 0 && wasm.length < TRAP_CLASSIFY_DEPTH; i--) {
      var f = frames[i];
      var file = f && (f.filename || f.abs_path);
      if (typeof file === "string" && WASM_FRAME_FILE_RE.test(file)) wasm.push(f);
    }
    if (!wasm.length) return null;
    // Resolved names are Rust paths; an unresolved frame has none (or
    // `wasm-function[N]` / `?`), never a `::`.
    var names = wasm.map(function (f) {
      return typeof f.function === "string" && f.function.indexOf("::") !== -1
        ? f.function
        : null;
    });
    if (names[0] === null) return null;
    if (!TACHYS_FRAME_RE.test(names[0])) return false;
    for (var j = 0; j < names.length; j++) {
      if (names[j] !== null && TACHYS_HYDRATION_FRAME_RE.test(names[j])) {
        return true;
      }
    }
    return false;
  }

  // Category 7: the redundant "RefCell already borrowed" executor cascade. See
  // the header. A handled panic with value "RefCell already borrowed" whose
  // contexts.rust_panic.location is the wasm-bindgen-futures single-threaded
  // executor is never a primary fault: that executor RefCell only reports
  // "already borrowed" when run() is re-entered during a panic unwind, so it is
  // always the secondary cascade of a primary panic the hook already reported
  // (which keeps its own actionable location). Drop it UNCONDITIONALLY — unlike
  // the category-3 onerror prong (fingerprint-gated to preserve a possible real
  // bug), this is safe because the actionable primary copy is retained: a clean
  // current browser still emits, and keeps, that primary panic. Scoped to the
  // executor location (an app-code double-borrow panics at an app/leptos path,
  // not singlethread.rs, and is preserved) and to the exact value (other
  // executor invariants still report). The location is set by
  // __ultrosReportRustPanic and reliably present at client-side beforeSend.
  function isExecutorReentryCascade(event) {
    try {
      var ex = firstException(event);
      if (!ex || typeof ex.value !== "string") return false;
      if (ex.value !== "RefCell already borrowed") return false;
      var ctx = event && event.contexts && event.contexts.rust_panic;
      var loc = ctx && ctx.location;
      return (
        typeof loc === "string" &&
        loc.indexOf("/usr/local/cargo/registry/src/index.crates.io-") === 0 &&
        ULTROS_JSSYS_EXECUTOR_RE.test(loc)
      );
    } catch (_) {
      /* never let the filter throw */
    }
    return false;
  }

  // Category 8: an error thrown entirely inside a third-party analytics / ads /
  // consent / CDN-telemetry script that Ultros loads but does not control and
  // cannot fix — the JS sibling of an adblocked request. e.g. Cloudflare Web
  // Analytics' beacon.min.js throwing "TypeError: t.entries.at is not a
  // function" on an ancient browser missing Array.prototype.at (GlitchTip
  // #6836, Chrome 90 / Android 5); gtag, AdSense, and the funding-choices
  // consent frame surface onerror noise the same way. The beacon URL carries a
  // rotating version hash (…/beacon.min.js/v4513226…), so each occurrence would
  // otherwise fragment into a fresh count=1 issue — the per-hash noise the
  // category-6 dedup was built for. Dropped ONLY when the exception carries
  // stack frames and EVERY one originates from a known third-party host: a
  // stack that reaches even one app / pkg frame (our bundle, an inline page
  // script, or SSR HTML on ultros.app — none of which are on the host list)
  // fails the check and is preserved, so a real Ultros bug can never be swept
  // up. A frameless error is left untouched (nothing proves it third-party).
  function isThirdPartyScriptError(event) {
    try {
      var ex = firstException(event);
      if (!ex) return false;
      var frames = (ex.stacktrace && ex.stacktrace.frames) || [];
      if (frames.length === 0) return false;
      for (var i = 0; i < frames.length; i++) {
        var f = frames[i] || {};
        var abs = typeof f.absPath === "string" ? f.absPath : "";
        var fname = typeof f.filename === "string" ? f.filename : "";
        if (
          !ULTROS_THIRD_PARTY_SCRIPT_HOST_RE.test(abs) &&
          !ULTROS_THIRD_PARTY_SCRIPT_HOST_RE.test(fname)
        ) {
          return false;
        }
      }
      return true;
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

  // Attach the injection snapshot to hydration-class panics so the invisible
  // injector behind the #6831 residual flood (see domInjectionSnapshot) becomes
  // diagnosable on the events that still reach GlitchTip. Called from the
  // Sentry beforeSend wrapper (see shell() in lib.rs) BEFORE the drop check, so
  // the leaked-through events carry it; annotating an event that is then dropped
  // is harmless. Scoped to the same classifier the suppression uses, so ordinary
  // panics are never touched. Mutates the event in place and returns it.
  window.__ultrosAnnotateEvent = function (event) {
    try {
      if (event && (isTachysHydrationPanicEvent(event) || isBareWasmTrap(event))) {
        event.contexts = event.contexts || {};
        if (!event.contexts.dom_injection) {
          event.contexts.dom_injection = domInjectionSnapshot();
        }
      }
    } catch (_) {
      /* never let the filter throw */
    }
    return event;
  };

  // Rule 3 for the bare trap, part 1 — read BEFORE symbolication: is this a
  // trap from an injecting population? Only these are symbolicated-then-
  // classified; every other trap is simply reported.
  window.__ultrosIsInjectedTrapCandidate = function (event) {
    try {
      return !!event && isBareWasmTrap(event) && hasInjectingFingerprint(event);
    } catch (_) {
      return false;
    }
  };

  // Rule 3 for the bare trap, part 2 — read AFTER symbolication: drop a
  // candidate only when its frames say tachys hydration. An unresolved trap
  // (no map) keeps the pre-immediate-abort behaviour and is dropped, so a
  // missing map cannot re-open the flood.
  window.__ultrosShouldDropSymbolicatedTrap = function (event) {
    try {
      return tachysHydrationTrap(event) !== false;
    } catch (_) {
      return true;
    }
  };

  window.__ultrosShouldDropEvent = function (event) {
    return (
      isUltrosWasmFetchAbort(event) ||
      isUltrosWasmCompileFailure(event) ||
      isStrippedHydrationBootstrap(event) ||
      isInjectedDocumentTypeError(event) ||
      isInjectedTachysHydrationPanic(event) ||
      isExecutorReentryCascade(event) ||
      isThirdPartyScriptError(event) ||
      isEmptyPromiseRejection(event)
    );
  };
})();
