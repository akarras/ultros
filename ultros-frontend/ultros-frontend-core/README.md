# ultros-frontend-core

The frontend API facade, shared application contexts, game-data access and
realtime subscriptions. Components consume these definitions directly; the
app also re-exports them for its routes and server integration.

Enable `ssr` or `hydrate` to match the application. Providers and consumers
must share context types. Toast and clipboard state are re-exported from the
base UI crate. The app supplies its commit to the update provider; this crate
does not embed build-specific version metadata.

See the [frontend architecture](../README.md) for dependency direction and checks.
