# Active CPU under identical load — nextrs vs Next.js (pinger era)

- **Window:** 2026-07-18 → 2026-08-03 (burst-era pinger data; pinger disabled
  2026-08-03, so this dataset is final)
- **Sources:** Vercel usage dashboard (team ashirsc, ~30-day view read
  2026-08-02/03) + Turso `coldstarts` aggregates (GET
  https://nextrs-docs.vercel.app/api/coldstarts)

## The dashboard numbers (Active CPU, ~30-day window)

Traffic was ≈100% the coldstart-pinger, which sent byte-identical load to
both sides of each pair (same burst size, schedule, jitter; batches
invalidated on region mismatch — see `metrics/ping.mjs`).

| pair    | Next.js app       | Active CPU | nextrs app  | Active CPU | ratio |
|---------|-------------------|-----------:|-------------|-----------:|------:|
| booking | hhh-nextjs        | ~2 h       | hhh-rs      | ~5–6 min   | ~20×  |
| todos   | bench-nextjs-todos| ~30 min    | react-todos | ~3 min     | ~10×  |
| —       | —                 | —          | nextrs-docs | ~6 min     | —     |

hhh-nextjs alone consumed ~half the Hobby plan's included Active CPU while
using ~10% of included invocations and negligible duration.

## Invocation skew and normalization (added 2026-08-04)

Dashboard also showed (booking pair, same window): **invocations** hhh-next
~25k vs hhh-rs ~10k; **provisioned memory** ~17 GB-hr vs ~0.5 GB-hr (~34×).

The invocation counts are asymmetric even though the pinger provably sent
equal load (3,550 Turso samples per target on each side). So hhh-next
generated ~2.5 invocations per request served — likely Next.js middleware
(billed as its own invocation per request), ISR revalidations, and/or more
bot traffic reaching its catch-all function. Unconfirmed (per-path metrics
paywalled).

Two normalizations, both defensible:

| basis | Active CPU | provisioned memory |
|---|---:|---:|
| per invocation (25k vs 10k) | ~288 ms vs ~33 ms → **~9×** | **~13×** |
| per identical request (pinger-equalized) | **~20×** | **~34×** |

Per-request is arguably the fairer frame — the extra invocations are ones
Next.js *needs* to serve the same request, which is itself a cost — but
some may be incidental bot traffic. Conservative public claim: **~10× CPU
and ~13× memory per invocation; ~20×/~34× per identical request.**

## Why the CPU line blew up while everything else stayed low

**1. The included allotments are wildly asymmetric.** Hobby includes ~1M
invocations but only ~4 h of Active CPU — a budget of ~14 ms CPU per
invocation. A single Next.js cold boot burns ~1–1.5 s of CPU (Node + Next
init), i.e. the CPU budget of ~100 invocations, before the handler runs at
all. A Rust binary boots in tens of ms — roughly one invocation's budget.

**2. Two-thirds of our "simplest calls possible" included booting an entire
server.** The bursts force scale-out *by design*, and per Turso the burst
requests were almost all cold — for hhh-nextjs: 2,822 of 2,840 burst page
requests cold, 1,951 of 2,840 api (Fluid absorbed some api concurrency);
nextjs-todos: 2,838/2,840. The handler was trivial; the boot billed as
Active CPU was not.

**3. The math closes.** hhh-nextjs logged ~4,800 cold boots over the window
(2,021 api + 2,822 page). At ~1–1.5 s of boot CPU each that is ~1.3–2.0 h —
essentially the entire 2 h on the dashboard; its ~2,250 warm requests at
~50–100 ms CPU add only minutes. hhh-rs logged a similar ~4,700 cold boots,
but at ~30–60 ms of boot CPU each that is ~3–5 min — matching its dashboard
line. **Same boot count, ~20× less CPU per boot: the entire gap is boot
cost, which is exactly the framework difference.**

Corroborating wall-clock from the same window (cold p50): hhh-nextjs page
6,395 ms vs hhh-rs page 1,575 ms; warm p50 232 ms vs 91 ms.

## Fair-use caveat for quoting this

The workload was cold-start-heavy by design (~66% cold rate). A mostly-warm
real-world mix narrows the ratio — though warm SSR still costs per-request
CPU in Node while nextrs rendering is comparatively free. Defensible claim:
**"10–20× less Active CPU under a cold-start-heavy synthetic load, with
smaller but real warm-path wins on top."** The strongest public version
would be a controlled 48 h re-run of the pinger against just the pairs with
before/after dashboard screenshots.

## Messaging note (Drew, 2026-08-04): retire cold-start *rates*, reframe

Not a work item yet — a direction for how the site/docs talk about this.

"How often do you get a cold start" statistics should go away: cold-start
frequency is so workload-dependent that any number we publish is really a
description of our synthetic pinger, not of anyone's app. What to emphasize
instead:

1. **Faster processing frees the instance sooner.** When a request completes
   quicker, that same instance is available for more requests — throughput
   per instance, not just latency per request.
2. **A small binary makes instance reuse the norm.** The smaller
   footprint means more requests can be packed through a single instance
   and instances get reused instead of constantly booting new ones — the
   scale-out that forces cold starts happens far less in the first place.
3. **Fluid's billing model rewards exactly this shape.** Under Active CPU
   pricing, the ideal function spends nearly all its wall-clock waiting on
   IO and hardly any Active CPU. A nextrs app naturally has that profile —
   which is also just cheaper.

The through-line: don't argue "our cold starts are rare/fast" (workload-
dependent); argue "our compute-per-request is tiny, so instances stay
available, get reused, and cost almost nothing under Active CPU billing."

Note: dashboard totals were the only CPU source (per-route CPU metrics are
paywalled behind Observability Plus) and roll off ~2026-09-02 — screenshot
before then. The Turso data (latency, cold counts, boot IDs) is permanent.
