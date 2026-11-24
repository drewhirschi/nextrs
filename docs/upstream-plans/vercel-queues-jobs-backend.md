# Vercel Queues as the jobs backend on Vercel — the Rust push consumer works

- **Reported-in:** onenote-extractor
- **Date:** 2026-11-24
- **Status:** fixed in f5b8261 (framework), 6b91fa0 (deploy), a19c874 (scaffold + react-todos)
- **Related:** [background-jobs.md](../background-jobs.md) ("Vercel Queues as a
  backend": this answers its two open questions), [jobs-and-logs.md](../jobs-and-logs.md)

## Problem

Jobs v1/v2 run behind `WaitUntil` with rows in the app's database, and need a
cron-driven sweep (`/__nx/jobs/sweep`) to redeliver failed or stuck jobs. On
Hobby, Vercel cron runs at most daily, so anything past the 60 s self-retry
waits a long time. Vercel Queues would replace the sweep with real redelivery,
but `background-jobs.md` left the linchpin unverified: can a vercel-rust
function carry a `queue/v2beta` trigger, and what request does it receive?

onenote-extractor wants this for "accept a box now, crop the page image in the
background" (the crop is about 1 s of a 1.4 s request).

## What the spike found (2026-11-24, team on Hobby, region pdx1)

Throwaway preview deploy of onenote-extractor with a Rust function
(`api/queue_spike.rs`, plain `vercel_runtime` + axum) and a Node function, each
with a trigger in `vercel.json`:

```json
"functions": {
  "api/queue_spike.rs": {
    "experimentalTriggers": [
      { "type": "queue/v2beta", "topic": "spike-rust", "retryAfterSeconds": 60, "maxDeliveries": 3 }
    ]
  }
}
```

1. **It builds.** `vercel build` copies the trigger into the Rust function's
   `.vc-config.json` and adds a generated consumer group name
   (`"consumer": "api_Squeue__spike_Drs"` — the file path, escaped). No runtime
   support was needed. Since nextrs writes prebuilt output, it could also write
   this block into `.vc-config.json` itself.
2. **It deploys** on Hobby, as a prebuilt deploy.
3. **Vercel calls the Rust function.** A message sent to the topic arrived at
   the function about 0.2 s later as a plain HTTP request:
   - `POST /api/queue_spike` (the function's own path), `user-agent: vercel-queue/v2beta`
   - **the body is the message payload, verbatim**, with the sender's `content-type`
   - CloudEvents headers carry the metadata: `ce-type: com.vercel.queue.v2beta`,
     `ce-vqsmessageid`, `ce-vqsqueuename` (topic), `ce-vqsconsumergroup`,
     `ce-vqsdeliverycount` (starts at 1), `ce-vqsreceipthandle`,
     `ce-vqscreatedat`, `ce-vqsexpiresat`, `ce-vqsvisibilitydeadline`
     (5 minutes after delivery), `ce-vqsregion`
   - so the consumer does not have to call ReceiveMessageById: everything is in
     the request.
4. **A consumer function is private.** Calling its URL from outside does not
   reach it (the request fell through to the app's catch-all rewrite and got a
   404 from the main function). So no shared secret is needed, and the
   `NEXTRS_JOBS_SECRET` machinery is unnecessary for this backend.
5. **Sending is per environment.** `POST https://{region}.vercel-queue.com/api/v3/topic/{topic}`
   with `Authorization: Bearer <OIDC token>`. A token from `vercel env pull` is
   a *development* token: with `Vqs-Deployment-Id` of a preview deployment it is
   refused (403 "names a deployment in a different environment than this
   token"), and without that header the message is accepted but never reaches
   the deployment's consumers. Sent from inside the deployment (the
   `x-vercel-oidc-token` request header, plus `Vqs-Deployment-Id:
   $VERCEL_DEPLOYMENT_ID`) it is delivered. The plain REST round trip
   (send / receive / acknowledge) also still works with the development token.

6. **A 2xx response does not acknowledge the message.** Both spike consumers
   answered `200` and did nothing else. Each message was delivered again 5
   minutes later (at its `ce-vqsvisibilitydeadline`), and then again every 60 s
   (`retryAfterSeconds`): deliveries at 21:56:27, 22:01:32, 22:02:32, 22:03:32.
   So the consumer must call the Acknowledge endpoint
   (`DELETE …/topic/{topic}/consumer/{group}/lease/{receiptHandle}`) itself;
   the JS SDK's `handleCallback` does this for JS consumers. Note this was four
   deliveries with `maxDeliveries: 3` at the time the log was read, so do not
   rely on that cap being exact.

## Proposed Direction

Make Vercel Queues the **default delivery** for jobs on Vercel. The user
surface does not change (`app/jobs/<name>/job.rs`, `#[nextrs::job]`,
call-to-enqueue, `JobHandle`, the admin dashboard). What changes is how a
queued row reaches a runner, and what redelivers it.

Today: `enqueue` inserts the row, then `deliver()` self-POSTs
`/__nx/jobs/<name>` with `NEXTRS_JOBS_SECRET`; the run route answers 202 and
runs behind `WaitUntil`; a cron-driven sweep redelivers due and stale rows.

With Queues (on Vercel, when the deployment has a consumer function):

- **Rows stay.** The job row in the `JobStore` is still the record (payload,
  attempts, history, result), so `JobHandle::status()`, the dashboard and
  "Retry" keep working. The message carries only the envelope
  (`{ id, name }`), like the self-POST does now.
- **Deliver = SendMessage** to one topic (`nx-jobs`), with
  `Vqs-Deployment-Id: $VERCEL_DEPLOYMENT_ID` and the OIDC token
  (`x-vercel-oidc-token` from the request being served when there is one,
  else `VERCEL_OIDC_TOKEN`). `deliver()` grows a delay argument, sent as
  `Vqs-Delay-Seconds`.
- **Run = a consumer function.** A second function (`api/jobs.rs`, generated
  by the scaffold) builds the same app router and wraps it in
  `nextrs::jobs::vercel_queues::consumer(router)`. It carries the
  `queue/v2beta` trigger for `nx-jobs`. For each delivery it claims the row,
  **runs the job to completion inside the request** (not behind `WaitUntil`),
  records the outcome, then acknowledges.
- **Retries = send again with a delay.** When an attempt fails and attempts
  remain, the consumer sends a new message with `Vqs-Delay-Seconds` set to the
  job's back-off, then acknowledges the one it holds. That gives the exact
  `backoff_secs · 2^(n-1)` schedule for any back-off up to 7 days (longer is
  clamped), with no sweep. The row's `attempts` is the attempt number; the
  queue's delivery count is not used for it.
- **A crashed run = queue redelivery.** If the instance dies mid-run nothing
  is acknowledged, so the queue delivers the message again after its
  visibility timeout. The row is then still `running`; the consumer reclaims
  it (stale by the job's own timeout) and runs it again, or marks it `dead`
  when attempts are spent. So the row's `max_attempts` bounds a message that
  keeps killing its instance; `maxDeliveries` on the trigger is only a
  ceiling for a message that is never acknowledged at all (see "As built").
- **Acknowledge explicitly.** `DELETE …/lease/{ce-vqsreceipthandle}` after the
  outcome is recorded. A 2xx response does not acknowledge (finding 6), so
  this call is the only thing that ends a delivery. It is idempotent (204
  twice).
- **No secret, no cron.** The consumer function is not reachable from outside,
  so `NEXTRS_JOBS_SECRET` is not required for this path, and nothing needs
  `/__nx/jobs/sweep`. Pruning of old rows moves to "now and then, from the
  consumer" (for example once per cold start), since the sweep did it.
- **Elsewhere nothing changes.** Local dev keeps the self-POST and the
  in-process sweeper; non-Vercel hosts keep the self-POST and the sweep route.
  On Vercel without a consumer function (an app that has not adopted
  `api/jobs.rs` yet) the current behaviour stays, so upgrading does not break
  a deployed app. `NEXTRS_JOBS_DELIVERY=http|vercel-queues` overrides the
  choice.

A rowless mode (payload in the message, no database) was first left out of
this change, then added the same day as a fallback: see "Rowless fallback"
under "As built".

## As built (2026-11-24)

The design above, with these specifics:

- **Mode selection** is `select_delivery(override, on_vercel, consumer_present)`
  in `crates/nextrs/src/jobs.rs`. "A consumer exists" means
  `NEXTRS_JOBS_CONSUMER=vercel-queues` in the function's environment, or the
  process is the consumer itself.
- **How the main function knows.** `nextrs deploy` writes that variable into
  the `environment` of every function's `.vc-config.json` when the output has
  the consumer. It is set from what was actually built, not from the source
  tree, so a consumer that failed to build cannot switch delivery over.
  `nextrs deploy` fails when `api/jobs.rs` exists and no function was built
  for it.
- **The trigger** is written by `nextrs generate` into `.nextrs/vercel.json`
  (`functions["api/jobs.rs"].experimentalTriggers`) when `api/jobs.rs`
  exists. In a server-bundle deploy there is no `vercel build`, so nextrs
  compiles the `jobs` binary itself (the default bundle's selection and
  features) into `functions/api/jobs.func` and writes the trigger, with the
  consumer group `vercel build` would have derived (`api_Sjobs_Drs`).
- **`maxDeliveries` is 32, `retryAfterSeconds` 60.** The consumer bounds a
  crash loop itself: each redelivery of a stale run is a reclaim, which
  spends an attempt, and the row goes `dead` at `max_attempts`. So the cap
  only stops a message that is never acknowledged (a job missing from the
  build). It is set above what a long job uses while its message is
  redelivered to a consumer that leaves the live run alone (once at 5
  minutes, then every 60 s), and finding 6 shows the cap is not exact.
- **A redelivery of a live run is not acknowledged.** If the row is `running`
  and inside its timeout (+60 s), the consumer does nothing. The message
  stays as that run's crash insurance and is dropped on a later redelivery,
  once the row is finished.
- **The consumer reaches the job through the app router.** `consumer(router)`
  rewrites the delivery to `POST /__nx/jobs/<name>` with the delivery in a
  request extension, so the job gets the app's `Extension<T>` layers. Only
  code in the process can set that extension, which is why the route needs
  no secret for it.
- **The OIDC token** is taken from the newest request the process has seen
  (a router layer, and the consumer), because background work outlives the
  request that carried it.
- **`JobStore::reclaim_one(id, cutoff)`** is new, with a default that falls
  back to `reclaim_stale`. The memory and libsql stores implement it.
- **Pruning** runs in the consumer once per instance.
- **The sweep still works in queue mode** (it sends due rows to the queue).
  Nothing needs it, but it is a backstop for a row whose send failed.

### Rowless fallback

Asked for after the first version: jobs should also work with no database.

- `select_record(store_available, delivery)`: rows whenever a store resolves;
  rowless when none does and delivery is the queue; still an error for HTTP
  delivery with no store on Vercel.
- The message is then `{ id, name, payload, attempt, max_attempts }`. The
  consumer runs it with no store (`run_attempt(None, …)`), and on a retryable
  failure sends the same job as attempt `n + 1` with the back-off as
  `Vqs-Delay-Seconds`, then acknowledges.
- A failed first send is an error from the enqueue call
  (`EnqueueError::Store`), not `delivered: false`: nothing else holds the job.
- `JobHandle::status()` returns `Ok(None)`.
- There is no claim, so the only double-run guard is an in-process set of
  running `id#attempt` keys. A redelivery to another instance while a job
  runs past the 5-minute visibility timeout starts a second copy.
- A message that carries a payload is always handled rowless, even if a
  store was configured after it was sent.

## Verified on a preview (2026-11-24, react-todos, Hobby, pdx1)

Two preview deployments of `examples/react-todos` built by `nextrs deploy
--preview` (server-bundle path), `dpl_TQwRaWtzsnc9LSGzqikSWTxPUXZt` and
`dpl_4WoxvQtbswVLE8BgLxmWifrkodSx`. The project has no database configured,
in any environment, so a real job row could not be shared between the two
functions. Both deployments therefore carried a temporary, uncommitted
in-memory store; the second one's consumer stood in for the missing row with
a queued `audit-todo` row whose title starts with `flaky:`.

Seen, from the deployments' runtime logs:

- The deploy with a nextrs-written trigger (no `vercel build`) is accepted,
  and `api/jobs` answers 404 from outside.
- The `NEXTRS_JOBS_CONSUMER` function environment reaches the main function:
  `POST /api/todos` logged `audit job enqueued … delivered=true` with no
  `NEXTRS_JOBS_SECRET` on the project, which only the queue path allows.
- The send from Rust works with the request's OIDC token and
  `Vqs-Deployment-Id`.
- The consumer got the delivery about 0.2 s later and parsed it.
- First deployment: the row was not in the consumer's memory, so the message
  was dropped and acknowledged. That was its only delivery in the following
  8 minutes; the spike's unacknowledged consumers were redelivered at 5.
- Second deployment: attempt 1 ran inside the delivery and failed
  (22:18:23.98), the consumer sent the retry with `Vqs-Delay-Seconds: 2` and
  acknowledged, attempt 2 was delivered 2.4 s later (22:18:26.38) as a new
  message and succeeded (`open_todos=2`), acknowledged.

A third preview, `dpl_Bxv1oseADvs9cphuY4RVcVkoUTR4`, was built from the
branch as committed, with the rowless fallback and no temporary code. With no
database on the project, two todos were created:

- `plain rowless check`: enqueued at 22:48:58.60, the real `audit-todo` body
  ran in the consumer at 22:48:58.82 with the real payload (`id: 5`) and
  succeeded; acknowledged.
- `flaky: rowless check`: attempt 1 ran at 22:48:57.96 and failed, the
  consumer sent attempt 2 with a 2 s delay and acknowledged, attempt 2 ran at
  22:49:00.33 and succeeded; acknowledged.

Not verified on Vercel:

- A job row in a real database, written by the main function and claimed by
  the consumer (the row-backed path end to end). The libsql store is covered
  by its own tests, and the consumer logic by unit tests against a fake queue
  endpoint.
- Reclaim after an instance dies mid-run (unit-tested only).
- The `vercel build` deploy path, where vercel-rust builds `api/jobs.rs`.
  The spike showed `vercel build` copies the trigger; the nextrs step that
  marks the functions afterwards is unit-tested only.
- Production.

## Implementation Notes

- The consumer must not sit behind the app's auth middleware; as a separate
  function it does not.
- The main function is reached through a catch-all rewrite. A trigger on it
  would deliver to its own path (`/api/index`), not to an app route, so a
  dedicated consumer function (or a router branch on `ce-type`) is needed.
- Region matters: messages do not cross regions; use the function's region.
- Message limits: retention 60 s – 7 days (default 24 h), delay up to 7 days,
  visibility timeout up to 60 min. Hobby includes the first 1,000,000 queue
  operations.

## Validation

- Unit tests (`cargo test -p nextrs --features jobs`): delivery parsing from
  `ce-*` headers, delay/back-off mapping, the reclaim-on-redelivery decision,
  delivery-mode selection, and the consumer against a fake queue endpoint
  (run to completion, delayed retry message, dead, unsent retry left for
  redelivery, stale run reclaimed, live run left alone).
- `cargo test -p cargo-nextrs`: the generated trigger, the consumer group
  name, and marking a build output.
- Preview: see "Verified on a preview" above.
- Still to do with a database on a Vercel project: enqueue `audit-todo` from
  a request and see the row reach `succeeded` in `/__nx/admin/jobs`, written
  by the consumer. A retry is a new message, so `ce-vqsdeliverycount` stays 1
  and the row's `attempts` is what counts up.

## Open

- react-todos has no database on Vercel, so its job runs rowless there and
  its admin pages have nothing to show until one is configured.
- Production behaviour of `Vqs-Deployment-Id`: a message sent by one
  production deployment while a newer one is promoted — which deployment's
  consumer receives it?
- The trigger type is `v2beta` and the feature is beta; pin and re-verify on
  upgrade.
