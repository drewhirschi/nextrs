# Latency: where it goes and how to make it faster

The published numbers (warm p50): ~220-250ms TTFB for any function-served route, ~145ms TTFB for the CDN-cached `/style.css`. Cold start adds ~250-330ms.

**Goal:** sub-100ms warm TTFB with cold start ~300ms acceptable. This doc breaks down where the current latency comes from, what's theoretically achievable, and the concrete optimizations to get there.

## Where the 220ms actually goes

A real `curl` to `nextrs-umber.vercel.app/simple` from a US west coast machine, broken into phases:

```
warm /simple, 308ms total:
  DNS lookup        4ms       — local resolver cache hit
  TCP handshake    22ms       — 1 RTT to nearest Vercel edge
  TLS handshake    63ms       — additional ~1 RTT for TLS 1.2/1.3 setup
  Server processing 220ms     — request goes from edge to function region
                                and back; includes routing, function
                                invocation, our handler, response
  Total (cold conn) 308ms
```

Reading the response header `x-vercel-id: sfo1::iad1::...`:
- `sfo1` = Vercel edge POP in San Francisco (close to test machine)
- `iad1` = function region in US East (Northern Virginia)

So **every uncached request to a Vercel function from west coast → eastern function region is paying a ~70ms cross-country one-way trip + back, plus Vercel's request routing, plus our actual handler**.

For comparison, the same machine hitting `/style.css` (which is served by the CDN from edge directly, no function hop):

```
warm /style.css, 127ms total:
  DNS              4ms
  TCP             22ms
  TLS             63ms
  Server          40ms       ← edge cache response, no function involved
  Total           127ms
```

The CDN-served file's 40ms server time is essentially "minimum possible TTFB from a west-coast client to nearest edge". The function path adds ~180ms on top of that, almost entirely the edge↔function hop.

**Our actual Rust handler probably runs in <5ms.** Render an askama template, write a few hundred bytes — that's nothing. The 220ms is overwhelmingly Vercel infrastructure, not framework overhead.

## What's the floor?

### Connection reuse (the realistic browser scenario)

The 308ms above includes a fresh TCP+TLS handshake (85ms). In a real browser, after the first navigation the connection is kept alive. Subsequent requests on the same connection skip TCP+TLS entirely:

| Scenario | TTFB |
|---|---|
| Fresh connection (new tab, cold DNS) | ~310ms |
| Same connection, warm function | ~225ms (just server processing) |
| Same connection, CDN edge cache | ~45ms |

Sub-100ms TTFB **on a fresh connection** is essentially impossible — TLS alone is ~85ms. Realistic target is sub-100ms **on warm connections** (subsequent navigation in the same tab). That requires shaving the 220ms server-processing time.

### Theoretical serverful (always-on Rust process, no Vercel)

A bare `axum::serve` Rust process running in the same region as the user, no Lambda-style routing layer:

| Scenario | TTFB |
|---|---|
| Localhost | <1ms |
| Same datacenter, warm conn | 1-5ms |
| Same region, warm conn | 5-20ms |
| Cross-country, warm conn | 60-100ms (network only) |

A well-tuned serverful Rust HTML service in the same region as the user can hit single-digit-ms warm TTFB. This is the upper bound on optimization. The gap between that and our ~220ms is "what serverless costs us".

## Why serverless is slower

Even when our function is warm, Vercel's invocation path adds latency:

1. **Edge POP receives request** — TLS terminates here, request inspected
2. **Routing decision** — match `vercel.json` rewrites, decide which function
3. **Edge → function-region** — request body forwarded across the Vercel internal network to the AWS Lambda environment in `iad1`
4. **Lambda invocation** — Vercel's "Fluid compute" reuses warm function instances; routing inside Lambda still costs ~10-30ms
5. **Our handler runs** — <5ms
6. **Response back** — function → edge → client

Steps 3 and 4 are the dominant cost. They exist because functions are deployed in regions, not at edges, and Lambda doesn't route requests directly from edges. This is the architectural cost of "function-as-a-service" vs "edge-as-a-service".

## Optimizations, ordered by impact

### Tier 1: Get to ~150ms warm TTFB (small changes)

**1.1. Multi-region function deployment.** Vercel lets you specify `regions` in `vercel.json`. If most users are on US west, deploy to `sfo1` (or both `iad1` and `sfo1`). Saves the cross-country hop. Expected: ~80-100ms TTFB drop on requests routed to a near-region function.

```json
{
  "functions": {
    "api/index.rs": { "regions": ["iad1", "sfo1"] }
  }
}
```

Cost: more cold starts (one per region per cold period). For our use case probably worth it.

**1.2. Route-level CDN caching.** Routes whose response doesn't depend on per-request data (e.g., `/`, `/simple`, the docs site) can be cached by the Vercel CDN with `Cache-Control: public, s-maxage=...`. Then warm-after-first-hit responses are served by the same edge that serves `/style.css`:

```rust
Response::builder()
    .header("cache-control", "public, s-maxage=300, stale-while-revalidate=86400")
    .body(...)
```

Expected: ~40-60ms TTFB on cache hits (matches `/style.css` numbers). Doesn't help streaming routes (those have per-request behavior by design).

**1.3. Pre-bake the layout shell at registry build time.** Currently `router.rs::render_route` calls `layout_shell()` on every request — composes the layout chain around a marker, splits on the marker. For a given route, this result never changes. Memoize it once at registry construction (or codegen time) and reuse. Saves ~1-3ms per request — minor at the network level, but it's the only piece *we control*.

### Tier 2: Get to ~50-80ms warm TTFB (architectural)

**2.1. Vercel Edge Runtime via WASM.** Compile our Rust to WebAssembly and deploy to Vercel's Edge runtime instead of the Rust function runtime. Edge functions run at the edge POP itself — no edge→region hop. Cloudflare Workers and Vercel Edge use V8 isolates; Wasm runs in those isolates with low startup cost.

Reality check:
- axum doesn't run on Wasm directly (no tokio in Wasm browsers, but `wasm32-wasi` works for serverside)
- `vercel_runtime` doesn't have a Wasm target today
- We'd need to either swap to a Wasm-compatible HTTP framework (worker-rs, spin-sdk) or wait for the Vercel team to add a Wasm-runtime path
- The `tokio::time::sleep` calls in our example pages would need a Wasm-compatible alternative

Expected if achievable: 30-60ms TTFB warm. Massive win, significant rework.

**2.2. Static-route precompilation.** For routes that have only `.html` (no `.rs`, no per-request data), pre-render the full HTML at build time and emit it as a file in `public/`. Then it's served by the CDN, never invokes the function. Codegen could do this automatically by inspecting `DiscoveredRoute` for "all-html" routes and emitting their composed layout+page output to `public/<route>/index.html`.

Expected: ~40-60ms TTFB for those routes (CDN edge speed). Doesn't help routes with `.rs` or `loading`.

### Tier 3: Stop doing serverless (out of scope, mentioned for completeness)

Same Rust binary, deployed to Fly.io / Cloud Run / a VM in the same region as users. Skip Vercel's Lambda layer entirely. Expected: 5-30ms TTFB warm. Cost: you give up Vercel's edge cache, deploy ergonomics, automatic scaling-to-zero, etc.

Not the path we want — defeats the point of using Vercel — but worth knowing the floor.

## The path to sub-100ms

Realistic plan, in order:

| Step | Effort | Expected p50 warm TTFB | Notes |
|---|---|---|---|
| Today | — | ~220ms | iad1 only, no caching |
| Add `sfo1` to function regions | trivial (1 line vercel.json) | ~140ms for west-coast, ~140ms for east-coast | More cold starts |
| Add Cache-Control to non-streaming routes | small (header per route) | ~50ms on cache hits, ~140ms on misses | Best for `/`, `/simple`, docs |
| Pre-render all-`.html` routes to `public/` | medium (codegen change) | ~50ms (CDN edge) | Doesn't help `.rs` routes or streaming |
| WASM Edge runtime | large | ~40-60ms | Needs framework rework |

**To hit sub-100ms p50 warm without WASM**, the realistic combination is:
1. Multi-region functions (cuts cross-country trip)
2. Aggressive CDN caching for cacheable routes

That gets cacheable routes to ~50ms (CDN edge) and uncacheable routes (streaming, dynamic) to ~140ms — which is below 100ms ONLY for routes that benefit from the cache.

**Honest answer:** sub-100ms p50 warm for *every* route requires Edge runtime (Wasm). Sub-100ms for *cacheable* routes is achievable today with CDN headers + multi-region.

For the streaming routes specifically (`/with-loading`, `/with-layout`), the user-perceived "first paint" is what matters more than TTFB. The loading shell lands at TTFB (~220ms today, ~140ms with multi-region). The page chunk arrives later (~800ms in our demo). Sub-100ms TTFB on streaming routes only matters if the user notices the gap before the loading shell paints — which they don't.

So the practical answer is:

- **Streaming routes**: optimize TTFB to ~140ms via multi-region. Loading shell paints fast enough that sub-100ms TTFB isn't really visible. Don't over-engineer.
- **Static-content routes**: cache aggressively. Get to ~50ms. Worth it.
- **Dynamic non-streaming routes**: multi-region gets you to ~140ms. Sub-100ms requires either CDN with short-TTL or Edge runtime.

## What's NOT worth doing

- **Shrinking the binary further.** Cold start is already 250-330ms above warm. Worth it if you want cold starts under 100ms, but our target is "cold start ~300ms is fine". LTO is on, codegen-units=1, no debug symbols in release. Diminishing returns from here.
- **Removing dependencies.** axum, askama, vercel_runtime, etc. are all in use. Stripping unused features inside them would save kilobytes, not milliseconds.
- **Custom HTTP server.** Replacing axum with raw hyper would save sub-millisecond per request. The 220ms is network and routing, not HTTP framework code.
- **Connection pooling.** Vercel handles this transparently for us; no client-side pool to tune.
- **Warming ping** (cron that hits the function to keep it warm). Vercel's Fluid compute already does this — we observed the function staying warm past 60s of inactivity.

## What I'd do next, concretely

1. **Multi-region in vercel.json** — one-line change, biggest single win. Verify with a US east curl vs US west curl after deploy.
2. **Cache headers on `/`, `/simple`** — one header per route. Re-measure with `x-vercel-cache: HIT` proving the path works.
3. **Time the difference** — capture before/after numbers. Update this doc with real results.

I'd defer pre-rendering and Wasm Edge until we have a real workload that needs them. The first two changes likely close most of the gap to sub-100ms for the common case.

---

*Numbers are p50 warm TTFB measured from a single US west coast client against a single deployment. Production p99 will be higher; cross-continent users will see different numbers; Vercel's infrastructure changes over time. Treat them as order-of-magnitude, not absolutes.*
