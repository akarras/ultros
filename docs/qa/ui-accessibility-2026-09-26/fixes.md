# UI accessibility follow-up

This branch implements the actionable findings from the September 26 audit against current main (`3a2eed1a`). The original audit checkout was older; main already included modal keyboard work and translated chart summaries. These changes extend those implementations.

| Audit area | Change |
| --- | --- |
| Collapsed navigation | Keep link names accessible; reveal rail labels on focus/hover; name the home link. |
| Dialogs and mobile menu | Shared native modal surface provides an inert background, contained focus, Escape dismissal and opener restoration; portalled pickers and tooltips stay inside their owning dialog. |
| World pickers | Named input API, unique popup/option IDs, selected input value before/after hydration, popup ownership, no-match feedback and focus-preserving selection/cancellation. Queued selection callbacks ignore blurred or disposed inputs. |
| Mobile search | Persistent localized Close action with a 44px minimum target. |
| Retainer ordering | Move up/down buttons, stable item identity, position announcements and drag handling confined to the grip. |
| Forms | Label associations for list name/world and retainer character; announce alert/endpoint errors and associate them with inputs. |
| Mobile analyzers | Opt-in result cards with expandable detail, live row updates, progressive loading and a full-table switch; recipe assumptions collapse behind an editable summary. |
| Missing sell world | Explicit selection prompt and current buying scope beside the analyzer. |
| Tooltip help | Keyboard-focusable short explanations, associated descriptions, pointer travel into overlays, Escape dismissal and touch access. Confidence sample help uses this shared behavior. |
| Notifications | Routine messages use polite status; errors use alert; warning/error messages persist; interacting with a timed message makes it persistent. |
| Connection state | Accessible text remains at every width; distinct mobile symbols supplement color; a stable live region announces the state independently of the elapsed-time counter. |
| Currency cells | Non-interactive gil icons remove repeated decorative buttons and nested interactive elements. |
| Chart inspection | Keyboard-only, debounced point announcements plus a table of time buckets, visible-series prices, total volume and listing floor. |
| Shell navigation | Skip link and focusable main; pathname transitions focus main, while query-only changes retain focus. |
| Regression coverage | Deterministic hydrated fixture and browser checks; semantic status tokens checked across twelve palettes in light/dark; new UI strings in all seven locales. |

## Validation

The work was checked on Windows against a dedicated loopback server from this worktree, with `test-auth` enabled and websocket ingestion disabled. Native debug symbols were omitted to reduce local build memory. The browser bundle retained the normal development configuration.

- `./check_ci.sh`: passed on the final Rust source, including the SSR picker value adjustment.
- `cargo leptos build`: final combined frontend and server build passed with `test-auth`.
- `node --test integration/*.test.cjs`: 167 passed.
- `test:accessibility`: passed with the unmodified test, including dialog/popup ownership, rapid keyboard navigation, typing, focus restoration, persistent errors, mobile menu/search containment, 320px result cards, live row updates, full-table switching, progressive loading, all visible fields, and empty results. The final run also passed the no-JavaScript SSR selected-world assertion.
- `test:list-modal-keyboard`: passed on the final build, including nested dialogs, access/settings dialogs, mobile notification layout, and detached-opener cleanup. The test explicitly starts at its intended desktop viewport before switching to mobile.

The full standard `scripts/run_e2e.sh` run completed with failures and is **not** reported as green. It passed eager-search cancellation/retry, FC material detail layouts, item responsive geometry, market-window races, recipe planning, account storage, device-list keyboard undo, device-list card editing, shopping companion, and the new accessibility suite. Remaining broad-harness failures included:

- Item-navigation assertions omit the existing `#item-verdicts` and `#bulk-basket` shortcuts.
- Grid/menu scripts expect older filter classes, operator `<select>` controls, or menu text that do not match the unchanged current grid components.
- A Lists handoff fixture expected zero priced items, while this local database produced one priced item.
- Item-history requests returned HTTP 500. The local ClickHouse service separately reported that its sales table could not attach because of broken parts.

The picker world-route assertions were updated to inspect the actual input value rather than the removed decorative text overlay. The targeted recipe rerun passed SSR path precedence, world changes across regions, reload/back/forward navigation, and a non-Latin world name before stopping at the old `.grid-column-filter` selector. The FC rerun passed SSR selection before stopping at a missing `analyzer-price-scope` marker in the unchanged calculation component. Neither complete world-route suite is reported as passing. The failures above remain limitations of the recorded broad validation.

## Screenshots

The before captures came from the public build at the audit baseline. After captures use the local development data, so item names, counts, and prices differ; compare layout and interaction affordances.

| Surface | Before | After |
| --- | --- | --- |
| Mobile search | [Before](before-search-mobile.png) | [Visible Close action](after-search-mobile.png) |
| Mobile recipes | [Before](before-recipes-mobile.png) | [Scope notice, assumptions and compact results](after-recipes-mobile.png) |

[320px deterministic card fixture](compact-results-320.png) verifies the narrowest automated layout check.

## Manual follow-up

Automated DOM/keyboard checks are not a screen-reader certification. Before a release, spot-check NVDA/VoiceOver announcements, physical iOS/Android keyboards, forced colors, and 400% zoom. The chart table exposes the shared time-bucket aggregates; it is not a transcription of every density bin or candlestick field.

## Merge review (2026-09-28)

The review resolved the `integration/package.json` conflict with current main while retaining both accessibility and analyzer-action test commands. It also found a modal notification regression: native `showModal()` makes the app-level toast container inert, so errors raised from a dialog could not be read or dismissed there. `DialogSurface` now renders the shared notification state inside the modal. The new browser regression checks the accessibility tree, keyboard focus, and dismissal of a modal error.

The review also found redundant keyboard stops on tooltip wrappers around and inside native controls. Standalone help remains focusable, while wrappers containing or inside a native control defer to that control. The analyzer-action probe now covers this regression and recognizes the mobile card layout without assigning grid semantics to cards.

Validation of the repaired tree:

- Client compilation, JS/WASM generation, and the native server build passed with `test-auth`.
- The focused accessibility suite and real list-dialog keyboard suite passed, including the added modal-error regression, nested dialogs, focus restoration, and mobile notification sizing.
- The complete `./check_ci.sh` gate passed after both repairs, including the feature-gated CSV tests and reactive-graph regressions.
- All 170 JavaScript regression tests passed.
- Analyzer inline actions passed across ten routes at desktop and mobile widths, including the new redundant-tab-stop assertion and copy/dialog behavior. The assertion failed on the preceding build and passed after the shared tooltip fix.
- The broad route assertions were updated for the existing verdict/basket anchors, the compact `5% tax` summary, and the signed-out `User groups` heading. Desktop and mobile passes succeeded with the updated assertions. The wide pass exposed the separate metadata race described below.

The full E2E driver is not represented as green. The review also ran it on the preload PR, which does not contain the accessibility changes. Existing analyzer filter/operator/menu selectors, list/group navigation or persistence checks, and fixture-dependent assertions failed there too. Empty sale-statistics data initially caused intentional HTTP 503 responses; a minimal synthetic fixture in the disposable local database allowed strict route checks to exercise initialized-market behavior. Priced Lists acceptance and the production soak were not performed by this review.

A repeated-load probe additionally found intermittent missing social metadata in the server HTML for `/cookie-policy`, followed by a client metadata hydration panic. The omission reproduced on both independently built branches (including one response in a 95-request preload-build probe). `social_meta.rs` is unchanged from main in both branches. This existing race remains unresolved and is not hidden by a successful retry. The focused modal repair does not change that metadata implementation.
