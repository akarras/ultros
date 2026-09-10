# Automatic market reconciliation

Part of #1178. This change repairs board drift without relying on receiving every
websocket event. It does not establish that the production discrepancy is fixed.

## Startup and steady state

World initialization finishes before the immutable world cache is constructed.
The upstream metadata fetch has a 15-second deadline. On timeout or failure,
startup uses persisted worlds; an empty world cache still prevents startup.
The existing recent-item catch-up starts without an initial timer delay. In
parallel, automatic full reconciliation claims the same PostgreSQL advisory lease
as `/rescan_market`, resumes an unfinished sweep, or starts a new durable sweep.
Discord availability does not delay reconciliation. Automatic runs log progress;
an operator-started run retains its channel when the bot is available at resume.

A resumed sweep can have visited early worlds before the outage. After finishing
it, a pass started before this process boot is followed by a fresh pass. A pass
started after this process boot, including one run by another replica or an
operator, satisfies startup coverage. Subsequent full passes become due six hours
after completion and are checked every five minutes. This is a scheduling interval,
not a promise that every board is verified every six hours: full passes take hours
and upstream failures can delay them further.

The worker retries a busy lease/database read every 30 seconds. After a sweep
attempt it waits five minutes before retrying or checking whether another pass is
due. One full sweep is allowed across replicas; recent-item catch-up still runs
alongside it. Existing request limits remain: up to 100 items per upstream batch,
one-second spacing after batches, bounded transient backoff, and at most 50 database
item operations per batch. This does not add a global rate limiter across all
upstream callers or replicas.

## Failure recovery

The durable world cursor stays at the earliest failed chunk even when subsequent
chunks succeed. Later worlds are still visited. A world with a retained failure
is not marked complete, and the sweep stays active for another attempt. Restarting
replays from that cursor; a clean retry can advance it. This intentionally repeats
some successful work instead of requiring a new retry-table migration. Counters
include repeated attempts and must not be read as unique-board counts.
If failures keep a sweep active, completed worlds become eligible again after
six hours. Their attempt counters are retained while their cursor restarts, so
one persistently failing world cannot indefinitely exclude the healthy worlds.

A batch 404 does not prove the world is unsupported: the scan only stops that
world if the separate recent-update endpoint has also classified it as unavailable.
Otherwise it proceeds to later batches. Unavailable batches are deferred to the
next full pass, never treated as empty listing boards. Items the upstream batch does not
resolve are not synthesized as empty boards. Full completion means traversal
completed, not that upstream provided a board for every possible item.

Set `ULTROS_DISABLE_AUTOMATIC_RECONCILIATION=true` on QA instances to disable the
automatic full-pass scheduler, including automatic resume. Manual scans and the
recent-item loop remain available. `ULTROS_DISABLE_UNIVERSALIS_WEBSOCKET` remains
independent and does not disable reconciliation.
Both disable flags accept whitespace around their values and treat unrecognized
nonempty values as enabled with a warning, so a typo cannot silently enable QA ingest.

## Verification and remaining work

Regression coverage checks immediate first attempt, lease contention and
cancellation, startup coverage after an old sweep finishes, periodic coverage
without new events, and failed cursors surviving later successful chunks/restart.
The opt-in database sweep test checks completed-pass lookup with an active successor.

Before closing #1178, repeat the read-only procedure in
[stale-listing-verification.md](stale-listing-verification.md), record the deployed
build, and correlate actual sweep progress with board and analyzer convergence.
Test a missed removal followed by fresh sale markers and more than 200 later
uploads. Check retries, request volume, pool pressure, and time to full coverage.

Still outside this change: ordered event writes, snapshot/event race protection,
per-board verification timestamps, weighted scheduling, durable outgoing events,
and upstream replay. A per-board retry queue would avoid replaying the successful
suffix of a failed world and allow more precise prioritization.
