# Live-reload long polling stalls headless browsers

- **Reported-in:** onenote-extractor
- **Date:** 2026-06-22
- **Status:** reported

## Problem

Generated pages include the tower-livereload long-poll script. That request
stays open by design, so a headless browser never reaches network quiescence:

```
chromium --headless --virtual-time-budget=4000 --screenshot=out.png <page>
```

hangs instead of exiting once the page is visually ready, because Chromium keeps
waiting on the still-active reload request.

Bounding it (`timeout 10s chromium --headless --screenshot=...`) does terminate,
but captures the early client-loading state — the timeout fires on wall-clock,
not on an app-specific readiness condition, so what you get is a screenshot of a
spinner.

This makes the cheapest possible visual check — screenshot a page, look at it —
unavailable in a nextrs dev server, which matters for agent-driven workflows
where that is the primary feedback loop.

## Proposal

- Suppress the live-reload script automatically for requests that are evidently
  headless/automated, or
- provide a documented opt-out — an env var (`NEXTRS_LIVERELOAD=0`) or a query
  parameter — that serves the page without the reload script.

An env var is probably the better primary: it composes with e2e runners and
screenshot tooling without every call site needing to rewrite URLs. A query
parameter is a useful secondary for one-off manual checks.

Whichever is chosen, it should be discoverable from the dev runner's startup
output, since the failure mode (a hanging command) gives no hint that live
reload is involved.

## Validation

- Assert the reload script is absent from the response when the opt-out is
  active, and present by default.
- End-to-end: run the bounded-screenshot command against a dev server with the
  opt-out set and assert it exits on its own well before the timeout.
