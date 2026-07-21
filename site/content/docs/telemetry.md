+++
title = "Route Telemetry"
description = "Diagnose slow routes progressively: Server-Timing in DevTools, custom segments with the Timing extractor, and OpenTelemetry export for production history"
section = "Guides"
order = 7
+++

Every nextrs route records a latency breakdown — middleware chain, handler,
and named sub-segments like the `seed` step of a prefetch-backed React page.
Nothing to configure; it's on for every app the moment you rebuild.

Diagnosing a slow route is a ladder. Most problems die on the first rung.

## 1. Read the Server-Timing header

Open the slow page with DevTools' Network tab, select the request, and look
at the **Timing** panel (or the raw headers):

```
server-timing: mw;dur=1.2, seed;dur=430.0, handler;dur=445.1, total;dur=447.0, route;desc="/todos/{id}"
```

| Metric | Meaning |
|---|---|
| `mw` | your `middleware.rs` chain, root → leaf |
| `seed` | server-side query seeding (`prefetch.rs`), for React pages |
| `handler` | the page render or API handler, inclusive of segments |
| `total` | the whole request, inside the framework |
| `route;desc` | which route template matched |

Here the verdict is immediate: 430 of 447 ms is seeding — the data layer,
not rendering or middleware.

Streaming pages (`loading` present) send headers before the page function
runs, so their header carries only `mw` and a `streaming` marker; the full
breakdown still reaches tracing (rung 3) when the stream completes.

To turn the header off (it is visible to clients), set
`NEXTRS_SERVER_TIMING=0`.

## 2. Add your own segments

When `handler` is big and you need to know *why*, extract `Timing` and wrap
the suspects:

```rust
use axum::{Extension, Json};

#[nextrs::api(get, ...)]
pub async fn get(
    Extension(db): Extension<Db>,
    timing: nextrs::Timing,
) -> Json<Vec<Todo>> {
    let todos = timing.span("db", db.list()).await;
    Json(todos)
}
```

Reload — `db;dur=…` appears in the same header. No infrastructure, no
config; the iteration loop is edit → reload → read the Network tab.

`Timing` works in seeded GET handlers too: when `prefetch.rs` seeds through
the handler during a page render, its segments land in that page's
breakdown. Outside a request (unit tests) every method is a no-op.

## 3. Ship it to a collector

The same instrumentation is a `tracing` span (`nextrs.route`) plus a
per-request summary event (target `nextrs::telemetry`) with OpenTelemetry
semantic-convention fields: `http.route`, `http.request.method`,
`http.response.status_code`, `total_ms`, `mw_ms`, `handler_ms`, `seed_ms`,
`segments`, `streaming`, `cold`.

- **Local dev:** `RUST_LOG=nextrs=info` prints one summary line per request
  (`=debug` adds the span and per-segment events).
- **Vercel:** the same lines land in your function logs — queryable in the
  dashboard with zero setup.
- **OTel backend** (Grafana, Honeycomb, Axiom, …): add an OTLP-exporting
  subscriber and the spans flow as real traces, your `timing.span` and
  library spans nested under `nextrs.route`:

```rust
// Cargo.toml: tracing-subscriber, tracing-opentelemetry,
//             opentelemetry, opentelemetry-otlp
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

let tracer = opentelemetry_otlp::new_pipeline()
    .tracing()
    .with_exporter(opentelemetry_otlp::new_exporter().tonic())
    .install_batch(opentelemetry_sdk::runtime::Tokio)?;

tracing_subscriber::registry()
    .with(tracing_subscriber::fmt::layer())
    .with(tracing_opentelemetry::layer().with_tracer(tracer))
    .init();
```

**Serverless caveat:** OTel's batch exporter buffers spans, and a frozen
Vercel instance never flushes them. Either use the simple (per-span)
exporter, or flush from background work registered with
[`WaitUntil`](/docs/react-server-props) so the runtime drains it before
freezing the instance.

## What gets measured

- Every matched page and API route, labeled with its route template.
- The soft-nav prefetch endpoint, labeled `/__nx/prefetch<route>` so slow
  seed work is attributable to soft navigations separately from hard loads.
- `not_found` surfaces, labeled `__not_found`.
- Static files are not instrumented.

The cold-start headers (`x-nextrs-cold`, `x-nextrs-boot-id`) ride on the
same responses, and the summary event carries `cold` — so cold and warm
latency for a route are separable in whatever backend you query.
