#!/usr/bin/env bash
# Local throughput + memory for the REALISTIC app (hhh: bookings/admin,
# better-auth, Postgres, shadcn) — the methodology's "realistic-deps" bar.
# Same matched-profile rules as bench-local.sh (release Rust vs production
# Next), but DB-backed: both variants point at the SAME local Postgres so the
# work per request is identical.
#
# Layout assumption (see docs/hhh-migration-plan.md):
#   HHH_NEXTRS  — checkout of hhh-next on branch `nextrs`   (default ~/work/hhh-next)
#   HHH_NEXT    — checkout of hhh-next on main              (default ~/work/hhh-main)
# Postgres must be up (docker compose in the hhh repo; MIGRATION.md documents
# port :5433 via the gitignored override) and the auth sidecar running for the
# nextrs variant (bun auth-sidecar.ts).
#
# Measures, per variant:
#   PAGE  /            (public landing — bundle shell + seed)
#   PAGE  /app         (authed user dashboard; session cookie)
#   API   action endpoint get-user-bookings (authed POST — the converted
#         server-action transport; for Next it's the original server action
#         invoked via its RSC action endpoint, measured at the page level
#         instead — see notes below) plus a public GET (health/products).
#   RSS while serving.
#
# A session cookie is required for the authed rows: pass
#   HHH_COOKIE='better-auth.session_token=<token>.<sig>'
# (create a user in either app, copy the cookie). Authed rows are skipped if
# unset.
#
# Usage: benchmarks/scripts/bench-hhh-local.sh
set -euo pipefail
HHH_NEXTRS="${HHH_NEXTRS:-$HOME/work/hhh-next}"
HHH_NEXT="${HHH_NEXT:-$HOME/work/hhh-main}"
DUR="${DUR:-10s}"; CONC="${CONC:-50}"
RUST_PORT="${RUST_PORT:-3021}"; NEXT_PORT="${NEXT_PORT:-3022}"

command -v hey >/dev/null || { echo "need 'hey' on PATH"; exit 1; }
[ -d "$HHH_NEXTRS" ] || { echo "no $HHH_NEXTRS"; exit 1; }
[ -d "$HHH_NEXT" ] || { echo "no $HHH_NEXT (git worktree add it from the hhh repo, branch main)"; exit 1; }

echo "building (release Rust + production Next)…"
( cd "$HHH_NEXTRS" && NEXTRS_SKIP_BUNDLE=0 cargo build --release >/dev/null 2>&1 )
( cd "$HHH_NEXT" && [ -d node_modules ] || bun install >/dev/null 2>&1; bun run build >/dev/null 2>&1 )

BIN="$(cd "$HHH_NEXTRS" && cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import json,sys; m=json.load(sys.stdin); print(m['target_directory'])")/release/hhh"
( cd "$HHH_NEXTRS" && PORT="$RUST_PORT" "$BIN" >/dev/null 2>&1 ) & RT=$!
( cd "$HHH_NEXT" && bun run start -- -p "$NEXT_PORT" >/dev/null 2>&1 ) & NJ=$!
trap 'kill $RT $NJ 2>/dev/null; pkill -P $NJ 2>/dev/null || true' EXIT

echo "waiting for servers…"
until curl -fs -o /dev/null "http://localhost:$RUST_PORT/" 2>/dev/null; do sleep 0.5; done
until curl -fs -o /dev/null "http://localhost:$NEXT_PORT/" 2>/dev/null; do sleep 0.5; done

# warm
for p in "$RUST_PORT" "$NEXT_PORT"; do
  for _ in $(seq 1 30); do curl -s -o /dev/null "http://localhost:$p/"; done
done

bench() { hey -z "$DUR" -c "$CONC" "${@:2}" "$1" 2>/dev/null | grep -E "Requests/sec|50%|99%" | sed 's/^/    /'; }

echo; echo "PAGE /  ($DUR, ${CONC}c)"
echo "  nextrs:"; bench "http://localhost:$RUST_PORT/"
echo "  nextjs:"; bench "http://localhost:$NEXT_PORT/"

if [ -n "${HHH_COOKIE:-}" ]; then
  echo; echo "PAGE /app (authed)"
  echo "  nextrs:"; bench "http://localhost:$RUST_PORT/app" -H "Cookie: $HHH_COOKIE"
  echo "  nextjs:"; bench "http://localhost:$NEXT_PORT/app" -H "Cookie: $HHH_COOKIE"
  echo; echo "ACTION get-user-bookings (authed POST, DB-backed)"
  echo "  nextrs:"; bench "http://localhost:$RUST_PORT/api/actions/user-bookings/get-user-bookings" \
      -m POST -H "Cookie: $HHH_COOKIE" -H "content-type: application/json"
  # Next.js comparator: the original is a server action (RPC inside the React
  # runtime, not addressable as a plain POST without the action ID). The
  # honest comparable is the authed PAGE row above, which embeds the same
  # query work. We do not fabricate a Next action endpoint.
else
  echo; echo "(authed rows skipped — set HHH_COOKIE)"
fi

rss() { awk '/VmRSS/{printf "%.1f MB\n",$2/1024}' "/proc/$1/status" 2>/dev/null; }
njpid=$(ss -ltnp 2>/dev/null | grep ":$NEXT_PORT" | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2)
rtpid=$(ss -ltnp 2>/dev/null | grep ":$RUST_PORT" | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2)
echo; echo "Memory (RSS while serving)"
echo "  nextrs: $(rss "${rtpid:-$RT}")"
echo "  nextjs: $(rss "${njpid:-0}")"
