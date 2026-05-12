# Frontend Error Reporting

Ultros can report browser crashes and Rust WASM panics to any Sentry-compatible
collector. The intended self-hosted target is GlitchTip, which accepts events
from the official Sentry browser SDK.

## Collector

GlitchTip is the best fit for the current app because it is self-hostable and
keeps the integration on the Sentry protocol. That lets us point the same
browser instrumentation at GlitchTip, Sentry SaaS, or another compatible
backend by changing the DSN.

Minimum deployment shape:

- GlitchTip web service
- PostgreSQL
- Optional Valkey/Redis for larger installs
- HTTPS reverse proxy

Create a frontend/browser project in GlitchTip and copy its public DSN into the
Ultros environment.

## Ultros Environment

```bash
ULTROS_ERROR_REPORTING_DSN=https://public-key@glitchtip.example.com/1
ULTROS_ERROR_REPORTING_ENVIRONMENT=production
ULTROS_ERROR_REPORTING_SAMPLE_RATE=1.0
ULTROS_ERROR_REPORTING_TRACES_SAMPLE_RATE=0.0
ULTROS_ERROR_REPORTING_SDK_URL=https://browser.sentry-cdn.com/10.34.0/bundle.min.js
```

`ULTROS_ERROR_REPORTING_DSN` is the only required setting. If it is empty or
unset, no SDK is loaded and no events are sent.

`ULTROS_ERROR_REPORTING_TRACES_SAMPLE_RATE` defaults to `0.0` because this is
crash telemetry first. Raise it later if browser performance traces are useful.

`ULTROS_ERROR_REPORTING_SDK_URL` can point at a self-hosted copy of the Sentry
browser bundle if we want to avoid loading the SDK from Sentry's CDN.

## Rust WASM Panics

The WASM hydrate entrypoint installs a panic hook that still forwards panic
details to `console.error` for local debugging, then calls a JavaScript bridge
installed by the shell. The bridge captures a synthetic `RustWasmPanic`
exception and tags it with `runtime=wasm`.

This is better than `panic = "abort"` for the current app: the hook gets the
Rust panic message and source location while the JavaScript SDK can attach the
browser stack. `panic = "abort"` would usually degrade the reported error to a
generic WASM trap unless we build more custom stack capture around it.

## Symbols, Source Maps, And WASM Debug Files

Use GlitchTip source map upload for the generated JavaScript glue and any
future bundled JavaScript. Do not publish source maps publicly; upload them to
the collector during deployment.

The `wasm-split` mentioned in Sentry's WASM docs is Sentry Symbolicator's tool,
not Binaryen's module-splitting tool. It post-processes a `.wasm` artifact for
symbolication:

- Adds a `build_id` custom section when one is missing.
- Strips debug sections from the shipped `.wasm`.
- Writes a private debug `.wasm` file that can be uploaded to the collector.

Example:

```bash
wasm-split target/site/pkg/ultros_bg.wasm \
  --debug-out target/site/pkg/ultros_bg.debug.wasm \
  --strip
```

Upload the `*.debug.wasm` file as a debug information file, and ship the stripped
`.wasm` that still contains the matching `build_id`.

For WASM, the practical production path is:

- Build the WASM with DWARF debug info available in the release artifact.
- Run Sentry's `wasm-split` to add/retain a `build_id`, strip the shipped file,
  and keep the private debug file.
- Upload generated JavaScript source maps for the wasm-bindgen glue.
- Upload the private WASM debug file to a collector that supports Sentry debug
  information files and WASM symbolication.
- Keep Rust commit/release metadata attached to each event through `release`.

Sentry's browser-side WASM stack enrichment uses `@sentry/wasm` with
`wasmIntegration()`. Ultros currently has no JavaScript bundling step, so the
server shell initializes the browser SDK as a lightweight first pass and the
Rust panic hook reports `RustWasmPanic` events. If we want first-class
Sentry-style WASM symbolication, add a tiny JS bundle that imports
`@sentry/browser` and `@sentry/wasm`, initializes `wasmIntegration()`, and then
wire the release pipeline to run `wasm-split` and upload debug files.

Before choosing GlitchTip for the full symbolication path, verify its support
for Sentry debug information files with WASM DWARF. Its Sentry-compatible SDK
ingestion is a good match for browser errors, but Sentry's WASM symbolication
depends on the native debug-file processing path.
