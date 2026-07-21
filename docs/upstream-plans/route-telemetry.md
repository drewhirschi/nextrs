# Route Telemetry (diagnosis-first)

- **Reported-in:** framework roadmap (Drew, planning session)
- **Date:** 2026-07-20
- **Status:** fixed in b508675 + 64eba47 + 0fde0ec

## Problem

There was no way to diagnose a slow route. The framework recorded nothing
per-request: no per-route timing, no tracing instrumentation, no breakdown of
where time went (middleware vs seed vs handler). The only telemetry was
cold-start classification (`health::stamp` headers + external pinger).

## Proposed Direction

Progressive diagnosis ladder, framework-user-facing:

1. **`Server-Timing` header** (default on, `NEXTRS_SERVER_TIMING=0` opt-out):
   per-request breakdown readable in DevTools' Network tab. Zero setup.
2. **`Timing` extractor** — `timing.span("db", fut).await` adds app-defined
   segments to the same header/events for depth.
3. **`tracing`** — `nextrs.route` span + per-request summary event with OTel
   semconv fields. Escalation to a collector is via `tracing-opentelemetry`
   (docs recipe); the framework takes no OTel dep.

Explicitly cut/deferred:

- **Custom JSON push sink** (`TelemetrySink` trait + `waitUntil` emit +
  Turso reference consumer): redundant with the OTel path for diagnosis —
  a sink-to-OTLP adapter would reimplement exporters and flatten span trees.
  Revisit only if users want raw JSON in their own store without a collector.
- **In-process metrics scrape endpoint**: fights serverless (ephemeral,
  horizontally-scaled instances make per-instance counters misleading).
- **Dev-only `/__nx/routes` ring-buffer page**: viewing is covered by
  DevTools + logs; revisit if demand appears.

## Implementation Notes

- `crates/nextrs/src/telemetry.rs`: `RouteTelemetry` (Arc'd, shared handler ↔
  layer ↔ stream), `Timing` extractor, `start_segment` guard, `record` layer.
- `router.rs`: handlers create the handle, put it in request extensions (for
  `Timing`) and response extensions (for the layer); streaming pages record
  handler time from inside the body stream with idempotent emit + Drop
  backstop. Prefetch endpoint labeled `/__nx/prefetch<route>`; not-found
  labeled `__not_found`.
- `record` is layered outside `health::stamp` so the summary event carries
  the cold flag.
- Codegen: generated shell handlers time the seed await as the `seed`
  segment; `#[nextrs::api]` seed companions accept a `Timing` arg (sourced
  from `_ext` like `WaitUntil`) with the build-time eligibility mirror
  updated to match.

## Validation

- `cargo test -p nextrs --features build,tsx,vercel` and `-p nextrs-macros`:
  header format on page/API routes, streaming partial header, middleware-
  rejection header, static files untouched, prefetch labeling, companion
  eligibility both sides.
- react-todos run end-to-end: `curl -i /api/todos` shows
  `mw/db/handler/total`; `/` shows `db` + `seed` (seed companion's segment
  lands in the page breakdown); `RUST_LOG=nextrs=info` shows summary events;
  `NEXTRS_SERVER_TIMING=0` removes the header.
- `cargo build -p site` with bundling on; landing page loads.
