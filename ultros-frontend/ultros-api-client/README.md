# Ultros API client foundation

This crate owns the shared API error types and the server's in-process Axum
transport. It lets transport and error-serialization tests run without compiling
Leptos views, application state, or game-data packs.

The default feature set contains only the error contract. `ssr` enables the
Axum transport; `hydrate` enables conversion from browser transport errors.
`ultros-app` forwards these features and preserves its `error` and public
`ssr_api` module paths through re-exports.

The transport implementation and all nine existing tests are unchanged.
`SsrApi::request`, the error-classification methods, and `AppResult` become
public so the app can continue calling them across the crate boundary. The
ten-second deadline still covers the handler and streaming response body, and
headers remain request-local. No networking or serialization behavior changes.

Endpoint functions, login bootstrap, and app-update detection remain in the
app for a separate extraction. In particular, this crate neither owns the
application router nor captures user headers in its shared transport.

```sh
cargo test --locked -p ultros-api-client --features ssr
./check_ci.sh
cargo leptos build
```

The app's existing API-facade integration test continues exercising this
transport through the compatibility re-export. No percentage build-time
improvement is claimed by this extraction alone.
