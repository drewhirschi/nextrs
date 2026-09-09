# Realtime Collections and Durable Object Coordination

- **Reported-in:** daily_mirror
- **Date:** 2026-09-01
- **Status:** open
- **Prototype:** `codex/realtime-streams-prototype`

## Problem

Daily Mirror will grow from one gallery into household-scoped galleries where
several people can upload and edit photos. The current page treats the API as a
snapshot and polls every 60 seconds. A new upload should instead become an
ordered event for everyone attached to that household, without making every
screen hand-roll WebSockets, cache invalidation, filtering, or pagination.

The framework needs two distinct primitives:

1. a provider-neutral way to attach to an authenticated resource and receive
   ordered database-shaped changes; and
2. a client collection that can keep filters, ordering, and loaded page
   windows correct as those changes arrive.

Those responsibilities should not be collapsed into a second application
database hidden inside a connection server.

## Research Findings

Research was refreshed on 2026-09-01 against primary documentation.

### Durable Objects are a good coordinator, with one correction

The useful mental model is not “a Durable Object is the realtime database.” It
is “a Durable Object is the single ordered meeting point for one coordination
atom.” Cloudflare explicitly recommends one object per logical unit such as a
chat room, game, user, or tenant, and warns against a global singleton. For
Daily Mirror, the first coordination atom should be a household, with resource
names inside its event envelope.

The browser attaches to the household object over a hibernatable WebSocket.
Cloudflare keeps those client sockets connected while evicting the object's
memory, and restores per-socket metadata from serialized attachments. A
deployment or platform shutdown can still terminate sockets, so reconnect and
snapshot reconciliation remain mandatory. Current platform limits allow up to
32,768 sockets per object at the API level, although CPU/memory make the
practical ceiling workload-dependent.

Cloudflare recommends SQLite-backed Durable Objects for new namespaces. That
storage is useful for a small durable sequence, room metadata, presence, or a
bounded replay buffer. It should not automatically become the system of record
for photos, accounts, and application queries; doing so would split the data
model and couple NextRS applications to one realtime provider.

Sources:

- [Cloudflare: rules of Durable Objects](https://developers.cloudflare.com/durable-objects/best-practices/rules-of-durable-objects/)
- [Cloudflare: WebSocket hibernation](https://developers.cloudflare.com/durable-objects/best-practices/websockets/)
- [Cloudflare: lifecycle and shutdown behavior](https://developers.cloudflare.com/durable-objects/concepts/durable-object-lifecycle/)
- [Cloudflare: Durable Object limits](https://developers.cloudflare.com/durable-objects/platform/limits/)
- [Cloudflare: SQLite-backed storage](https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/)

### “TanStack Stream” is now best expressed as TanStack DB

TanStack DB extends TanStack Query with normalized collections and live
queries. Its live query engine incrementally maintains filters, ordering,
joins, limits, and offsets when rows enter, change, or leave a collection. Its
Query Collection adapter can load through an existing REST/TanStack Query API,
and its direct-write methods are explicitly intended for WebSocket or SSE
deltas. `useLiveInfiniteQuery` provides a bounded, expanding page window.

This is a better division of labor than teaching NextRS how to reimplement a
client relational engine. NextRS should generate the typed snapshot and change
transport; TanStack DB should decide which visible query results change.

Sources:

- [TanStack DB overview](https://tanstack.com/db/latest/docs/overview)
- [TanStack DB live queries](https://tanstack.com/db/latest/docs/guides/live-queries)
- [TanStack DB Query Collection and realtime direct writes](https://tanstack.com/db/latest/docs/collections/query-collection)
- [TanStack DB React adapter](https://tanstack.com/db/latest/docs/framework/react/overview)

### Turso can remain authoritative, but it is not the socket broker today

There are two Turso generations to distinguish:

- Existing libSQL-backed Turso Cloud provides remote SQLite and legacy
  embedded replicas. Replicas synchronize periodically or explicitly; other
  replicas observe changes on their next sync.
- The new Turso database engine has native CDC tables and live materialized
  views. CDC records inserts, updates, and deletes in a queryable table. The
  hosted engine began broad early preview in August 2026 with concurrent
  writes.

The reviewed Turso Cloud documentation does not currently expose a hosted,
low-latency WebSocket subscription/change-feed API that a Durable Object can
consume. CDC is valuable for a future durable relay or outbox consumer, but a
Durable Object polling a CDC table would sacrifice hibernation and add latency.

The recommended Turso path now is therefore:

1. Commit the domain mutation in Turso.
2. In the same transaction, optionally append a compact outbox row.
3. After commit, publish the typed change to the household Durable Object.
4. Treat that broadcast as a latency optimization, never the source of truth.
5. On publish failure, leave the committed data intact; a retrying outbox
   worker restores delivery, while reconnecting clients refetch regardless.

For a prototype where every write goes through the NextRS API, direct
post-commit publish is enough. Add the outbox before claiming guaranteed
delivery or supporting writers that bypass the application.

Sources:

- [Turso Rust SDK and sync behavior](https://docs.turso.tech/sdk/rust/reference)
- [Turso embedded replicas](https://docs.turso.tech/features/embedded-replicas/introduction)
- [Turso CDC PRAGMA reference](https://docs.turso.tech/sql-reference/pragmas)
- [Turso Cloud engine early preview](https://turso.tech/blog/concurrent-writes-on-turso-cloud)
- [Turso live materialized views](https://turso.tech/blog/introducing-real-time-data-with-materialized-views-in-turso)

### Database compatibility is an event-source question

| Database/source | Best event seam | NextRS adapter shape | Caveat |
|---|---|---|---|
| Turso Cloud (libSQL) | Application publish + transactional outbox | Rust publisher posts committed changes to the room | No documented hosted push feed; external writers need outbox/CDC reconciliation |
| Turso engine | CDC table or application outbox | Cursor-based CDC relay when hosted consumption matures | CDC records changes; it does not itself deliver browser sockets |
| PostgreSQL | Transactional outbox, `LISTEN`/`NOTIFY`, or logical replication | Long-running relay publishes to rooms | `NOTIFY` is a signal with a small payload, not durable replay |
| Durable Object SQLite | Write and broadcast inside one object | Provider-native adapter | Excellent per-room consistency, but provider-coupled and capped at 10 GB per object |
| Any transactional store | Domain event/outbox written with the mutation | Poll/drain outbox into the same protocol | Adds relay operations, but gives the most portable guarantee |

PostgreSQL delivers `NOTIFY` only after commit and preserves commit order, so it
is a convenient wake-up signal for a relay. The PostgreSQL docs recommend
putting larger structured data in a table and sending only its key, which is
the same durable-outbox principle.

Source: [PostgreSQL `NOTIFY`](https://www.postgresql.org/docs/current/sql-notify.html)

## Proposed Direction

### Architecture

```text
                         snapshot / filters / pages
browser + TanStack DB  ─────────────────────────────►  NextRS API  ──► Turso
         │                                                │              │
         │ WebSocket                                      │ after commit │
         ▼                                                ▼              │
Cloudflare Worker ──► Durable Object per household ◄── typed change      │
         │                     │                                         │
         └──────── ordered change frames ────────────────────────────────┘

On connect, reconnect, or sequence gap: browser refetches the Turso-backed snapshot.
```

The public protocol has three frames:

```json
{"type":"ready","topic":"household.123.photos","sequence":41}
{"type":"change","topic":"household.123.photos","sequence":42,"operation":"upsert","key":"photo-id","value":{"id":"photo-id"}}
{"type":"resync","topic":"household.123.photos","sequence":57}
```

`delete` carries a key; `invalidate` carries neither key nor value and requests
a full snapshot. Sequences are monotonic within a topic, not globally.

### Authentication boundary

Knowing a household ID must never authorize attachment. The edge Worker must
authenticate before selecting a Durable Object. The prototype calls a NextRS
authorization route with the browser's cookie. A production implementation
may replace that extra request with a short-lived, household-scoped signed
ticket. Publishers use a separate server credential.

### Delivery is recoverable, not exactly once

Clients must tolerate duplicates, reconnects, and gaps. `upsert` is naturally
idempotent by key. Deletes of absent keys are harmless. The authoritative
snapshot repairs anything else. This is a much cheaper and more honest
contract than promising exactly-once browser delivery.

## Product / UX Policies

The event transport should not decide what a person sees. The prototype makes
three policies switchable at runtime:

| Policy | Behavior | Good default for | Cost |
|---|---|---|---|
| Auto merge | Apply the row delta immediately | collaboration, chat, status, visible shared work | A sorted page boundary can move under the person's pointer |
| Auto refetch | Event invalidates and reloads the current snapshot | complex server projections or events without a safe row delta | More reads and possible loading indicators |
| Notify first | Count changes and refetch when clicked | galleries, feeds, search results, careful reading | Data is intentionally stale until accepted |

For Daily Mirror, notify-first is the recommended default while someone is
scrolled away from the top or has a lightbox open. Auto-merge is safe at the
top of the newest-first gallery when no focused interaction would move. That
suggests an eventual adaptive policy rather than one global choice:

```text
at top + idle      → merge new photo into the first page
scrolled / focused → “3 new photos” banner
edit/delete open   → patch the affected visible item, preserve position
sequence gap       → silent snapshot reconciliation, then resume
```

TanStack DB should own the mechanics. If the loaded window is 50 items and a
new matching item sorts first, auto-merge keeps the window bounded: the new row
enters and the old boundary row leaves. Notify-first leaves the window stable
until the person refreshes. Filters run as live query expressions, so an
updated object automatically enters or exits the visible result.

## Prototype Surface

This branch contains:

- `nextrs::realtime::MemoryRealtime`, a tested single-process WebSocket broker
  for local development;
- `@app/client/realtime`, a generated browser subscription helper with
  reconnect, ordered sequence checking, and explicit `ready`/`resync` hooks;
- `/realtime` in `react-todos`, a TanStack DB collection with live filtering,
  bounded infinite paging, and all three UX policies;
- `examples/react-todos/realtime-worker`, a deployable SQLite-backed Durable
  Object adapter using hibernatable WebSockets and the same protocol.

The memory broker is intentionally named and documented as local-only. The
Durable Object currently accepts explicit publish calls; wiring the CLI to
generate/deploy it and adding the Rust HTTP/outbox publisher are the next
infrastructure steps if the interaction model proves useful.

## Implementation Notes

Before stabilizing a framework API:

- Decide whether topic scope defaults to tenant/household or to a narrower
  collection. Household-first reduces socket count and permits batched events;
  collection-first reduces irrelevant fan-out.
- Batch bursts (for example, face-processing updates) into fewer WebSocket
  messages. Cloudflare specifically recommends batching logical events.
- Specify maximum event size, replay retention, and whether the first stable
  version includes a bounded Durable Object replay log.
- Generate event TypeScript from Rust payload types rather than leaving
  application authors to duplicate them.
- Connect optimistic TanStack DB mutations to the server-observed event so
  optimistic state resolves only when the committed version returns.
- Add signed topic tickets or a reusable authorization callback convention.
- Extend `nextrs generate/deploy` from its existing Cloudflare cron sidecar to
  a separate realtime Worker, without mixing cron and Durable Object lifecycle.

## Validation

- Rust unit tests prove topic-local ordering and cursor advancement without
  subscribers.
- Generated-client tests require the realtime export and declaration files.
- TypeScript builds the TanStack DB collection without handwritten API types.
- Wrangler dry-run validates the Durable Object binding and migration.
- End-to-end: open `/realtime` twice, mutate in either tab, and verify merge,
  refetch, and notify-first behavior with both filtering and a loaded second
  page.
- Planned before stabilization: overflow a deliberately tiny memory channel,
  verify a `resync` frame, and verify the collection converges after refetch.
