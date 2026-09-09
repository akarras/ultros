# Lists local-first sync

`/list/:id` behind the `lists-sync` Labs toggle runs on a Loro CRDT document
stored in the browser and merged through the server. Spec:
`docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md`.

## Bundle

| build | raw | gzip -9 |
|---|---|---|
| before (prod, main @ 14ff1fc) | 18,108,088 | 5,681,660 |
| after (Phase 4 @ 67eeba30) | 20,138,107 | 6,427,736 |
| growth | +2,030,019 | +746,076 (limit 750 KB per the spec) |

The "before" row is the wasm production served on 2026-09-09 (same
`cargo leptos build --release` pipeline as the Dockerfile); Phases 1 to 3
added no frontend crate, so it is the pre-Loro baseline. Loro accounts for
the difference; measured in isolation it is 1.99 MB raw and 703 KB
compressed.

## How it works

- One Loro document per list. The server keeps the merged state in
  `list_doc` (`snapshot`, `version`, `changes_since_compaction`) and keeps
  `list_item` and the `list` name and scope as a projection of it. Nothing
  else writes those rows: the REST handlers, the Discord bot and the socket
  all call `ListSync` (`ultros/src/lists/sync.rs`), and the old row writers
  were deleted from `ultros-db/src/lists.rs`, so the compiler enforces it.
- The browser stores one snapshot per user and list under
  `ultros.listdoc.v1.{user_id}.{list_id}` in localStorage, with a per-user
  index at `ultros.listdoc.index.v1.{user_id}` (last use and last known
  permission). At most 20 snapshots per user; least recently used evicted
  first. Sign-out closes the document but keeps the snapshot, so an offline
  edit survives a session expiry. A permission denial or a deleted list
  purges it.
- Sync rides the existing websocket. `SubscribeListDoc` carries the
  client's version vector; the server replies with a snapshot, the missing
  updates, or up-to-date, then relays every other peer's updates. Bytes are
  base64 inside the JSON frames. Offline edits need no queue: every
  reconnect re-runs the handshake with the current version and the client
  sends whatever the server lacks.
- If the server cannot take the client's history (it compacted past it, or
  a non-owner renamed the list), it answers with a fresh snapshot; the
  client rebases its local rows onto that snapshot as new operations and
  sends them once more. A second rejection of the same server version stops
  the loop and shows the page as offline.
- An idle page revalidates on any list broadcast (one REST fetch per second
  at most), so a revoked share or a deleted list reaches an open tab within
  about a second.
- Undo is Loro's undo manager: local operations only, one-second merge
  window, 100 steps, per open page. Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y (Cmd on
  Apple), never inside an input, textarea or an open modal.

## Debugging

- Handshake by hand: log in with `curl -c jar "$BASE/test/login?user_id=...&username=..."`
  on a test-auth build, then `websocat -H "Cookie: discord_auth=..." ws://host/api/v1/realtime/events`
  and send `{"SubscribeListDoc":{"subscription_id":1,"list_id":ID,"version":""}}`.
  The reply is a `ListDocSubscribed` with a `Snapshot` payload and the
  server's version.
- A list whose page looks wrong: compare
  `SELECT list_id, length(snapshot), changes_since_compaction, updated_at FROM list_doc WHERE list_id = ID`
  with `SELECT * FROM list_item WHERE list_id = ID`. The rows must equal the
  document; if they do not, a write bypassed `ListSync`, which is a bug.
- Compaction: a snapshot is replaced by a shallow one after 5,000 changes or
  256 KB. Clients that start from a shallow snapshot still sync both ways.
- Bus lag: the socket answers a lagged relay with `Stale` and the client
  re-runs the handshake with its current version. Every client resubscribe
  adds a relay stream on the server for that socket; merges are idempotent,
  so this costs bandwidth, not correctness.
- Browser side: `localStorage.getItem("ultros.listdoc.index.v1.<user_id>")`
  lists the cached lists, their last use and last known permission.
- Divergence query for the soak:
  `SELECT l.list_id FROM list_doc l WHERE l.updated_at > now() - interval '1 day'`
  gives the lists touched; for each, the projection check above must hold.

## Promotion

Promotion out of Labs deletes `LAB_LISTS_SYNC`, the `LabsSettings` section
when the registry is empty, the legacy `ListView`, and the REST-driven
actions it owns. It requires the soak below to pass.

## Production soak

Run after the branch is deployed, over at least a week with the toggle on
for the maintainer's own shared lists.

1. Turn it on under Settings › Labs on two browsers and one phone, and
   share one list between two Discord accounts.
2. Daily: GlitchTip has no new issue whose title contains `list_doc`,
   `ListDocError`, `sync_payload` or `hydration` on `/list/`;
   `docker logs ultros | grep -i "list document"` shows no warnings from
   `ListSync::publish`; `SELECT count(*), pg_size_pretty(sum(length(snapshot))) FROM list_doc;`
   grows with use, not runaway, and no snapshot above 256 KB survives a
   change; the Discord bot's `/list add_item` and `remove_item` land on the
   Labs page live.
3. Exit criteria: zero divergence between `list_doc` and `list_item` for
   every list touched during the soak; no data loss on the shared list,
   including both accounts ticking the same item within a second of each
   other; the legacy page, opened without the toggle, always matches the
   Labs page.

When all three hold, open the redesign spec (Lists 2.0 sub-project 2). The
toggle stays on for the maintainer until the redesign ships on top of it.

## Known gaps carried out of Phase 4

- A row added locally shows no price until a market event or an import
  bumps the listings cache.
- `MakePlaceImporter` and the recipe modal still write over REST; their
  rows reach the document through the socket.
- The bulk-edit Puppeteer suite (`integration/list-bulk-edit.cjs`) sends
  add-item bodies without `ListItem.id` and fails on `main` too; it is not
  in the e2e gate.
