# Ultros game connections and token security

Updated 2026-10-19. This describes the connected implementation and its limits, rather than an independent security certification. Setup lives at `/plugin`; installation/release instructions are in [the plugin README](../plugins/dalamud/README.md).

## Browser login and game approval

Discord OAuth signs the player into the website. The plugin never receives Discord credentials, a Discord client secret or the website session cookie. It receives a separate, scoped Ultros grant after browser approval.

1. The plugin creates a short-lived challenge with a label and requested scopes. The server returns a high-entropy polling credential, a separate 256-bit browser-selection credential and a short matching code.
2. The complete URL uses `/plugin/verify#pair=…`. Its fragment is removed with `history.replaceState` before app startup and exchanged through a same-origin POST for an encrypted, five-minute HttpOnly cookie. It is not a polling/access/refresh credential. Browser sign-in and explicit approval are still required.
3. A phone can use the bare verification page and short code instead. The code fingerprints the longer selection credential; it is not used as an access token. Challenges expire after five minutes and consume once.
4. The browser checks the signed-in account, requested scopes and matching code. Cookie/account binding, exact Origin, CSRF proof, a selected challenge and a single-use approval nonce protect approval. Selection cannot change accounts or scopes during redemption.
5. The plugin redeems the polling credential through a POST body. Approval/redemption and refresh rotation are transactional; replay does not mint a second credential family.

Only approve a request started in your own game. A label and matching code do not prove that arbitrary client software is trustworthy. The complete-link/manual-code interaction follows the purpose of [RFC 8628 §3.3.1](https://www.rfc-editor.org/rfc/rfc8628.html#section-3.3.1), with a separate high-entropy browser-selection credential.

The Leptos setup, consent and client-management pages use the normal site shell with third-party scripts omitted, a restrictive CSP and private/no-store responses. Consent secrets live in client-side resources/actions, not SSR resources, browser storage or URLs. Pairing links use a dedicated launcher because Dalamud's general link helper can log a failed URL.

## Credentials and storage

- Opaque access/refresh credentials have separate kinds, random identifiers and random secrets. The server stores validation digests rather than usable token secrets and checks the current account/grant, scope, expiry and revocation state.
- Access credentials stay in memory and expire after fifteen minutes. Refresh credentials expire after thirty idle days and ninety absolute days. Refresh rotates credentials; reuse revokes the family. An uncertain refresh response is not automatically retried; the client attempts revocation and requires reconnecting.
- Verified Windows storage protects refresh credentials with CurrentUser DPAPI and a current-user-only directory ACL. Ordinary plugin configuration and support exports contain no token. Wine or unverified storage uses memory-only credentials and reconnects after restart; no plaintext fallback is provided.
- Profiles use exclusive locks. Use a separate profile for each simultaneous game process. Files are partitioned by origin/profile/account/list so reconnecting another account cannot send the prior account's saved work.

The dedicated HTTP transport disables cookies and automatic redirects and uses ordinary TLS validation. Production builds target the fixed HTTPS Ultros origin. Explicit developer builds can target loopback HTTP in a separate credential namespace; use an isolated development database. Deployment keys for pairing and private game identities are separate, stable secrets. Preserve the identity key across deployments.

Authorization, cookies, raw credential responses, pairing credentials and launch exceptions must stay out of diagnostics. Error formatting is deliberately generic at transport/storage boundaries. Rust receives document operations and bytes, never account credentials.

## Authority and private game data

Notification delivery/read state, list reads/writes and retainer reads/registration/uploads have distinct approved scopes. Dependencies are checked as a set. Registration, list editing and uploads require explicit choices; private uploads also require the module switch and per-character consent.

Game APIs authenticate bearer proof independently from browser cookies. Version/protocol headers are compatibility labels, not authorization. Missing/unsupported versions receive HTTP 426; capabilities, public release downloads and credential revocation remain available. See [release metadata and gates](../ultros/src/web/plugin/releases.rs).

Mutations revalidate grant authority inside their database transaction. Account/grant locks order revocation against writes; list writes also lock their actual permission evidence and use the same CRDT merge path as other clients. Permission denial cannot be bypassed with a relational list-row write.

Observed characters are unverified game observations, not proof of Lodestone ownership. Private retainer uploads are owner-bound, consent/epoch checked, bounded and receipt-idempotent. They never write `active_listing`; only Universalis supplies public listings. Removing private snapshots or revoking consent must not make those observations public.

## Notifications and revocation

The implemented game transport is bearer-authenticated HTTPS/SSE plus durable HTTP fetch/ack/read. **Game bearer tokens are not accepted by the browser market WebSocket.**

SSE carries only wake-ups, not event text or credentials. It rechecks the grant before each wake-up and on its sixty-second timer; connections also end after two minutes and reconnect. Scoped HTTP fetch checks current authority independently. A sixty-second polling fallback recovers missed or cross-instance wake-ups. A revoked idle stream may remain open until its next check, but it cannot authorize a new protected fetch.

Browser client-management controls revoke one/all clients. Revocation disables the game destination and cancels pending deliveries; explicit disconnect attempts server revocation and clears local credentials. Previously delivered data cannot be retracted. Ordinary website logout and changing FFXIV characters do not revoke an independently approved installation; account deletion invalidates its authority.

Local CRDT checkpoints remain bound to their original account/origin. Network/session failures preserve unsent work; explicit resource permission denial removes that resource's cached checkpoints. Recovery preserves both copies and pauses edits when persistence or reconciliation needs attention.

## Validation and remaining limits

[Database integration tests](../ultros-db/src/plugin/tests.rs) cover pairing/consent, rotation/replay, scoped resources, private data, revocation and list permission behavior against isolated Postgres. Browser tests cover direct-link cleanup, HttpOnly continuation through sign-in, phone fallback, consent/denial, replay and account isolation. [Managed integration checks](../plugins/dalamud/Ultros.Lists.Smoke/HttpChecks.cs) exercise real browser approval, protected HTTP, concurrent CRDT checkpoints, SSE reconnect/recovery and disconnect. Release checks cover minimum versions and immutable downloads. Run the required root `check_ci.sh` plus the documented browser/managed checks before shipping relevant changes.

DPAPI does not isolate credentials from other plugins or malware in the same game process or OS account. A compromised client can use its approved authority; a compromised server is a different boundary from a leaked database digest. Scope limits, expiry, rotation and revocation reduce exposure without making the client a trusted enclave. See [portability and the possible WASM experiment](../plugins/dalamud/PORTABILITY.md) for the document-runtime boundary.

Windows validation does not establish Steam Deck/Proton or XIV on Mac compatibility. In-game behavior and those environments still need player testing.
