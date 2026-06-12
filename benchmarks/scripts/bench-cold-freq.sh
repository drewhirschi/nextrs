#!/usr/bin/env bash
# Cold-start *frequency* under sustained load (methodology.md "Planned
# experiments" #1): hold a fixed concurrency against a deployed function for a
# fixed window and count how many instances Vercel spins up to serve it.
#
# Each function self-reports `x-cold: 1|0` (first request this instance served)
# and `x-instance` (per-process ID), so we can count cold starts AND distinct
# instances directly instead of inferring from latency. Reports:
#   - total requests, achieved RPS
#   - cold responses (= instances that served their first request in-window)
#   - distinct instances observed, requests-per-instance distribution
#   - cold starts per 1k requests
#
# Usage: bench-cold-freq.sh <api-url> [duration-seconds] [concurrency]
set -euo pipefail
URL="${1:?usage: bench-cold-freq.sh <api-url> [duration-seconds] [concurrency]}"
DURATION="${2:-300}"
CONC="${3:-40}"

OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT
echo "Cold-start frequency: $URL"
echo "  ${CONC} sustained concurrent workers for ${DURATION}s"

DEADLINE=$(( $(date +%s) + DURATION ))
for w in $(seq 1 "$CONC"); do
  (
    h="$OUT/h.$w"
    while [ "$(date +%s)" -lt "$DEADLINE" ]; do
      ttfb="$(curl -s -o /dev/null -D "$h" -m 30 -w '%{time_starttransfer}' "$URL" 2>/dev/null || true)"
      [ -n "$ttfb" ] || { echo "ERR" >> "$OUT/log.$w"; continue; }
      cold="$(grep -i '^x-cold:' "$h" | tr -d '\r' | awk '{print $2}')"
      inst="$(grep -i '^x-instance:' "$h" | tr -d '\r' | awk '{print $2}')"
      echo "$(date +%s.%N) ${cold:-?} ${inst:-?} $ttfb" >> "$OUT/log.$w"
    done
  ) &
done
wait

cat "$OUT"/log.* > "$OUT/all"
python3 - "$OUT/all" "$DURATION" "$CONC" <<'PY'
import sys, math
from collections import Counter

rows, errors = [], 0
for line in open(sys.argv[1]):
    parts = line.split()
    if parts == ["ERR"]:
        errors += 1
    elif len(parts) == 4:
        rows.append(parts)
duration, conc = float(sys.argv[2]), int(sys.argv[3])

total = len(rows)
cold = sum(1 for _, c, _, _ in rows if c == "1")
instances = Counter(i for _, _, i, _ in rows if i != "?")
per_inst = sorted(instances.values(), reverse=True)

def pct(xs, p):
    xs = sorted(xs)
    if not xs: return float("nan")
    k = (len(xs) - 1) * p; f, c = math.floor(k), math.ceil(k)
    return xs[f] if f == c else xs[f] + (xs[c] - xs[f]) * (k - f)

ttfb = [float(t) * 1000 for _, _, _, t in rows]
print(f"\nrequests: {total} ok, {errors} errors  ({total/duration:.1f} req/s achieved, {conc} workers)")
print(f"TTFB ms: p50={pct(ttfb,.5):.0f}  p95={pct(ttfb,.95):.0f}")
print(f"\ncold responses (x-cold:1):     {cold}")
print(f"distinct instances (x-instance): {len(instances)}")
print(f"cold starts per 1k requests:   {1000*cold/total:.2f}" if total else "")
if per_inst:
    print(f"requests per instance: max={per_inst[0]}  median={per_inst[len(per_inst)//2]}  min={per_inst[-1]}")
    print(f"top instances by share: {[f'{100*n/total:.0f}%' for n in per_inst[:8]]}")
PY
