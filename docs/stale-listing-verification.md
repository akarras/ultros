# Stale-listing verification (#1178)

Part of [#1178](https://github.com/akarras/ultros/issues/1178). This is a
read-only diagnostic, not a repair or evidence that the issue can close.

## Measurement: 2026-09-08

**Listing-floor drift persists.** On Jenova (`world_id=40`), item `5669`
had nine surplus Ultros listing signatures and an NQ floor of **5,000 gil**
in both its board and analyzer cache, versus **500,000 gil** on Universalis.
The discrepancy was unchanged in the follow-up more than six minutes later.
No HQ stock was present on either side. Do not close #1178 from this evidence.

The inspected checkout was
`0a9fbdb268f3657a8789eb0fff694f91d0fbf637`; every captured Ultros board and
analyzer response reported production build `0a9fbdb`. Captures in UTC:

| Capture | Start | End | Selection |
| --- | --- | --- | --- |
| `round1` | 21:50:24.035 | 21:50:35.310 | Ten recent uploads, plus Cloth/Caligae of Legend |
| `recent1` | 21:53:06.909 | 21:53:11.111 | Six items with trades at 21:32–21:48 |
| `followup` | 21:59:31.189 | 21:59:38.813 | Same 18 items |

[Machine-readable measurements](evidence/2026-09-08-stale-listing-verification.json)
include separate NQ/HQ prices, counts, recent sale prices/times, ingest
markers, review times, request URLs, headers, and raw-response hashes.
The capture artifact directory is recorded there. No live events, private
metrics, or reconciliation execution logs were captured.

Follow-up prices are unit gil. `—` means no stock of that quality, not
failure. `U/L` counts mean Universalis/Ultros. The Ultros and analyzer
columns are combined here because they agreed for every sampled quality.

| Item | Universalis NQ / HQ | Ultros and analyzer NQ / HQ | U/L rows | Remaining content discrepancy |
| --- | ---: | ---: | ---: | --- |
| 21713 | — / — | — / — | 0/0 | None |
| 35034 | 19,998 / 24,999 | 19,998 / 24,999 | 9/9 | None |
| 49303 | — / 470,357 | — / 470,357 | 8/8 | None |
| 47173 | — / 375,994 | — / 375,994 | 9/9 | None |
| 47183 | — / 375,995 | — / 375,995 | 8/8 | None |
| 44051 | 484 / 568 | 484 / 568 | 60/60 | None |
| 47172 | — / 395,985 | — / 395,985 | 15/15 | None |
| 47182 | — / 145,992 | — / 145,992 | 14/14 | None |
| 5342 | 90 / — | 90 / — | 11/11 | None |
| 47170 | — / 375,994 | — / 375,994 | 9/4 | Five missing Ultros listings above the floor |
| 50830 (Cloth of Legend) | 53,419 / — | 53,419 / — | 29/29 | None |
| 50256 (Caligae of Legend) | 259,990 / 249,000 | 259,990 / 249,000 | 4/4 | None |
| 44061 | 4,000 / 11,989 | 4,000 / 11,989 | 22/22 | None |
| 17954 | 28,796 / — | 28,796 / — | 16/16 | None |
| 5669 | **500,000 / —** | **5,000 / —** | **1/10** | **Nine surplus Ultros listings** |
| 44012 | 800 / 5,000 | 800 / 5,000 | 62/64 | Two surplus NQ listings at 1,891 gil |
| 4422 | 2,000,000 / 10,004 | 2,000,000 / 10,004 | 21/21 | None |
| 8161 | 6,000 / — | 6,000 / — | 5/5 | None |

Item `5669`'s recent NQ sales included single units at 10,000 gil
(21:37:06 and 21:37:07) and 10,200 gil (21:37:09). Those sale prices are
historical context, not substitutes for the current board floor. Its
Universalis upload time was 21:41:48.961; Ultros's ingest marker was
21:41:49.087. Thus the marker looked current relative to that upload while
the board disagreed. At 21:54:03, Universalis's aggregate world floor also
reported 500,000 gil, excluding a board-versus-aggregate disagreement in
that observation. The 200-item Jenova recency window captured immediately
afterwards began at 21:44:02.854 and omitted `5669`.

All follow-up upstream brackets were stable. The first `44061` bracket
changed: one HQ signature at 3,999 gil disappeared between the upstream
reads, and it was absent from the intervening Ultros response and the
follow-up. This is consistent with removal reaching the board, but without
an event trace it does not establish websocket delivery latency or causality.
There were no further upstream signature disappearances between the first
capture's ending board and the follow-up for any sampled item. The nine
`5669` surplus signatures, two `44012` surplus signatures, and five `47170`
missing signatures remained; no reconciliation of those discrepancies was
observed. Waiting across the nominal interval does not prove a pass ran.

Six otherwise matching boards in the initial twelve-item sample also had
retainer-city metadata differences covering 22 rows; these were kept separate from stock discrepancies. For
example, Cloth of Legend matched all 29 listing signatures despite an
oldest Ultros review timestamp of August 10. Old review age alone would
misclassify that matching stock as drift.

Ten offline probe-classification tests cover evidence semantics. Runtime
cause and production convergence remain unresolved; this change supplies a
diagnostic rather than an ingest repair. No server was started, no
refresh/sweep was requested, and no production mutation occurred.

## Reproduce a bounded observation

Use Python 3's standard library. Choose one world and at most 20 item IDs;
retain the same sample for the follow-up. For example, this sample includes
Cloth of Legend (`50830`), Caligae of Legend (`50256`), and an item with a
reproduced price discrepancy (`5669`):

```sh
python3 scripts/probe_stale_listings.py \
  --world Jenova --world-id 40 --items 50830,50256,5669 \
  --output /tmp/ultros-board-before
```

After at least six minutes, run the same command with a new output directory.
This spans the *nominal* five-minute catch-up interval; it does not prove
that a catch-up pass ran, finished, or included these items. The service
visits worlds sequentially and a pass can take longer than five minutes.
Do not use refresh buttons, `/item/refresh/...`, `/rescan_market`, purchases,
or sweeps to produce the result.

Each capture makes `item count + 3` public GET requests: a complete
Universalis board batch, one Ultros analyzer floor snapshot, individual
Ultros boards, and the Universalis batch again. There are no retries.
Requests have a 30-second timeout and an 8 MB response limit. The output
directory must be new, preventing accidental overwrites. Each response
records URL, UTC start/end, HTTP/cache headers, `x-ultros-commit`, and body
or failure. `report.json` records the local inspected revision separately
from observed production revisions. Keep the capture files with the report.

For sample selection, use
`https://universalis.app/api/v2/extra/stats/most-recently-updated?world=Jenova&entries=10`
for recently uploaded boards, or
`https://ultros.app/api/v1/recentSales/Jenova` for recent trades. These are
different populations: a recent upload is not proof of a recent sale.
Confirm selected trades against Universalis `recentHistory` and retain
their quality, unit price, quantity, and timestamp.

## Interpret the evidence

- Compare the exact item, world, and quality. `null` means no listing of
  that quality in a successful response, not a zero price. HTTP failures,
  malformed responses, wrong worlds, and truncated upstream boards are
  unavailable evidence. They cannot establish an empty board.
- Compare raw unit prices, excluding taxes and stack totals. Analyzer
  floors come from `/api/v1/cheapest/{world}`; Postgres-backed boards come
  from `/api/v1/listings/{world}/{item}`. Matching these supports the shared
  listing-price input used by recipe pricing, but does not independently
  validate a rendered recipe's full ingredient total or selected cost basis.
- Listing comparison uses a multiset of `(retainer name, hq, unit price,
  quantity)`, so duplicate identical listings retain their multiplicity.
  Ultros's public listing IDs are internal database IDs, not Universalis
  listing IDs. Retainer names are only a proxy for identity: repricing,
  relisting, and ambiguous names require further investigation.
- Retainer city is compared separately when the content multisets agree.
  City metadata differences must not be called phantom stock. Review times
  are also excluded from identity. Ultros preserves listing review time
  separately from its per-world/item ingest marker; neither an old review
  nor a fresh marker establishes that all removals arrived.
- A stable upstream bracket means the two captured content multisets
  agree. It reduces ordinary market-race ambiguity, but cannot exclude
  upstream replica inconsistency or an intervening change that reverted.
  A moving bracket is labeled explicitly; repeat it before attributing drift.
- Surplus rows are candidates, not evidence of a specific missed websocket
  event. Equal cheapest prices can coexist with non-cheapest phantom stock
  or missing listings. Compare whole boards as well as floors.
- Exit status zero means capture/comparison succeeded, even if prices
  disagree. It is never a health check or an issue-closure signal.

Offline classification checks:

```sh
python3 -B -m unittest discover -s scripts -p test_probe_stale_listings.py -v
```

## Follow-up evidence needed

The public API does not expose Universalis listing identity on Ultros rows
or the actual catch-up execution history. Correlating a disappearance with
an ingest operation needs a bounded passive websocket capture plus
read-only production logs/metrics (or a read-only identity-bearing database
snapshot). No such production log/metrics/database connection was available
in this task. The metrics listener is separate from the public application
router; see [ingest observability](ingest-observability.md).

Useful existing metrics include `ultros_catchup_price_drift_items`,
`ultros_catchup_price_drift_probe_failed`, `ultros_catchup_items_recovered`,
`ultros_sweep_chunks_failed`, and `ultros_world_ingest_staleness_seconds`.
Correlate deltas and world labels with capture times; a world-wide freshness
gauge alone cannot prove individual boards reconciled. Do not infer repair
from an upstream announcement or a previously requested sweep.

## Investigation direction if drift persists

At inspected main `0a9fbdb268f3657a8789eb0fff694f91d0fbf637`,
`UpdateService::get_missing_updates` fetches the last 200 uploaded items per
world. Both the marker comparison and the aggregate-price probe operate on
that list. The saturation fallback checks whether **all 200 items were
classified as missed**, not whether 200 or more uploads happened since the
last completed pass. Receiving other events can therefore keep the marker
comparison mostly current while a drifted board scrolls out of probe coverage.
This is a code-level coverage limitation, not proof of the runtime sequence
that produced an individual discrepancy.

A repair should preserve a bounded reconciliation path for items that leave
that recency window, and test an item with a fresh sale/ingest marker, a lost
cheap removal, and more than 200 intervening uploads. It should also cover
failed probes, retry eligibility, concurrent updates, and scope/quality
separation. Merely increasing the window or inspecting marker age does not
establish eventual reconciliation. Confirm actual pass timing and failure
metrics before choosing a queue/cursor policy or changing request volume.

The price-drift repair (#1203, `87467e79`) and reprice-safe removals (#1269,
`c4bb6f80`) are already ancestors of this inspected main. No predecessor branch
is required. No production repair has been applied by this diagnostic task.
