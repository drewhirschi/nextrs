# Seeding Handlers That Take `State<..>` / `Extension<..>`

- **Reported-in:** onenote-extractor (boundary found while widening fallible seeding); re-raised by finstream (`Extension<AppContext>` GETs lose their companions)
- **Date:** 2026-07-03 (design 2026-07-18)
- **Status:** open — designed, ready to implement

## Problem

The seed companion calls the real handler, so it can only exist for handlers
whose arguments it can construct: `Path<..>` and `Query<..>` (values the
caller supplies). A handler taking `State<Db>` or `Extension<..>` is
companion-ineligible entirely — and real apps keep connections/config in
state, so their GETs can't be seeded through the wire contract at all
(apps fall back to querying the DB directly in `prefetch.rs`, bypassing the
handler). `WaitUntil` (nextrs 0.4.1) joined the disqualifying list the same
way.

## Design (2026-07-18)

**Source the new extractors from `_ext` inside the companion — do not widen
the companion signature.** The original sketch (companion takes each
extractor's inner value as an argument, prefetch.rs supplies it) dissolved
once we traced where extensions actually flow:

- Every companion already accepts `_ext: &http::Extensions`, and every
  prefetch.rs already passes `req.extensions()`.
- Both prefetch call sites hand the companion a *real* request that has
  already been through the app's layer stack: the SSR shell handler receives
  the page request (wrapped by `main.rs`'s `.layer(Extension(ctx))`), and the
  `/__nx/prefetch` endpoint lives in the same router *and* explicitly runs
  the route's middleware chain first. So `Extension`-installed state and the
  Vercel-injected `WaitUntil` are sitting in `_ext` at both sites today.

So the "where does prefetch.rs get the state?" open question answers itself:
it already has it. The companion just needs to look.

### Macro changes (`seed_companion`)

Argument classification becomes:

| arg type | companion behavior |
| --- | --- |
| `Path<..>` (≤1) | unchanged — caller-supplied, substitutes into the URL/key |
| `Query<T>` (≤1) | unchanged — caller-supplied, hashed into the key |
| `Extension<T>` (any count) | `_ext.get::<T>().cloned()` → `Extension(v)`; **missing → seed nothing** |
| `WaitUntil` | `_ext.get::<WaitUntil>().cloned().unwrap_or_default()` — infallible |
| anything else | no companion (unchanged) |

- Any `Extension` arg makes the companion return `Option<SeedEntry>` even
  for an infallible handler: a missing extension seeds nothing and the page
  degrades to fetch-on-mount, exactly like the fallible-handler `Err` path.
  `QuerySeed::seed` already accepts both shapes, so prefetch.rs call sites
  are untouched.
- **The query key is unchanged: URL + query params only.** Extension args
  are server-side context invisible to the client hook; they must never
  enter the key or seeded keys stop matching hook keys. This invariant is
  the core of the design review.
- `WaitUntil` sourced from `_ext` means seeding gets *real* waitUntil
  semantics on Vercel (the page request's extensions carry the
  AppState-backed one injected by `StreamingVercelLayer`); locally the
  `unwrap_or_default()` is the detached tokio::spawn fallback — identical to
  the extractor's own behavior.
- Declared argument order is preserved via the existing `call_order`
  mechanism; `Extension<T>` needs no new bounds (axum already requires
  `T: Clone + Send + Sync + 'static`).
- Type recognition is textual (last path ident `Extension` / `WaitUntil`),
  same as `Path`/`Query` today; a user-defined type named `WaitUntil` would
  be misclassified — accept and document, consistent with existing macro
  limits.

### `State<T>` stays out (deliberately)

`State` handlers require `Router::with_state`; the registry builds a
stateless router, so such handlers don't route through nextrs conventions
today anyway. The paved road for seedable-and-stateful is: install context
with `.layer(Extension(ctx))` in `main.rs` (or a `middleware.rs` that
inserts it), which this design makes fully seedable. Document that in the
typesafe-client/conventions docs as part of the implementation.

### build.rs mirror

`get_is_seed_eligible` (textual mirror) must accept `Extension<..>` and
`WaitUntil` args in the same positions, and `ret_is_seedable` is unchanged.
Extend the `seed_eligibility_mirrors_macro` test with both shapes so the
mirror can't drift.

### Open questions (small)

- Silent degradation: a missing extension yields an unseeded page with no
  signal. v1 ships silent (matches fallible-`Err` behavior); if it bites,
  add a debug-assertions-only `eprintln!` in the companion.
- Same-type `Extension<T>` twice: both get the same value — harmless,
  no special-casing.

## Implementation Notes

1. `crates/nextrs-macros/src/lib.rs` — extend arg collection + `call_args`
   emission per the table; new fallibility rule (Extension ⇒ `Option`).
2. `crates/nextrs/src/build.rs` — mirror in `get_is_seed_eligible`.
3. `examples/react-todos` — move the todos store behind an
   `Extension<TodosCtx>` installed in `main.rs`/`api/index.rs` so the seeded
   GET demonstrates the feature (per CLAUDE.md, the demo must show it).
4. Docs: conventions + typesafe-client pages — "seedable handlers" section
   gains Extension/WaitUntil rows and the State guidance.

## Validation

- Macro tests: `Extension<T>` GET emits a companion returning
  `Option<SeedEntry>` that reads `_ext`; `WaitUntil` GET emits an infallible
  companion; `State<T>` still emits none; mirror test extended to match.
- react-todos: page renders seeded with the Extension-backed handler
  (script-tag seed present, no fetch on first paint); removing the layer
  degrades to fetch-on-mount, not a 500.
- Vercel path: seeded page + `wait_until` in the same GET registers work
  with the AppState awaiter (extend the `nextrs::vercel` injection test).
