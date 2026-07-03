# Seeding Handlers That Take `State<..>` / `Extension<..>`

- **Reported-in:** onenote-extractor (boundary found while widening fallible seeding)
- **Date:** 2026-07-03
- **Status:** open

## Problem

The seed companion calls the real handler, so it can only exist for handlers
whose arguments it can construct: `Path<..>` and `Query<..>` (values the
caller supplies). A handler taking `State<Db>` or `Extension<..>` is
companion-ineligible entirely — and real apps keep connections/config in
state, so their GETs can't be seeded through the wire contract at all
(apps fall back to querying the DB directly in `prefetch.rs`, bypassing the
handler).

## Proposed Direction (sketch — needs design)

Generate companions that accept each extractor's inner value as an argument,
in declared order: `get(State(db): State<Db>, Path(id): Path<i64>)` →
`__nextrs_seed_get(db: Db, id: i64, _ext)`. `prefetch.rs` passes the state it
has access to. Open questions:

- Where does `prefetch.rs` get the state? Apps construct it in `main.rs`; the
  registry is built without it. Possibilities: thread state through request
  extensions (the `_ext` slot was reserved for extension forwarding), or a
  `build_router_with_state` that inserts app state as an Extension so both
  middleware and prefetch fns can read it.
- Eligibility mirror in build.rs must track whatever shape the macro accepts.

## Validation

TBD with the design; must include a real State-taking handler seeded in a
worked example.
