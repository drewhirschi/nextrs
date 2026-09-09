# Durable Object realtime adapter

This Worker is the production-shaped transport for the `/realtime` lab. It
maps each topic to one SQLite-backed Durable Object, accepts hibernatable
WebSockets, persists an ordered sequence, and broadcasts the same frames as
`nextrs::realtime::MemoryRealtime`.

The browser connection is authorized by calling the Rust app once at
`/api/realtime/authorize?topic=...`. In a real household app that route checks
the current session against household membership. Publishers use a separate
secret because they are trusted application servers, not browsers.

## Run locally

Run the nextrs app on port 3000, then in another terminal:

```sh
cp .dev.vars.example .dev.vars
# replace the placeholder with at least 16 characters
npm install
npm run dev
```

Open:

```text
http://localhost:3000/realtime?realtime_origin=http://localhost:8787
```

To broadcast through the Durable Object while that page is open:

```sh
curl -X POST \
  -H 'authorization: Bearer replace-this-with-your-dev-secret' \
  -H 'content-type: application/json' \
  --data '{"operation":"upsert","key":"99","value":{"id":99,"title":"From the Durable Object","done":false}}' \
  http://localhost:8787/__nx/realtime/household.demo.todos
```

That `curl` is a transport-only probe: it does not write the Rust app's
in-memory todo database, so the next authoritative refetch removes that row.
Real mutations must commit to the database first and publish afterward.

Without `realtime_origin`, the page uses the in-process Axum broker and normal
todo mutations broadcast automatically. That is the quickest two-tab demo.

Production should route `/__nx/realtime/*` to this Worker under the app's
domain, set `APP_URL` to the deployed Rust app, store
`REALTIME_PUBLISH_SECRET` with `wrangler secret put`, and publish only after
the database transaction commits.
