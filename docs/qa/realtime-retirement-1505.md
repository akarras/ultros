# Realtime callback retirement (#1505)

A list navigation can replace a connection while the previous browser WebSocket
is still CLOSING. Previously the Rust callback closures were dropped while the old
socket still held their JavaScript functions. Late close, error, open or message
events could then throw `closure invoked recursively or after being dropped`.

The client now detaches all four event properties before dropping their closures,
both when replacing a socket and when its final Rust owner is dropped. Each
connection's callbacks and reconnect timer are fenced by its generation. One owned
reconnect timer coalesces the browser's error/close sequence and is cancelled when
opening, replacing or disposing the connection.

## Regression procedure

Use a fresh test-auth client/server build and its own database:

```sh
BASE_URL=http://127.0.0.1:53125 npm --prefix integration run test:realtime-retirement
BASE_URL=http://127.0.0.1:53125 npm --prefix integration run test:list-compaction-recovery
BASE_URL=http://127.0.0.1:53125 npm --prefix integration run test:list-sync
```

The first probe uses real browser WebSockets, authenticated list subscriptions and
server edits. It only overrides the old socket's visible readyState to hold the
CLOSING transition deterministically while a real SPA navigation replaces it.
Delayed events go through the old event target, not saved callback references.
The probe asserts handler detachment, no application exceptions, one timer for an
error/error/close sequence, one successor, and a real edit reaching the server.
The final-owner Rust destructor is reviewed in source: the app's global realtime
context does not expose an in-page disposal control.

Baseline `1644d84faa3eff56272bf656b773d6276209ec3d` fails this probe with all four
handlers attached and dropped-closure exceptions; the broader compaction suite
also reproduced the CloseEvent exception twice despite passing all four data
preservation scenarios. No application-error allow-list was added.

The existing search responsiveness probe now disables only the external ad SDK
and keeps every page-error stack in its failure assertion. A full-stack diagnostic
identified the previous opaque `Wl` exception as Google's ad script. The same
application regression passed with that script disabled. This avoids confusing
third-party ad execution with disposed application signals.
