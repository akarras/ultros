# Dalamud shopping lists: Lists 2.0 and Rust interop

Date: 2026-10-18. Status: design plus a [connected prototype](../plugins/dalamud/README.md). Lists 1.0 is being deprecated; the game client is a Lists 2.0 peer. ABI 2 now exposes a shared Rust handshake/recovery state and durable checkpoint fencing; C# owns HTTP, protected credentials and atomic persistence on one worker. See [current validation and remaining limits](dalamud-prototype-status.md). In-game and Wine/macOS behavior remain unvalidated. Full-release sections below remain a roadmap.

The cart is a Lists 2.0 peer using the same Loro document as the website. Reuse the Rust document and recovery implementation through a narrow native interface. Keep Dalamud/ImGui, HTTP/WebSockets, account pairing, and credential storage in C# initially.

## Existing code to reuse

| Component | What exists | Proposed use |
| --- | --- | --- |
| [`ultros-list-doc`](../ultros-list-doc/src/lib.rs) | Typed `ListDocument`, pinned Loro 1.16.2, schema validation, version vectors, binary import/export, row identity | Compile into the native library; this is the primary reusable core. It has no Leptos, database, or network dependency. |
| [`recovery.rs`](../ultros-list-doc/src/recovery.rs) | Causal common-base recovery after missing/compacted history, additive acquisition deltas, identity checks, explicit conflicts | Reuse directly. Preserve both copies when recovery needs review. |
| [`undo.rs`](../ultros-list-doc/src/undo.rs) | Local undo, purchase undo, explicit action grouping | Expose the same behavior to the plugin. |
| Browser [`list_doc/sync.rs`](../ultros-frontend/ultros-app/src/list_doc/sync.rs) | Handshake/diff/resync guards, rejection-loop prevention, durable recovery fencing | Extract framework-independent state transitions into a shared Rust client-core crate, then adapt the browser and native bridge to it. Do not embed Leptos, localStorage, timers, or browser callbacks. |
| [`ultros-lists`](../ultros-lists/src/sync.rs) | Server merge, projection, activity, alert publication | Keep on the server. It depends on database/alert/server crates and is not the library to embed in Dalamud. |
| [`ultros-calc`](../ultros-frontend/ultros-calc/src/lib.rs) | Framework-independent shopping calculations | Keep the proposed server calculation adapter initially; local calculation reuse can follow separately if needed. |

Existing behavior and regressions are documented in [lists-sync.md](lists-sync.md) and [the Lists 2.0 spec](superpowers/specs/2026-09-07-lists-local-first-sync-design.md). This does not assert that browser-specific orchestration is already a ready-made native client: extracting the shared state machine is implementation work.

## Runtime and portability

Current Dalamud API 15 targets .NET 10; its build properties specify `net10.0-windows` and x64. Its boot code hosts CoreCLR through Windows `hostfxr`/`nethost`. The Linux launcher explicitly runs the injector in its Wine prefix and passes the bundled .NET runtime as Wine paths. This is distinct from running the plugin on the host machine's Mono runtime. Sources: [API 15](https://dalamud.dev/versions/v15/), [build properties](https://github.com/goatcorp/Dalamud/blob/master/Directory.Build.props), [CoreCLR host](https://github.com/goatcorp/Dalamud/blob/master/Dalamud.Boot/CoreCLR.cpp), [Unix runner](https://github.com/goatcorp/XIVLauncher.Core/blob/main/src/XIVLauncher.Common.Unix/UnixDalamudRunner.cs).

Therefore the initial artifact to test is one Windows x64 Rust DLL (`x86_64-pc-windows-msvc`) loaded by the Windows game process. On Linux that same process runs through Wine/Proton; for Wine-based macOS launchers the same Windows-process reasoning is the hypothesis to validate. [XIV on Mac](https://github.com/marzent/XIV-on-Mac) documents a Wine-based launcher with Dalamud support, including Apple Silicon. Its launcher being host-native does not imply that a game-plugin library should be a macOS arm64 dylib.

Using the same Windows DLL across these hosts is an architectural expectation, not a tested portability claim. Test the actual loader/runtime/CPU/dependency combinations. A future genuinely native Linux/macOS consumer would need a matching `.so`/`.dylib` build, but can keep the same C ABI. Do not select an artifact solely from the physical host OS.

Ship a self-contained document core without OpenSSL, a second .NET runtime, or server dependencies. Audit imported DLL/CRT dependencies and use a conservative CPU baseline rather than `target-cpu=native` or Linux server optimization flags. Windows APIs used by Rust dependencies still need Wine compatibility tests. Protected token storage is a separate platform issue: loading the DLL successfully does not validate Wine DPAPI security.

## Native interface

The prototype's `ultros-list-doc-ffi` crate builds as a `cdylib`, depending only on the document core and JSON/base64 adapters. Its managed `Ultros.Lists` adapter uses **C ABI** P/Invoke. Opaque integer document IDs are wrapped by `SafeHandle`; immutable response IDs expose bounded size/copy/free calls for caller-owned byte buffers. No Rust allocation or internal `String`, `Vec`, enum, or trait object crosses the boundary. Full sync will also need the extracted client state machine. Reference: [Rust FFI](https://doc.rust-lang.org/nomicon/ffi.html), [.NET native loading](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/native-library-loading).

The connected prototype serializes document, HTTP and disk work on one C# worker, uses outbound SSE notification wake-ups with HTTP fetch/recovery and list polling, and keeps the native module resident until game exit. The stream reader only signals the worker; it never edits documents or refreshes credentials concurrently. The separate local merge lab remains synchronous. Native hot reload and WebSocket transport below remain future work; no inbound network listener is opened.

The boundary needs only:

- ABI/protocol/schema compatibility query, open/close document context, and explicit owned-buffer release.
- Load/export snapshots and causal versions, import updates, and export the same pending causal diff again after a lost send.
- Typed local actions (`add_item`, `set_need`, `add_acquired`, `set_acquired`, `set_quality`, remove, undo/redo) that mutate the local document, not server command endpoints.
- A row/meta projection for drawing, plus readiness/dirty/recovery state. JSON UTF-8 is acceptable for low-frequency projections; Loro bytes remain binary buffers until transport base64 encoding. Define integer widths and signedness explicitly.
- Sync input/output actions: consume a server snapshot/update/up-to-date/error, request send/resubscribe/persist, and report durable-save success/failure. Extraction must preserve existing recovery guards.

Use opaque handles wrapped by C# `SafeHandle`; specify pointer/length ownership and release every allocation through its allocating library. Initially prefer serialized calls and returned actions over callbacks across languages. A per-document worker owns edits/import/recovery; the ImGui draw thread reads immutable C# snapshots. Never use a native game pointer as document input or call network APIs from the draw loop.

Resolve the library from the plugin's package directory using an assembly-scoped resolver and a uniquely versioned filename, not a global PATH change. Negotiate the ABI before opening data. Stop workers and dispose handles before unloading managed/native components; test updates and hot reload with old/new library versions. If loading or ABI checks fail, disable cart editing and report it without breaking notifications/item lookup/uploader controls.

Exported Rust functions must return structured errors and catch unwinding panics inside Rust; use an unwind-capable native build profile. Mark a panicked context unusable and preserve the last good saved snapshot. `catch_unwind` does not contain an abort, OOM, invalid pointer, or native access violation: an in-process native defect can crash FFXIV. Buffer limits, fuzzed imports, out-of-process bridge tests, lifecycle tests, and a small ABI are release gates. Ordinary managed exceptions cannot make arbitrary FFI memory access safe.

## Sync and concurrent edits

1. Load a validated full local snapshot keyed by origin/account/list/client profile. A missing cache starts with `empty_peer`, not an invented empty complete list. Reopening uses a fresh writer peer ID; never share a live peer identity across game clients or browser tabs.
2. `SubscribeListDoc` sends the current causal version. Apply `ListDocSubscribed` snapshots/updates/up-to-date, then export only what the server lacks. `ListDocUpdate` carries the original operations; replaying those bytes is idempotent. Never turn a retransmission into another local `add_acquired` call.
3. Persist local changes and accept remote updates using the existing document validation. Disconnection leaves the document editable when it was known readable/writable; save locally and handshake again on reconnect. The document is the durable offline state, not an additional command queue.
4. Handle `Stale`/cross-replica missed relays with bounded version handshakes (initial 60-second mean fallback with jitter), not JSON invalidation against Lists 1.0. Use the same typed sync envelopes on a bounded plugin-authenticated fallback if required; keep one per-document state machine.
5. Missing dependencies or compacted history trigger the shared recovery algorithm. Persist the recovered replacement successfully before sending newly generated operations; preserve the rejected-version guard across save/reload. Otherwise a lost acknowledgement or failed save can duplicate an acquisition during re-expression.
6. Permission loss, deletion, account changes, expired credentials, incompatible schema, and storage failure are distinct states. Session expiry pauses transport and keeps local work; explicit permission denial/deletion follows the existing cache-purge policy. A different account never receives the old account's document or pending changes. Do not expose an automatic upload after reconnect without current authorization.

Normal concurrent edits merge rather than returning a whole-list `expected_revision` conflict. The existing convergence suite includes concurrent `+2` and `+3` Acquired changes producing `5`, and remove winning over an edit inside that row. Other fields follow the current document semantics: Need/Target are last-writer-wins, same-row concurrent additions to Need are not an additive counter, and Acquired setters express deltas. Two peers setting Acquired to 5 from 0 can produce 10. Preserve those semantics in the UI; CRDT convergence does not mean every edit becomes additive or every user intention survives a simultaneous overwrite. See [document operations](../ultros-list-doc/src/document.rs:314) and [convergence tests](../ultros-list-doc/tests/convergence.rs).

Keep recovery review for genuine compaction/history/identity conflicts. Do not impose a new scalar list revision or manual retry for unrelated ordinary edits.

## Authorization remains server-side

Amend the earlier deny-all-Loro device policy: `SubscribeListDoc` requires `lists:read` and current list permission; `ListDocUpdate` requires `lists:write` plus current write permission. Revalidate the device/account and membership evidence in the merge transaction; retain expiry/revocation checks on subscriptions and outgoing events. Notification-only/read-only tokens cannot mutate documents.

Route updates through the existing `ListSync`/validated merge/project/activity/alert path with a distinct plugin actor. The native DLL cannot authorize anything. For the initial cart scope, cap effective merge permission at row-write level, even when the account owns the list; changing list name/scope requires a separately designed management capability. The existing metadata-forbidden check is a foundation, but plugin permission capping is new work. Bound frame/update/snapshot sizes and subscription counts; malformed or future-schema data must not mutate live state.

## Acceptance spike and release gates

Before depending on this bridge, demonstrate one C# host loading the DLL, opening the server/browser fixture, editing and round-tripping a snapshot. Then test that exact packaged Windows artifact under Windows, Linux Wine/Proton, and the supported macOS Wine launcher, recording runtime/game/CPU versions. There is no portability validation yet.

Run mixed browser/Rust/C# convergence, duplicate/reordered frames, concurrent acquisition, offline restart, compaction, lost acknowledgement, and failed durable recovery tests. Verify two game clients use distinct document peers and protected profiles. Exercise missing DLL, wrong architecture/ABI, panic/error paths, buffer ownership, unload/reload, incompatible cache, revoked write scope, and permission races. Keep the core's pinned Loro compatibility fixtures shared across all clients. Add meaningful Rust/native/.NET checks to the root CI gate when implemented; no C ABI should become a second list schema or a second sync protocol.
