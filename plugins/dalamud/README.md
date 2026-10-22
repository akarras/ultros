# Ultros for Dalamud

One plugin with browser-approved account access, item listings/recent sales, shared lists, game-chat notifications, and an optional private retainer uploader. The C# plugin and shared Rust document implementation live in this repository; no separate distribution repository is required.

## Install

Visit **https://ultros.app/plugin/** for availability and setup. The backend serves a [Dalamud custom repository](https://dalamud.dev/plugin-publishing/custom-repositories/) at **https://ultros.app/api/v1/plugin/repository.json**. Add that address under Dalamud Settings → Experimental → Custom Plugin Repositories, then install **Ultros** in `/xlplugins`. Open `/ultros` and connect using browser approval. The repository remains empty until an operator publishes a compatible release.

For migration from the development prototype, disable every old Ultros dev-plugin entry before installing. The internal name is now **Ultros**, so this is a fresh plugin installation and needs a new connection. Preserve any unsynchronized local work before removing old files. Website lists and retainer registrations stay with your account.

## Retainer price assistance

Under **Characters & retainers**, choose **Listings & prices** beside a registered retainer. The separate window can sit beside its in-game Markets window. It displays the last private capture, or cached Universalis rows if no capture exists. Enable both uploader permissions and open Markets to replace a capture; the helper does not independently detect sales or edits.

Comparisons use the same quality on the retainer's home world and exclude all retainers saved to your account. Suggestions are one gil below the lowest other price, with a one-gil floor. Unknown, future-dated or more-than-30-minute-old market reviews do not produce suggestions; displayed suggestions also expire while the window stays open. There may be better pricing strategies, and cached data cannot guarantee a sale.

Market review time means Ultros last successfully reconciled the complete item board on that world. Sales and individual listing events do not renew it. It is not the seller's upload time; existing cached boards have unknown age until their next full refresh.

Use **Copy** to copy a unit price, then enter and confirm it in game. No game actions or price changes are automated. Selecting an item opens its market view using that retainer's world. Retainer reading can be approved without registration or upload permission after the retainers have been saved.

## Build and load

Requires Windows, PowerShell 7, .NET 10, the repository's Rust/MSVC toolchain, and Dalamud API 15 references. The SDK normally finds them in the XIVLauncher Dalamud dev directory.

~~~powershell
pwsh -File ./plugins/dalamud/build-plugin.ps1
# Release with explicit localhost support:
pwsh -File ./plugins/dalamud/build-plugin.ps1 -Configuration Release -EnableLocalhost
# Optional references: -DalamudHome 'C:\path\to\Dalamud'
# Managed/backend-only iteration with the game open (Rust code and ABI unchanged):
pwsh -File ./plugins/dalamud/build-plugin.ps1 -Configuration Release -EnableLocalhost -KeepInstalledNative
# Validate a complete release without changing the development installation:
pwsh -File ./plugins/dalamud/build-plugin.ps1 -Configuration Release -StageOnly
~~~

The script tests in a separate staging directory, then updates **artifacts/dalamud/dev/Ultros.dll** and **artifacts/dalamud/dev.zip** on every build. The development path stays fixed for Dalamud hot reload. Debug includes localhost support; ordinary Release accepts only the fixed production origin.

`-KeepInstalledNative` validates the installed native package and runs the managed interop checks against that exact library. Use it only when the Rust CRDT/FFI implementation is unchanged. A full build publishes the newly compiled native library; the shadow-copy loader keeps that installed source file replaceable.

Add the printed Ultros.dll path under **Dalamud settings → Experimental → Dev Plugin Locations**, enable **Ultros**, and run **/ultros**. **/ultroslab** opens the local merge experiment in localhost-enabled builds only. Keep every packaged DLL/JSON together, including capture-compatibility.json. Use this fixed `/dev/` path throughout iteration.

Rebuild in the same configuration for C# iteration; the script publishes dependencies and compatibility metadata before replacing the watched plugin DLL. At runtime, the release client copies its tested Rust library into a hash-checked cache inside its configuration directory before loading it. This keeps the installed source DLL replaceable during updates, while existing documents keep their original module alive through finalization. Old prototype installations loaded that source DLL directly: exit the game once if they still lock it. No timestamped or hashed installation folders are created.

## Publish a release

The single version source is `Directory.Build.props`: `UltrosPluginVersion`, a four-part numeric version (`major.minor.patch.revision`), with `UltrosPluginReleaseNotes` for the installer changelog. Feature additions increment minor; compatible fixes increment patch. Protocol 1 and native ABI 2 are separate compatibility contracts and change only when those contracts change.

1. Update the version, metadata and player changelog, run the root `./check_ci.sh`, and commit the release sources.
2. Run `pwsh -File plugins/dalamud/build-plugin.ps1 -Configuration Release -PublishRelease`. This builds/tests a fresh native library, checks managed/native versions and emits an immutable ZIP plus metadata under `artifacts/dalamud/releases/<version>/`. It also runs the native shadow-copy update test. Localhost-enabled, dirty-source and reused-native builds cannot be published. Publishing does not replace the live development installation.
3. Copy the complete version directory to the release volume, then atomically replace `current.json` last. Mount that volume read-only in the web container and set `ULTROS_PLUGIN_RELEASE_DIR` to its container path. Keep old version directories for immutable download URLs.
4. Check `/api/v1/plugin/release`, `/api/v1/plugin/repository.json` and the advertised ZIP. The index includes the source commit, SHA-256, byte size, protocol, native ABI and Dalamud API level. Reusing a version with different package content is rejected; bump the version instead.

Set `ULTROS_PLUGIN_MINIMUM_VERSION` (default `1.0.0.0`) when a release must replace older clients. Every game API request carries `X-Ultros-Client-Version` and `X-Ultros-Protocol-Version`; missing, malformed, old or incompatible values receive HTTP 426 with an update address. These labels are compatibility checks; scoped bearer authentication remains required independently. Capabilities, installation downloads and revocation remain available to outdated clients. Publish a usable release before raising the minimum.

## Localhost mode

Use a development database and the normal Ultros environment configuration:

~~~powershell
pwsh -File ./plugins/dalamud/serve-localhost.ps1 -Port 8080
~~~

This enables server feature plugin-localhost, sets ULTROS_PLUGIN_ENABLED=true, and binds to loopback. In the plugin's **Settings**, enable **Target localhost development server**, enter the exact origin (such as http://localhost:8080), save, and reload. Hostname and port must match the server's HOSTNAME. Production and localhost have different credential/cache namespaces.

Optional server switch **-TestAuth** enables the repository fixture login. On an isolated local server, visit /test/login?user_id=7777777777777&username=LocalPlayer&redirect=/plugin/verify to bypass Discord. Otherwise use ordinary Discord login. The helper does not change your database or Discord configuration.

Production connections default off. Enabling ULTROS_PLUGIN_ENABLED=true requires HTTPS HOSTNAME and separately provisioned, stable 32-byte hexadecimal ULTROS_PLUGIN_PAIRING_KEY and ULTROS_PLUGIN_IDENTITY_KEY secrets. Localhost derives separate origin-bound development keys from the existing cookie key. Preserve the identity key across deployments.

## Website setup guide

Open **/plugin/** on the same Ultros server as the game client. **/plugin/verify** handles connection review; **/plugin/devices** manages and disconnects clients. These pages use the website layout, with ads and analytics disabled while connection controls are open. Installation documentation is integrated into the same Leptos route.

Development ZIPs stay local. The web server advertises only indexed release packages from `ULTROS_PLUGIN_RELEASE_DIR`; it never accepts a client-supplied filesystem path. Localhost defaults to `artifacts/dalamud/releases`. An unavailable or incompatible release produces an empty repository and no advertised download.

## Try it

1. **Account:** label the client, choose optional permissions, connect, and open the integrated Ultros connection page. The direct link opens permission review after sign-in; compare the account and short code, then approve. For phone sign-in, use the displayed bare verification address and short code instead. The direct link uses its own 256-bit fragment token, distinct from polling/access/refresh credentials. The website removes it from the address bar immediately and preserves the request across login with an encrypted HttpOnly cookie. Links expire after five minutes and stop working after approval or decline. Extra permissions require browser review. Revoke clients at /plugin/devices.
2. **Item:** right-click an item or recipe and choose **Search Ultros** for the game window, or **Open Ultros website** for the browser. **Add to [active list]** adds one item with any quality and opens the list for feedback. The Item tab also has an add button. Without a writable selection, it opens Lists so you can select a list or enable list editing in Account. A queued add stays bound to the account/device/list shown when its menu opened. Search also accepts item names. The price zone defaults to the current world's region, falling back to the home region; enter a different region/datacenter/world to override it. Purchases stay manual.
3. **Lists:** create an **online list** on Ultros, or choose **Make online** for a device list. The tab shows **Lists - [active list]**. An existing Cart is selected by default (your own Cart takes priority); later selections are remembered per account/profile/origin. Refresh/select another list to edit its rows or mark quantities acquired. Concurrent Acquired deltas add; Need is last-writer-wins. Numeric edits commit on Enter or leaving the field. Market estimates use the website's shared Rust calculator and allow partial stacks.
4. **Characters & retainers:** visit a summoning bell and click **Register all observed retainers**. A persistent saved count confirms registration. This creates missing retainer records and groups them under the unverified character in the website's normal Edit, All Listings, and Undercuts views. Existing verified assignments and market metadata are preserved. Observed identities have their own namespace and do not verify Lodestone ownership.
5. **Uploader:** in **Characters & retainers**, click **Enable listing uploader** and allow uploads on the character. Both switches are shown together and must be on. Settings has the same uploader switch; enable it again after a game build change. Open a retainer's selling list. The window's initial setup/data update and market inventory updates can trigger capture; responses received just before the window becomes readable are briefly retained for that same character/retainer/object. A full loaded container, matching roster count, and 500 ms of stable data are still required. Uploaded rows appear immediately in your website listing views as **Awaiting confirmation**, with the capture time. They stay private; only Universalis writes public listings. Matching public stacks replace the pending rows one at a time. A fresh, loaded empty list clears the previous private view. Capture status distinguishes missing updates, incomplete counts, invalid prices and unchanged snapshots. Status changes also appear in the Dalamud log without raw game IDs or credentials.
6. **Notifications:** messages appear in game chat with an **[Ultros]** prefix. Settings offers Echo, System messages or Notices, an optional popup, and optional sound effects 1–16 with a local test. Enable that category in the desired game chat tab's filters. **Notification history** retains recent messages, endpoint testing and mark-read controls. Acknowledgment and marking the website inbox read are separate actions.

After pairing, a bearer-authenticated SSE connection wakes the durable notification fetch immediately for events on that server. It reconnects automatically; scoped HTTP fetch/ack and 60-second polling recover missed or cross-instance signals. Unpaired clients do not open a notification stream. Remote list changes still poll up to 60 seconds; local edits sync immediately.

### Optional inventory progress

In Settings, enable **Automatically mark new inventory items acquired** for the active writable list. It defaults off and requires matching packaged client structs and explicit re-enablement after a game build change. A complete, stable snapshot of player bags, crystals, armoury and equipment establishes the baseline; existing items are not counted. Later positive net gains fill exact-quality rows first, then any-quality rows, up to each row's remaining need. Bag/equipment moves do not count, and consumption does not reduce progress. Transfers from retainers/saddlebags into the tracked inventories count as incoming items.

Changing list/account/character, disabling the option, loading incomplete inventory or entering recovery discards the old observation context and queued work. Deltas run on the same background document worker as manual edits and use the CRDT acquisition counter. Inventory snapshots and character identifiers remain in memory; only matching list progress is saved/synchronized. Transient gains consumed between stable observations can be missed; manual progress controls remain available.

## Credentials and saved work

- Discord credentials never enter the plugin. Access tokens stay in memory. Rotating refresh tokens use verified Windows CurrentUser DPAPI and a user-only directory ACL. Wine/unverified storage uses memory only. This does not isolate credentials from other code running as your OS user or inside the game.
- Refresh expires after 30 idle days/90 absolute days. An uncertain refresh is not retried; the client attempts revocation and requires reconnecting. Tokens never enter URLs, logs, Rust, clipboard or plugin configuration.
- Use a distinct profile per simultaneous game process. Exclusive locks protect credentials/documents. Connected carts are keyed by origin, profile, account and list. Server authorization is checked again for every operation.
- Rust owns recovery; C# writes atomic checkpoints before sending pending work or resuming recovered edits. Save failure pauses edits/sync. Recovery conflicts retain both copies; **Keep server copy** backs up the original local document. Explicit list permission denial removes its cached checkpoints; login/network failure preserves unsent work.
- Uploads are private, consent/revision checked and receipt-idempotent. Open All Listings/Undercuts pages check for private captures every five seconds while visible, alongside existing public market events. The in-memory capture queue holds at most 10, expires after 10 minutes and is invalidated when the visit/module stops. Raw content IDs are HMACed before database storage and never persisted in a local upload queue.
- Notifications are delivered at least once. A crash around toast acknowledgment can repeat one. Alert acceptance and destination queues commit together; external destinations retry transient failures.
- The local lab saves Cart A under merge-lab-carts/PROFILE/cart.loro. Peer B is scratch state. cart.loro.before-merge preserves the previous cart; unload retries dirty saves and attempts .unsaved-recovery. Restore backups with the plugin unloaded.

## Limits and validation

ABI 2 uses checked integer handles, immutable responses, caller-owned buffers and an allocation-free close export. Unwinding panics are caught and poisoned state fails closed; native faults, aborts and arbitrary OOM cannot be contained.

Both `ultros-list-doc` and `ultros-list-doc-ffi` deny production Clippy lints for
`unwrap`, `expect`, explicit panic/todo/unimplemented/unreachable, unchecked
indexing/string slicing, arithmetic side effects and process exit. The shared
document crate forbids unsafe code; FFI pointer operations require explicit
unsafe blocks and safety comments. The ABI refuses to compile with `panic=abort`.
Root `./check_ci.sh` runs these lints, including a non-test library build; fixture
assertions remain available in tests.

Three narrowly scoped, documented `expect_used` exceptions retain existing
infallible APIs: the legacy `new` and trusted `from_rows` constructors (fresh
attached maps with the pinned Loro), and integer-only postcard version-map
serialization into a growable buffer. Native initialization uses `try_new`;
production row import uses `try_from_rows`, which propagates errors and rejects
duplicate keys. These lints do not inspect dependency internals or prove freedom
from every possible panic. Dependency unwinds still rely on the ABI catch boundary.

Resource guards include 8 MiB snapshots, a 32 MiB decompression budget, 16 MiB per decoded block and 100,000 retained operations. Local edits are checked before mutation, so accepting an edit cannot create a checkpoint that exceeds those limits. Undo reserves conservative capacity and can refuse before consuming its step. Clean, exactly acknowledged history compacts at 50,000 operations and waits for durable storage before edits resume; pending work is preserved. There is no lifetime peer-count ceiling and causal identities are never discarded. Encoded transport limits still apply, including 8 MiB of base64 causal-version text and a 24 MiB combined native/plugin list-sync envelope. Other plugin HTTP responses remain capped at 12 MiB. The browser realtime WebSocket permits a 24 MiB aggregate message or single frame for the combined list-sync envelope; the separate alerts WebSocket is unchanged. The pinned decoder checks growth before extending buffers. The 256-row soft add/lab-import cap does not prevent reopening existing merged/undo states.

Connected document/HTTP/disk work runs on one background worker. Game memory is copied on the framework thread. The local merge lab remains synchronous.

The artifact targets Windows x64 inside the game's .NET 10/CoreCLR process. Standalone Windows tests do not establish in-game behavior or Wine/macOS portability; see [portability and the possible managed WASM experiment](PORTABILITY.md). Capture checks the packaged client-structs hash and disables itself after a game build change until explicitly enabled again.

If capture reports **Dalamud client structs changed**, rebuild with `-DalamudHome` pointing at the installed Dalamud version, then load the new package. A Dalamud update can invalidate an older package even if the game build is unchanged. Keep the generated compatibility file with the DLLs; replacing only that file would bypass the check without rebuilding the reader.

~~~powershell
dotnet run --project plugins/dalamud/Ultros.Lists.Smoke -- --client-only
# test-auth localhost server + exact packaged native directory:
dotnet run --project plugins/dalamud/Ultros.Lists.Smoke -- --http http://localhost:8080 C:\path\to\package
$env:BASE_URL='http://localhost:8080'
node integration/dalamud-device-flow.cjs
~~~

Run the required root ./check_ci.sh after code changes. PostgreSQL integration tests require an explicitly isolated test URL; see ultros-db/src/plugin/tests.rs.
