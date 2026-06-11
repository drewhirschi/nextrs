#!/usr/bin/env bash
# Sample cold vs warm TTFB against a deployed function.
#
# Vercel exposes no cold/warm signal, so the function self-reports via an
# `x-cold: 1|0` response header (see each app's API handler). A concurrent
# burst against a deployment with no warm capacity makes Vercel spin up fresh
# instances — each returns x-cold:1 on its first response — so one burst yields
# several cold samples. Idle gaps between bursts let instances recycle.
#
# Usage: bench-cold.sh <api-url> [rounds] [concurrency] [idle-seconds]
set -euo pipefail
URL="${1:?usage: bench-cold.sh <api-url> [rounds] [concurrency] [idle-seconds]}"
ROUNDS="${2:-8}"
CONC="${3:-25}"
IDLE="${4:-25}"

OUT="$(mktemp)"
trap 'rm -f "$OUT"' EXIT
echo "Cold-start sampling: $URL"
echo "  ${ROUNDS} rounds x ${CONC} concurrent, ${IDLE}s idle between bursts"

for r in $(seq 1 "$ROUNDS"); do
  for _ in $(seq 1 "$CONC"); do
    (
      h="$(mktemp)"
      ttfb="$(curl -s -o /dev/null -D "$h" -w '%{time_starttransfer}' "$URL" 2>/dev/null || true)"
      cold="$(grep -i '^x-cold:' "$h" 2>/dev/null | tr -d '\r' | awk '{print $2}')"
      rm -f "$h"
      [ -n "$ttfb" ] && echo "${cold:-?} $ttfb" >> "$OUT"
    ) &
  done
  wait
  echo "  round $r/$ROUNDS done"
  [ "$r" -lt "$ROUNDS" ] && sleep "$IDLE"
done

python3 - "$OUT" <<'PY'
import sys, math
rows = [l.split() for l in open(sys.argv[1]) if l.strip()]
def pct(xs, p):
    xs = sorted(xs)
    if not xs: return float("nan")
    k = (len(xs) - 1) * p; f, c = math.floor(k), math.ceil(k)
    return xs[f] if f == c else xs[f] + (xs[c] - xs[f]) * (k - f)
cold = [float(t) * 1000 for c, t in rows if c == "1"]
warm = [float(t) * 1000 for c, t in rows if c == "0"]
print(f"\nsamples: {len(cold)} cold, {len(warm)} warm")
for name, xs in (("COLD", cold), ("WARM", warm)):
    if xs:
        print(f"  {name} TTFB ms:  p50={pct(xs,.5):.0f}  p95={pct(xs,.95):.0f}  min={min(xs):.0f}  max={max(xs):.0f}")
PY
