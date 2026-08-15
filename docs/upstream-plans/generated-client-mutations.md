# Generated clients need a discoverable mutation path

- **Reported-in:** onenote-extractor
- **Date:** 2026-06-23
- **Status:** reported

## Problem

While adding update flows for source metadata and region review dates, it was
easier to hand-write

```ts
fetch(`/api/sources/${id}`, { method: "PATCH", ... })
```

from the page component than to find and use the generated client's update
operation.

That defeats the point of generating a client. The hand-rolled call
reintroduces exactly what codegen exists to eliminate: a duplicated route
string, a duplicated request shape, and a hand-maintained response type that can
drift from the server without anything failing to compile.

The mutation operations *are* generated. They are just less discoverable than
the query hooks, and the scaffold's examples show read flows, so the read path
is the one that gets learned. When the ergonomic gap and the documentation gap
point the same direction, app authors end up back on raw `fetch`.

## Proposal

- Ensure mutation operations (POST/PATCH/DELETE) get obvious, predictable names
  and ergonomic React hooks, at parity with the query side.
- Include mutation examples in the scaffold and in
  `docs/typesafe-client-codegen.md`, not only read/query flows — with the
  invalidation and optimistic-update patterns spelled out, since "how do I
  refresh the list after the PATCH" is the next question and its absence is part
  of why raw `fetch` wins.
- Consider surfacing the available operations somewhere a reader will trip over
  them (generated barrel doc comment, or the runner's codegen output).

## Validation

- Scaffold a fresh app and confirm a mutation flow can be written end to end
  from the generated client without reading the framework source.
- Assert generated mutation hooks exist and are exported from the barrel for a
  fixture spec containing POST/PATCH/DELETE operations.
