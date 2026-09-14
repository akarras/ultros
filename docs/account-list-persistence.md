# Account list persistence

Account snapshots are primary offline data, not a disposable cache. The previous
20-list LRU is disabled for all snapshots, including older records whose server
acknowledgement is unknown. Browser quota is the remaining limit; failure leaves
other lists intact and exposes retry and a device-compatible recovery download.
This intentionally favors retention over reclaiming space. A future bounded
cache must keep primary pending edits separately and prove coverage by an accepted
server document version. A connected socket or a successful send is not that proof.

Every current-client snapshot save acquires a per-account Web Lock, reads the
latest snapshot, imports it into a temporary Loro document, exports the merged
document, and writes snapshot plus index. Failure or pending history is not
reported as durable. Index failure is reported even when the snapshot write
succeeded; retry repairs the index. No unlocked fallback is used on browsers
without Web Locks. Storage events import peer work without adding it to local
undo and relay it through the connected tab's existing sync path.

The save indicator describes local durability separately from connection state.
Unsaved snapshots stay in account-scoped memory across client-side navigation;
leaving the browser with unsaved work prompts a warning. Recovery downloads use
the existing device-list backup format and can be restored without signing in.
Browser-data clearing and forcibly terminating a tab before saving remain outside
application durability guarantees.

Revocation purges the snapshot and advances a generation token. Queued saves
captured before that purge must not recreate it. Account IDs scope snapshots,
locks and recovery buffers. A stale or incompatible compacted history fails
closed with recovery available; compaction-intent recovery remains #1468.

Validation: native tests exercise real Loro merges, stale saves, quota/index
failures, generation invalidation and 21+ retained lists. Run
`node integration/account-list-storage.cjs` for real-browser Web Lock concurrency,
retry and unsupported-lock behavior. With a fresh test-auth server, run
`BASE_URL=http://127.0.0.1:8080 node integration/account-list-ui.cjs` for two
offline tabs closed before reconnect, quota failure, navigation recovery, retry,
reload, sign-out and account switching. Account route remounts still require
network access for their REST shell; this does not add cold offline account pages.

Save acknowledgement compares causal version coverage, not serialized version
bytes, whose map ordering can differ between equivalent documents. Recovery
status reads tolerate a disposed handle during route replacement.
