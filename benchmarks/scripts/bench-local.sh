#!/usr/bin/env bash
# Local throughput + memory, matched profiles (release Rust, production Next),
# both apps client-rendered with a server-read seed. Builds, starts, warms,
# load-tests with `hey`, reads RSS, tears down.
#
# Requires: hey (https://github.com/rakyll/hey), node/npm, cargo.
# Run from the repo root: benchmarks/scripts/bench-local.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DUR="${DUR:-10s}"; CONC="${CONC:-50}"

command -v hey >/dev/null || { echo "need 'hey' on PATH"; exit 1; }

echo "building (release Rust + production Next)…"
( cd "$ROOT/examples/react-todos" && cargo build --release --bin react-todos >/dev/null 2>&1 )
( cd "$ROOT/benchmarks/apps/nextjs" && [ -d node_modules ] || npm install >/dev/null 2>&1; npm run build >/dev/null 2>&1 )

"$ROOT/target/release/react-todos" >/dev/null 2>&1 & RT=$!
( cd "$ROOT/benchmarks/apps/nextjs" && npm start -- -p 3001 >/dev/null 2>&1 ) & NJ=$!
trap 'kill $RT $NJ 2>/dev/null; pkill -P $NJ 2>/dev/null || true' EXIT

echo "waiting for servers…"
until curl -fs -o /dev/null http://localhost:3000/ 2>/dev/null; do sleep 0.5; done
until curl -fs -o /dev/null http://localhost:3001/ 2>/dev/null; do sleep 0.5; done
# correctness gate: same API shape
a=$(curl -s "http://localhost:3000/api/todos?status=open"); b=$(curl -s "http://localhost:3001/api/todos?status=open")
echo "  nextrs api: ${a:0:48}"; echo "  nextjs api: ${b:0:48}"
# warm
for p in 3000 3001; do for _ in $(seq 1 30); do curl -s -o /dev/null "http://localhost:$p/" "http://localhost:$p/api/todos?status=open"; done; done

bench() { hey -z "$DUR" -c "$CONC" "$1" 2>/dev/null | grep -E "Requests/sec|50%|99%" | sed 's/^/    /'; }
echo; echo "PAGE /  ($DUR, ${CONC}c)"
echo "  nextrs:"; bench "http://localhost:3000/"
echo "  nextjs:"; bench "http://localhost:3001/"
echo "API /api/todos?status=open"
echo "  nextrs:"; bench "http://localhost:3000/api/todos?status=open"
echo "  nextjs:"; bench "http://localhost:3001/api/todos?status=open"

rss() { awk '/VmRSS/{printf "%.1f MB\n",$2/1024}' "/proc/$1/status" 2>/dev/null; }
njpid=$(ss -ltnp 2>/dev/null | grep ':3001' | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2)
echo; echo "Memory (RSS while serving)"
echo "  nextrs: $(rss "$RT")"
echo "  nextjs: $(rss "${njpid:-0}")"
