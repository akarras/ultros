# Lists: one workspace and Make online (#1500)

## Visible journey

One Lists heading, New list action, search and card grid contain local and online
lists. Local cards say Saved on this device and offer Make online. The editor
places the same action beside its title. Connecting… becomes Online · private;
Manage access is a separate, explicit action. Restore and backup live behind
secondary controls, with no transfer explanations in the normal directory.

## Identity and durability contract

1. A device UUID is the identity; names never identify or merge lists. Making a
   list online durably records the selected account and market scope alongside
   the original IndexedDB document before a request is sent.
2. The server creates at most one private destination for account + device UUID.
   It retains the original validated CRDT history. Retries merge the same history,
   including counter edits, instead of replaying a reconstructed projection.
3. Device saves preserve the continuation record. Acknowledgement records the
   exact uploaded IndexedDB revision; a later save remains pending and resends
   from its existing device handle. Late tabs therefore cannot silently create an
   independently edited twin. Account session checks precede every send.
4. The editor follows the destination only once its local edits are saved and
   acknowledged. An old device URL performs the same drain and continuation.
   Reload/offline failures retain all original bytes and the retry identity.
   The final session check revalidates the current revision after its await.
   Uncommitted field or composer input keeps the editor open until committed or
   cancelled; already-used composer text is not a pending draft. The selected
   Build/Shop mode continues on the destination.
5. The directory represents a linked device and its destination once. A pending
   source takes precedence over the account card until its edits are acknowledged.
   Account switches never repurpose an existing binding or send to another account.
6. Old projection-only adoption receipts are not CRDT continuations. Existing
   copies get an explicit Continue online or Keep this version separately choice.
   Original recovery bytes are retained; no automatic name-based merge or deletion.
7. Making online grants no group/user/link access. Existing permission checks,
   revocation and account cache isolation remain in force.

## Evidence and limits

The user supplied the expanded transfer/backup screenshot. Fresh in-app browser
inspection of a823562d at port 53118 confirmed the signed-out directory, local
editor, signed-in mixed directory, and private online access modal. Fixtures use
only account 990000001500 and lists named UX 1500. Screenshots are in
docs/qa/lists-seamless-online-1500. Current findings: two directory/create flows;
the local editor hides its online action below the workspace; transfer mechanics
are repeated as prose; access controls show group, numeric-user and link creation
together. Screenshots alone do not certify keyboard or screen-reader behavior.

The implementation includes #1477's validated snapshot/import boundary and the
reviewed realtime retirement and sorting fixes. Final verification must use this
branch's fresh server/client build, not the audit server. The concise Labs/help
correction from #1476 is part of the same journey; its anonymous preview probe
must pass before that issue can be closed.
