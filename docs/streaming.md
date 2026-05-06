# Streaming in nextrs

Streaming is the central UX feature of the framework. When a route has a `loading.{rs,html}` slot, the server sends the loading shell to the browser **before** the page handler has finished computing — and then sends the page content as a second chunk on the same response, swapping the loading shell out via a tiny inline `<script>`. No second HTTP request, no client-side framework, no htmx.

This document covers how it works internally, the local-vs-Vercel split, and how to verify it.

## The model

A request to a route with a `loading` slot produces a chunked HTTP response with this shape:

```
[layout-open]
<div id="__nx_slot__">
  …loading.html content…
</div>
                                    ← server awaits the page handler here
                                      (could be 100ms, could be 2s)
<template id="__nx_page__">
  …page.html content…
</template>
<script>
  // ~200 bytes inline
  var s = document.getElementById('__nx_slot__');
  var t = document.getElementById('__nx_page__');
  if (s && t) { s.replaceWith(t.content); t.remove(); }
</script>
[layout-close]
```

The browser parses incrementally as bytes arrive. The user sees the loading shell as soon as it's painted (typically <300ms TTFB). The page handler runs concurrently; when it resolves, its content arrives as a `<template>` tag (which the browser parses but doesn't render) followed by the swap script (which runs as soon as it's seen, at which point both the slot and the template are already in the DOM).

**Routes without a `loading` slot** skip the streaming machinery entirely and return one synchronous response.

## Internals

### Layout shell split

When a route has `loading`, the framework can't just wrap the response with `layout(loading)` and stream that — because the layout's `[layout-close]` half needs to come AFTER the page swap, not before.

`router.rs::layout_shell` solves this by:
1. Composing the layout chain around an internal sentinel string (`<!--__nx_content__-->`)
2. Splitting the rendered shell on the sentinel into `(before, after)` halves

So if the layout chain is `<html><body>{{children}}</body></html>`, then:
- `before` = `<html><body>`
- `after` = `</body></html>`

The streaming chunk order is then `before + slot_div + (await page) + page_template + swap_script + after`.

For routes without `loading`, the same split is used but `before + page + after` is sent as a single response — equivalent to `layout(page)`.

### Why layouts must use `{{ children|safe }}`

If a layout template uses `{{ children }}` (askama's escape-by-default), the framework's content marker gets escaped to `&#60;!--__nx_content__--&#62;` during the shell render, the split fails to find the marker, and the page renders **outside** the layout (concatenated after `</body></html>` because `before` ends up being the entire rendered shell and `after` is empty). Use `{{ children|safe }}`.

The static-layout helper (`nextrs::conventions::static_layout`) does literal string substitution and accepts both `{{children}}` and `{{ children }}`, so this gotcha only applies to `.rs` layouts via askama.

### The streaming primitive

`router.rs::render_route` builds the response body using `async-stream`:

```rust
let stream = async_stream::stream! {
    yield Ok::<Bytes, Infallible>(Bytes::from(before));
    yield Ok(Bytes::from(slot_div));         // arrives immediately
    let page_html = page_fn(req).await;      // suspends until page resolves
    yield Ok(Bytes::from(swap_chunk));       // template + swap script
    yield Ok(Bytes::from(after));
};
Body::from_stream(stream)
```

`async-stream` makes the `yield` between awaits incremental — the body's underlying channel pushes each chunk to the HTTP layer as it's produced. axum's `Body::from_stream` flushes each chunk as a separate frame.

## Local vs Vercel

### Local (`cargo run -p nextrs-example`)

axum's native `Body::from_stream` does the right thing out of the box. The request hits the example's axum router, the response streams over hyper's HTTP/1.1 chunked transfer encoding, the browser parses incrementally. Nothing special required.

### Vercel

Vercel functions go through `vercel_runtime::run`. The provided `vercel_runtime::axum::VercelLayer` adapts an axum router to Vercel's request/response shape — but its response handling has a subtle bug for our use case.

`VercelLayer` decides whether to stream a response by checking its content-type:

```rust
// from vercel_runtime::axum::StreamingUtils
fn is_streaming_response(headers: &HeaderMap) -> bool {
    headers.get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.contains("text/event-stream") || ct.contains("application/json"))
        .unwrap_or(false)
}
```

We send `text/html`, which doesn't match. So `VercelLayer` calls `axum::body::to_bytes(body, usize::MAX)` and ships the response as one frame. Symptom on a deployed app: TTFB ≈ total response time on streaming routes; `curl --no-buffer --trace-time` shows a single `Recv data` event.

### `nextrs::vercel::StreamingVercelLayer`

The framework ships a drop-in replacement, gated by the `vercel` cargo feature:

```toml
[dependencies]
nextrs = { path = "...", features = ["vercel"] }
```

```rust
use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;

let app = ServiceBuilder::new()
    .layer(StreamingVercelLayer::new())
    .service(router);
vercel_runtime::run(app).await
```

`StreamingVercelLayer` does the same request-side conversion as `VercelLayer` (collect body bytes → axum::Body, run the inner axum service) but unconditionally calls `StreamingUtils::create_stream_body` for the response, bypassing the content-type gate. ~70 lines in `nextrs/src/vercel.rs`.

Non-streaming responses are unaffected — they arrive as a single frame either way.

**Worth filing upstream eventually:** either an `always_stream` flag on `VercelLayer`, or extend `is_streaming_response` to recognize `text/html`. (TODO; needs to go to https://github.com/vercel/vercel.)

## Verifying streaming

### In tests

`router.rs` has three tests that cover streaming:

| Test | What it proves |
|---|---|
| `test_loading_arrives_before_page_resolves` | The loading shell's frame arrives at <100ms while the page handler is sleeping for 200ms. The page chunk's frame arrives ≥200ms (after the sleep). Real timing assertions. |
| `test_loading_stream_yields_multiple_frames` | The body has more than one frame — proves the response isn't being coalesced before the handler resolves. |
| `test_loading_stream_contains_loading_then_page` | Substring positions in the concatenated body confirm the order: layout-open < slot div < loading text < page template < page content < swap script < layout-close. |

These run in-process via `tower::ServiceExt::oneshot` against the framework router. They don't exercise the Vercel adapter (which is an opaque shim around `VercelService`'s request handling), but they prove the framework's stream production is correct.

### Against a running server

Locally:
```bash
curl --no-buffer --trace-time --trace - http://localhost:3000/with-loading 2>&1 | grep "<= Recv data"
```

Should show two or more `Recv data` events with timestamps separated by approximately the page handler's sleep time. If you see one event, streaming isn't working.

On Vercel (preview URL needs `x-vercel-protection-bypass` if SSO is enabled):
```bash
curl --no-buffer --trace-time --trace - \
  -H "x-vercel-protection-bypass: $TOKEN" \
  https://your-deployment.vercel.app/with-loading 2>&1 \
  | grep "<= Recv data"
```

Verified arrival pattern from a real deploy:
- T+0.000s — 914 bytes (layout open + loading shell)
- T+0.842s — 1703 bytes (page template + swap script + most of layout close)
- T+0.844s — 25 bytes (final layout close)

842ms gap matches the example's 800ms simulated data fetch + ~40ms of framework + network overhead.

### The smoke test that catches buffering

The simplest end-to-end check:
```bash
curl -o /dev/null -w "TTFB=%{time_starttransfer}s total=%{time_total}s\n" $URL/with-loading
```

If `TTFB ≈ total`, streaming is broken (or there's no loading slot). If `TTFB << total` and the gap matches the page sleep, streaming is working.

## What we're not doing (yet)

- **Suspense-style nested streaming.** React Server Components render placeholders inline and stream `<template>` chunks for each `<Suspense>` boundary as they resolve, with a runtime that splices them in. We do exactly one swap per route (one loading slot, one page swap). Adding nested boundaries would need either (a) a more sophisticated streaming protocol or (b) handing the user a `Suspense` primitive they wrap their page content in. Out of scope for now.
- **Cancellation.** If a client disconnects mid-stream, the framework should drop the in-flight page handler. We rely on tokio's drop semantics — not specifically tested.
- **Backpressure.** If the client is consuming slowly, we'll buffer in the channel. Not specifically bounded; relies on `async-stream`'s default buffering.
- **Error handling.** If the page handler panics or returns an error, the loading shell has already shipped. The browser shows the loading state forever (or whatever timeout the user has). A real fix needs `error.{rs,html}` segment convention and an error frame in the stream.

These are future work, not blockers for the current single-loading-slot model.
