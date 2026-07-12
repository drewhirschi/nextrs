+++
title = "Streaming"
description = "How loading slots stream the shell before the page resolves — and how to verify it"
section = "Guides"
order = 3
+++

nextrs has two rendering models. **React `.tsx` pages** mount in the browser and seed their TanStack React Query cache from a `prefetch.rs` sibling; **Rust/HTML pages** (`page.{rs,html}`) render on the server and stream. Streaming is the central UX feature of that second path.

When a Rust/HTML route has a `loading.{rs,html}` slot, the server sends the loading shell to the browser **before** the page handler has finished computing — then sends the page content as a second chunk on the same response, swapping the shell out with a tiny inline script. One HTTP request, and on this path no client-side framework and no htmx — React Query only runs on the `.tsx` path.

## The model

A request to a Rust/HTML route with a `loading` slot produces a chunked response shaped like this:

```
[layout-open]
<div id="__nx_slot__">
  …loading content…
</div>
                                    ← server awaits the page handler here
                                      (could be 100ms, could be 2s)
<template id="__nx_page__">
  …page content…
</template>
<script>
  // ~200 bytes inline
  var s = document.getElementById('__nx_slot__');
  var t = document.getElementById('__nx_page__');
  if (s && t) { s.replaceWith(t.content); t.remove(); }
</script>
[layout-close]
```

The browser parses incrementally as bytes arrive: the user sees the loading shell as soon as it paints (typically under 300ms TTFB). When the page handler resolves, its content arrives inside a `<template>` (parsed but not rendered), and the swap script replaces the slot with it.

Routes **without** a `loading` slot skip the streaming machinery and return one synchronous response.

## How the layout splits

The layout's closing half (`</body></html>`) has to arrive *after* the page swap. The framework composes the layout chain around an internal sentinel comment, then splits the rendered shell on it into `(before, after)` halves. The streamed order is `before + loading slot + (await page) + page template + swap script + after`.

This is why **Askama layouts must use `{{ children|safe }}`**: with plain `{{ children }}`, Askama escapes the sentinel, the split fails to find it, and your page renders outside the layout. (Static `.html` layouts do literal substitution and aren't affected.)

## Middleware runs first

All matching `middleware.rs` handlers run root-to-leaf **before** the loading shell is sent — once the first chunk ships, the status and headers are committed. That ordering means auth checks and redirects in middleware return real HTTP status codes even on streaming routes. Put fast request guards in middleware; put slow data work in the page and let the loading shell cover it.

## Verifying streaming works

The smoke test that catches buffering anywhere in the stack:

```bash
curl -o /dev/null -w "TTFB=%{time_starttransfer}s total=%{time_total}s\n" \
  http://localhost:3000/with-loading
```

If `TTFB ≈ total`, streaming is broken (or the route has no loading slot). If `TTFB << total` and the gap matches the page's work time, it's streaming.

To see the individual chunks:

```bash
curl --no-buffer --trace-time --trace - http://localhost:3000/with-loading 2>&1 \
  | grep "<= Recv data"
```

Two or more `Recv data` events, separated by roughly the page handler's duration, means it's working. A real deploy of the demo's `/with-loading` route (800ms simulated work) shows the first frame at T+0.000s and the page frame at T+0.84s.

## Deploy targets

Locally, axum's `Body::from_stream` streams over chunked transfer encoding with no extra setup. On Vercel, the stock adapter buffers `text/html` responses — the framework ships a drop-in fix. See [Deploy to Vercel](/docs/deploy-vercel#streaming-through-the-vercel-adapter).

## Current limits

- **One swap per route.** No Suspense-style nested boundaries (yet) — one loading slot, one page swap.
- **No error frames.** If the page handler panics after the shell shipped, the browser keeps the loading state. An `error.{rs,html}` convention is on the roadmap.
