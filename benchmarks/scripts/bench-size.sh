#!/usr/bin/env bash
# Deployed function size — the real artifact Vercel ships per serverless
# function, computed offline from `vercel build` output (no deploy needed).
#
#   nextrs   = the release `index` binary (the whole function)
#   Next.js  = the files in .vercel/output/functions/index.func, including the
#              traced node_modules referenced by .vc-config.json's filePathMap
#              (a plain `du` of the .func dir under-reports — deps are
#              referenced, not bundled, until deploy)
#
# Run from the repo root: benchmarks/scripts/bench-size.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

echo "== nextrs function (release index binary) =="
( cd "$ROOT/examples/react-todos" && cargo build --release --bin index >/dev/null 2>&1 ) || true
nextrs_bytes=$(stat -c%s "$ROOT/target/release/index" 2>/dev/null || echo 0)
printf "  %.1f MB (%d bytes)\n" "$(echo "$nextrs_bytes/1048576" | bc -l)" "$nextrs_bytes"

echo "== Next.js function (traced) =="
NJ="$ROOT/benchmarks/apps/nextjs"
( cd "$NJ" && vercel build --yes --scope ashirsc >/dev/null 2>&1 ) \
  || ( cd "$NJ" && npx --yes vercel build --yes >/dev/null 2>&1 ) || true
python3 - "$NJ/.vercel/output/functions/index.func" <<'PY'
import json, os, sys
fn = sys.argv[1]
cfg = json.load(open(f"{fn}/.vc-config.json"))
total = 0
for src in set(cfg.get("filePathMap", {}).values()):
    for p in (src, os.path.join(os.path.dirname(fn), "../../..", src)):
        if os.path.exists(p):
            total += os.path.getsize(p); break
for root, _, files in os.walk(fn):
    for f in files:
        total += os.path.getsize(os.path.join(root, f))
print(f"  {total/1048576:.1f} MB ({total:,} bytes)")
PY
