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

## Preload stats

The app shell [preloads](/docs/streaming#preload) a link's data on intent —
hover, or a prediction such as the pointer heading for the link. Whether
prediction is worth its extra requests depends on your app: how slow your
`prefetch.rs` routes are, and how your users move. Measure it with real
traffic by setting one environment variable:

```
NEXTRS_PRELOAD_STATS=1
```

Each visitor's browser then reports its soft navigations to
`POST /__nx/preload-stats` when the page is hidden, and the server emits them
as `tracing` events with target `nextrs::preload` — into the same pipeline as
rung 3 above (function logs, or your OTel backend).

One `preload navigation` event per soft navigation:

| Field | Meaning |
|---|---|
| `http.route` | the route template navigated to |
| `trigger` | the signal that started the preload: `hover`, `mouse/trajectory`, `tab/forwards`, `touch`, … or `none` |
| `data` | whether the route has a `prefetch.rs` |
| `load_ms` | how long the preload took |
| `lead_hover_ms` | head start hover had before the click (absent: never hovered, e.g. keyboard) |
| `lead_predicted_ms`, `predictor` | head start the first predictor had, and which one |
| `wait_none_ms` | the wait with no preload at all (= `load_ms`) |
| `wait_hover_ms` | the wait if only hover preloaded |
| `wait_predictive_ms` | the wait with hover and prediction |
| `saved_ms` | `wait_hover_ms − wait_predictive_ms`: what prediction bought |

One `preload cost` event per report: `hover_fired` / `hover_used` and
`predicted_fired` / `predicted_used` — preloads started by each kind of
signal, and how many were followed by a click. The gap is wasted requests.

Read the two together. Average `saved_ms` is the benefit; the predicted
fired-to-used gap is the price.

Only route templates and timings leave the browser — never a URL, a param
value, or anything identifying the visitor. The endpoint is unauthenticated,
so it size-caps and shape-checks every report and rejects anything that is not
a template and numbers. With the variable unset (the default) the endpoint is
not mounted and browsers send nothing.

## What gets measured

- Every matched page and API route, labeled with its route template.
- The soft-nav prefetch endpoint, labeled `/__nx/prefetch<route>` so slow
  seed work is attributable to soft navigations separately from hard loads.
- `not_found` surfaces, labeled `__not_found`.
- Static files are not instrumented.

The cold-start headers (`x-nextrs-cold`, `x-nextrs-boot-id`) ride on the
same responses, and the summary event carries `cold` — so cold and warm
latency for a route are separable in whatever backend you query.
