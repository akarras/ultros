# Ultros SSR lifecycle patch

Vendored from the official crates.io `hydration_context` 0.3.2 archive
(SHA-256 `285b05858fe7d58c5eff059f7b46c228fd86996d996c59eb35fc0340d174f640`).

Late resource serializers can remain buffered after the hydration-data stream
ends. They hold child owners, which hold the same shared context. Stream wakers
can also retain the underlying async buffer independently of the context Arc.
Removing the request's reactive arena values does not break these cycles.

`SharedContext::close()` is a default no-op for browser/custom contexts. On the
server it closes registration, takes the serializer and deferred-work buffers,
and drops their futures after releasing the locks. Writers check closure under
their respective buffer locks, so concurrent or later registrations cannot
repopulate a closed request. Closure is idempotent and permits reentrant cleanup.

The companion `reactive_graph` patch calls this at forced root teardown and
root destruction. Ordinary effect cleanup and child cleanup do not close the
context. This preserves the existing hydration protocol and context access.

The regression tests cover futures that retain their own context, including
while a stream independently retains the buffers, and rejection of late work.
The owner/arena lifetime tests are in `vendor/reactive_graph`.

Run `cargo test --locked --manifest-path vendor/hydration_context/Cargo.toml --lib`.
`scripts/check_tests.sh` includes this gate. `ULTROS_PATCH.diff` applies to the
pristine 0.3.2 archive; regenerate it after edits. The manifest's `[workspace]`
exists only to run standalone tests. The root workspace and reactive_graph's
standalone workspace both patch this crate to the vendored path.
