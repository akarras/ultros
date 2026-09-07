# Lists Sync Phase 5: Promotion Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure the bundle, ship the changelog entry, document the sync layer for operators, and define the production soak that gates promotion out of Labs and the start of the redesign spec.

**Architecture:** No code beyond a changelog JSON and a docs page. This phase produces evidence and a checklist.

**Tech Stack:** cargo-leptos release build, `ultros-changelog` JSON entries, GlitchTip, the prod Docker host.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md`, sections 8 (bundle) and 10 (phase 5), and the Risks section.
- Expected compressed bundle growth at most 750 KB.
- A changelog entry is required for any player-visible change (`AGENTS.md`); the Labs toggle is visible in Settings, so this experiment gets one.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

### Task 1: Bundle numbers

**Files:**
- Create: `docs/lists-sync.md` (started here, completed in Task 3)

- [ ] **Step 1: Measure before**

On a checkout of `main` at the commit before Phase 1 merged:

```bash
cargo leptos build --release
ls -l target/site/pkg/ultros.wasm
gzip -9 -c target/site/pkg/ultros.wasm | wc -c
```

Record both numbers.

- [ ] **Step 2: Measure after**

On the branch with Phases 1 to 4:

```bash
cargo leptos build --release
ls -l target/site/pkg/ultros.wasm
gzip -9 -c target/site/pkg/ultros.wasm | wc -c
```

Expected: the compressed difference is at most 750 KB. If it is larger, run `cargo bloat --release --target wasm32-unknown-unknown -p ultros-app --features hydrate --no-default-features -n 30` and check that Loro's `richtext` and `tree` containers are what grew; they are pulled in by the crate regardless of use, and that is the accepted cost. Only an unexpected contributor, such as a second copy of `serde_json` or `regex`, warrants a change.

- [ ] **Step 3: Write the numbers down**

Start `docs/lists-sync.md` with:

```markdown
# Lists local-first sync

`/list/:id` behind the `lists-sync` Labs toggle runs on a Loro CRDT document
stored in the browser and merged through the server. Spec:
`docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md`.

## Bundle

| build | raw | gzip -9 |
|---|---|---|
| before (main @ <sha>) | <bytes> | <bytes> |
| after (Phase 4) | <bytes> | <bytes> |

Loro accounts for the difference; measured in isolation it is 1.99 MB raw and
703 KB compressed.
```

Replace every `<sha>` and `<bytes>` with the measured values before committing.

- [ ] **Step 4: Commit**

```bash
git add docs/lists-sync.md
git commit -m "docs: lists sync bundle numbers

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Changelog entry

**Files:**
- Create: `ultros-changelog/changes/2026-09-<day>-lists-local-first-sync-labs.json` (use the merge date)

- [ ] **Step 1: Write the entry**

```json
{
  "category": "features",
  "importance": "medium",
  "title": "Lists that live in your browser and sync everywhere (Labs)",
  "blurb": "Turn on \"Lists: local-first sync\" under Settings › Labs. Your lists are kept in the browser and synced through the server: edits apply instantly, keep working when you lose connection, and merge with everyone who shares the list, including purchases ticked at the same time on two devices. The list page also gets undo and redo: Ctrl+Z and Ctrl+Shift+Z, Cmd on a Mac.",
  "link": "/settings"
}
```

- [ ] **Step 2: Check it renders**

Run: `cargo test -p ultros-changelog`
Expected: the changelog crate's parser accepts the entry.

- [ ] **Step 3: Commit**

```bash
git add ultros-changelog/changes
git commit -m "docs(changelog): lists local-first sync under Labs

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Operator documentation

**Files:**
- Modify: `docs/lists-sync.md`

- [ ] **Step 1: Append the operations sections**

```markdown
## How it works

- One Loro document per list. The server keeps the merged state in
  `list_doc` (`snapshot`, `version`, `changes_since_compaction`) and keeps
  `list_item` and the `list` name and scope as a projection of it. Nothing
  else writes those rows: the REST handlers, the Discord bot and the socket
  all call `ListSync` (`ultros/src/lists/sync.rs`).
- The browser stores a snapshot per list under `ultros.listdoc.v1.{id}` in
  localStorage, at most 20 lists, least recently used evicted first.
- Sync rides the existing websocket. `SubscribeListDoc` carries the client's
  version vector; the server replies with a snapshot, the missing updates, or
  up-to-date, then relays every other peer's updates. Bytes are base64 inside
  the JSON frames.
- Undo is Loro's undo manager: local operations only, one-second merge
  window, 100 steps, per open page.

## Debugging

- Handshake by hand: log in with `curl -c jar "$BASE/test/login?user_id=...&username=..."`
  on a test-auth build, then `websocat -H "Cookie: discord_auth=..." ws://host/api/v1/realtime/events`
  and send `{"SubscribeListDoc":{"list_id":ID,"version":""}}`.
- A list whose page looks wrong: compare `SELECT list_id, length(snapshot), changes_since_compaction, updated_at FROM list_doc WHERE list_id = ID`
  with `SELECT * FROM list_item WHERE list_id = ID`. The rows must equal the
  document; if they do not, a write bypassed `ListSync`, which is a bug.
- Compaction: a snapshot is replaced by a shallow one after 5,000 changes or
  256 KB. Clients that start from a shallow snapshot still sync both ways.
- Bus lag: `ultros_analyzer_bus_lagged_total` with `bus="lists"` or
  `bus="list_docs"` in Grafana means subscribers fell behind; the socket
  answers with `Stale` and clients re-run the handshake.
- Browser side: `localStorage.getItem("ultros.listdoc.index")` lists the
  cached lists and their last known permission.

## Promotion

Promotion out of Labs deletes `LAB_LISTS_SYNC`, the `LabsSettings` section
when the registry is empty, the legacy `ListView`, and the REST-driven
actions it owns. It requires the soak below to pass.
```

- [ ] **Step 2: Commit**

```bash
git add docs/lists-sync.md
git commit -m "docs: lists sync operations guide

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Spec bookkeeping

**Files:**
- Modify: `docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md`

- [ ] **Step 1: Update the Risks section**

Replace the bullet beginning "The projection is correct only while nothing writes `list_item` directly" with:

```markdown
- The projection is correct only while nothing writes `list_item` directly.
  The old row-writing functions were deleted from `ultros-db/src/lists.rs`
  in Phase 3, so the compiler enforces this; `docs/lists-sync.md` gives the
  query that detects a divergence if one ever appears.
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md
git commit -m "docs(spec): row-writer invariant is enforced by deletion

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Production soak checklist

No files. Run this after the branch is deployed, over at least a week with
the toggle on for the maintainer's own shared lists.

- [ ] **Step 1: Turn it on**

Enable "Lists: local-first sync" under Settings › Labs on two browsers and one
phone, and share one list between two Discord accounts.

- [ ] **Step 2: Daily checks**

- GlitchTip: no new issues whose title contains `list_doc`, `ListDocError`,
  `sync_payload`, or `hydration` on `/list/`.
- `docker logs ultros | grep -i "list document"` shows no warnings from
  `ListSync::publish` (activity or relay failures).
- `SELECT count(*), pg_size_pretty(sum(length(snapshot))) FROM list_doc;`
  grows with use, not runaway; no snapshot above 256 KB survives a change.
- The Discord bot's `/list add_item` and `remove_item` land on the Labs page
  live.

- [ ] **Step 3: Exit criteria**

- Zero divergence between `list_doc` and `list_item` for every list touched
  during the soak (the query in `docs/lists-sync.md`).
- No data loss reported on the shared list, including a deliberate test of
  both accounts ticking the same item within a second of each other.
- The legacy page, opened without the toggle, always matches the Labs page.

When all three hold, open the redesign spec (Lists 2.0 sub-project 2). The
toggle stays on for the maintainer until the redesign ships on top of it.

---

## Self-review

- Spec coverage: section 8's bundle measurement (Task 1), the changelog rule from `AGENTS.md` (Task 2), phase 5 of section 10 (Tasks 3 to 5), and the Risks bullet the Phase 3 plan promised to update (Task 4).
- Placeholders: the `<sha>` and `<bytes>` markers in Task 1 are filled by the measurement step before commit and are called out as such; nothing else is deferred.
- Type consistency: not applicable; no code.
