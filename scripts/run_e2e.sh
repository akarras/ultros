#!/usr/bin/env bash
# Local E2E driver: brings up *this worktree's* app on a fresh random port,
# runs the Puppeteer suite in `integration/`, tears the server down.
#
# Default behavior is fresh-spawn-on-random-port, because multi-worktree setups
# can have unrelated agent processes squatting on 8080 from other branches.
# Reusing whatever is listening there would test the wrong build.
#
# Env:
#   REUSE_SERVER   1 to reuse a server already up on $BASE_URL (skips build/spawn)
#   BASE_URL       only used when REUSE_SERVER=1 (default http://127.0.0.1:8080)
#   E2E_PORT       pin the spawned server to this port (default: pick a free one)
#   READY_PATH     default /                  (path polled for HTTP 200 readiness)
#   READY_TIMEOUT  default 300                (seconds to wait for the server)
#   ANALYZER_READY_PATH  default /api/v1/cheapest/North-America (probed second to
#                  wait out the analyzer service's cold-start warmup; set to
#                  empty to skip)
#   ANALYZER_READY_TIMEOUT  default 180       (seconds to wait for analyzer)
#   DEVICE         desktop | mobile | both (default both)
#   SKIP_BUILD     1 to skip `cargo leptos build` before serve
#   LEPTOS_FEATURES extra leptos-bin-features (space-separated). Set to an
#                  explicit empty string to override metadata bin-features.
#
# Exit code is the npm test exit code (0 on success).

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

REUSE_SERVER="${REUSE_SERVER:-0}"
READY_PATH="${READY_PATH:-/}"
READY_TIMEOUT="${READY_TIMEOUT:-300}"
ANALYZER_READY_PATH="${ANALYZER_READY_PATH-/api/v1/cheapest/North-America}"
ANALYZER_READY_TIMEOUT="${ANALYZER_READY_TIMEOUT:-180}"
DEVICE="${DEVICE:-both}"
bin_feature_args=()
if [ "${LEPTOS_FEATURES+x}" = "x" ]; then
    bin_feature_args=(--bin-features "$LEPTOS_FEATURES")
fi

log() { printf '[e2e] %s\n' "$*" >&2; }

if [ ! -f .env ] && [ -z "${DATABASE_URL:-}" ]; then
    cat >&2 <<'EOF'
[e2e] Neither .env nor DATABASE_URL is set. Either:
    - copy an .env from a sibling worktree (`cp ../../.env .`), or
    - export the required vars directly (DATABASE_URL, DISCORD_*, KEY,
      HOSTNAME, PORT) — CI does this via the workflow `env:` block.
EOF
    exit 1
fi

pick_free_port() {
    # node is required for puppeteer anyway; use it to grab an ephemeral port.
    node -e "const s=require('net').createServer().listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>console.log(p));});"
}

server_pid=""
cleanup() {
    if [ -n "$server_pid" ] && kill -0 "$server_pid" 2>/dev/null; then
        log "stopping cargo leptos serve (pid $server_pid)"
        kill -TERM -- "-$server_pid" 2>/dev/null || kill -TERM "$server_pid" 2>/dev/null || true
        if command -v taskkill >/dev/null 2>&1; then
            taskkill //F //T //PID "$server_pid" >/dev/null 2>&1 || true
        fi
        for _ in 1 2 3 4 5; do
            kill -0 "$server_pid" 2>/dev/null || break
            sleep 1
        done
        kill -KILL "$server_pid" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

wait_for_server() {
    local url="$1"
    local deadline=$(( $(date +%s) + READY_TIMEOUT ))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        if curl -fsS -o /dev/null --max-time 3 "$url" 2>/dev/null; then
            return 0
        fi
        sleep 2
    done
    return 1
}

if [ "$REUSE_SERVER" = "1" ]; then
    BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
    log "REUSE_SERVER=1 — checking $BASE_URL$READY_PATH"
    if ! curl -fsS -o /dev/null --max-time 3 "$BASE_URL$READY_PATH" 2>/dev/null; then
        log "no server reachable at $BASE_URL — aborting (unset REUSE_SERVER for a fresh spawn)"
        exit 1
    fi
    log "reusing existing server at $BASE_URL"
else
    port="${E2E_PORT:-$(pick_free_port)}"
    BASE_URL="http://127.0.0.1:$port"
    log "spawning fresh server on port $port"

    if [ "${SKIP_BUILD:-0}" != "1" ]; then
        log "cargo leptos build (set SKIP_BUILD=1 to skip)"
        cargo leptos build "${bin_feature_args[@]}"
    fi

    set -m
    PORT="$port" \
        HOSTNAME="$BASE_URL" \
        LEPTOS_SITE_ADDR="127.0.0.1:$port" \
        cargo leptos serve \
        "${bin_feature_args[@]}" \
        >/tmp/ultros-e2e-server.log 2>&1 &
    server_pid=$!
    set +m
    log "server pid $server_pid; logs at /tmp/ultros-e2e-server.log"

    log "waiting up to ${READY_TIMEOUT}s for $BASE_URL$READY_PATH"
    if ! wait_for_server "$BASE_URL$READY_PATH"; then
        log "server did not become ready; last 60 log lines:"
        tail -n 60 /tmp/ultros-e2e-server.log >&2 || true
        exit 1
    fi
    log "server is up"

    if [ -n "$ANALYZER_READY_PATH" ]; then
        # AnalyzerService is built async after server start; first hit can be
        # 503 (Uninitialized) for tens of seconds while it fills its caches.
        # Wait it out so screenshot/console assertions aren't polluted.
        log "waiting up to ${ANALYZER_READY_TIMEOUT}s for analyzer at $BASE_URL$ANALYZER_READY_PATH"
        deadline=$(( $(date +%s) + ANALYZER_READY_TIMEOUT ))
        while [ "$(date +%s)" -lt "$deadline" ]; do
            if curl -fsS -o /dev/null --max-time 5 "$BASE_URL$ANALYZER_READY_PATH" 2>/dev/null; then
                log "analyzer is warm"
                break
            fi
            sleep 3
        done
    fi
fi

if [ ! -d integration/node_modules ]; then
    log "installing Puppeteer (one-time)"
    (cd integration && npm ci)
fi

case "$DEVICE" in
    desktop) test_script="test:desktop" ;;
    mobile)  test_script="test:mobile" ;;
    both|*)  test_script="test" ;;
esac

test_exit=0
if [ "${DASHBOARD_ONLY:-0}" = "1" ]; then
    log "DASHBOARD_ONLY=1 — skipping the broad desktop+mobile route suite"
else
    log "running npm run $test_script in integration/ against $BASE_URL"
    # `|| test_exit=$?` captures the npm exit code without triggering set -e.
    ( cd integration && BASE_URL="$BASE_URL" npm run "$test_script" ) || test_exit=$?
fi

if [ "${RUN_FC_CRAFTING_BREAKDOWN:-1}" != "0" ]; then
    log "running FC crafting material-breakdown E2E"
    fc_crafting_exit=0
    ( cd integration && BASE_URL="$BASE_URL" npm run test:fc-crafting-breakdown ) || fc_crafting_exit=$?
    if [ "$fc_crafting_exit" -ne 0 ] && [ "$test_exit" -eq 0 ]; then
        test_exit="$fc_crafting_exit"
    fi
fi

# If we built with test-auth, also exercise the login flow even when the
# screenshot suite failed — failures may be unrelated and the login signal
# is independently valuable.
case " ${LEPTOS_FEATURES:-} " in
    *" test-auth "*)
        log "running login flow (test-auth feature detected)"
        login_exit=0
        ( cd integration && BASE_URL="$BASE_URL" npm run test:login ) || login_exit=$?
        if [ "$login_exit" -ne 0 ] && [ "$test_exit" -eq 0 ]; then
            test_exit="$login_exit"
        fi
        log "running shared-list flow (test-auth feature detected)"
        shared_list_exit=0
        ( cd integration && BASE_URL="$BASE_URL" npm run test:shared-list ) || shared_list_exit=$?
        if [ "$shared_list_exit" -ne 0 ] && [ "$test_exit" -eq 0 ]; then
            test_exit="$shared_list_exit"
        fi
        log "running list-flow E2E (test-auth feature detected)"
        list_flow_exit=0
        ( cd integration && BASE_URL="$BASE_URL" npm run test:list-flow ) || list_flow_exit=$?
        if [ "$list_flow_exit" -ne 0 ] && [ "$test_exit" -eq 0 ]; then
            test_exit="$list_flow_exit"
        fi
        log "running browser-push smoke (test-auth feature detected)"
        push_exit=0
        ( cd integration && BASE_URL="$BASE_URL" npm run test:push ) || push_exit=$?
        if [ "$push_exit" -ne 0 ] && [ "$test_exit" -eq 0 ]; then
            test_exit="$push_exit"
        fi
        ;;
esac

# Optional focused dashboard screenshots — covers the new home-page
# MarketPulse + Trends ConfidenceBadge + item view surfaces. Captured into
# integration/artifacts/dashboard/ regardless of the broader suite outcome.
# Set RUN_DASHBOARD=0 to skip.
if [ "${RUN_DASHBOARD:-1}" != "0" ]; then
    log "running dashboard screenshots"
    dashboard_exit=0
    ( cd integration && BASE_URL="$BASE_URL" npm run test:dashboard ) || dashboard_exit=$?
    if [ "$dashboard_exit" -ne 0 ] && [ "$test_exit" -eq 0 ]; then
        test_exit="$dashboard_exit"
    fi
fi

log "screenshots in integration/artifacts/ (exit=$test_exit)"
exit "$test_exit"
