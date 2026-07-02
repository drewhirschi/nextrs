# Route Params for `.tsx` Pages and `props.rs`

- **Reported-in:** onenote-extractor
- **Date:** 2026-07-01
- **Status:** open

## Problem

`app/source/[id]/page.tsx` needs its route param, but nothing in the codegen or
client runtime exposes it. The app hand-rolled a `useSourceId()` helper that
regex-parses `window.location.pathname`. `props.rs` receives only the raw
`http::Request`, so seeding data for a param'd route means parsing the URI by
hand.

## Proposed Direction

The server matched the route, so it should hand the params over:

- The generated shell handler for a dynamic tsx route extracts the matched
  params (axum has them after routing) and streams them as a JSON script tag
  (`__nx_params__`) before the mount div — same mechanism as seeds.
- The bundle entry wrapper reads the tag and passes them Next.js-style:
  `export default function Page({ params }: { params: { id: string } })`.
- The scaffolded client runtime gains `useParams()` for deep components.
- For routes with dynamic segments, the generated handler calls
  `props(req, params)`; paramless routes keep `props(req)` unchanged.

## Implementation Notes

- New `nextrs::Params` (string map + `to_script_tag()` with `<` escaping,
  mirroring `QuerySeed`). Extraction via `axum::extract::RawPathParams` from
  the request parts.
- Dynamic-ness is decided by the route's URL pattern (`{seg}` present), so no
  source parsing of `props.rs` is needed.
- Values are strings (they come from the URL); typed parsing is the caller's
  concern (`params.get("id")?.parse()`).

## Validation

- build.rs test: dynamic tsx route emits params extraction + two script tags;
  static route emits `static_page` as before.
- bundle.rs test: entry wrapper reads `__nx_params__` and passes the prop.
- macro-level: none (server-side only).
