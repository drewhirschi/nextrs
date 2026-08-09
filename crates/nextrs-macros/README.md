# nextrs-macros

Proc-macros for [nextrs](https://crates.io/crates/nextrs). You normally depend on `nextrs` (which re-exports these), not on this crate directly.

## `#[nextrs::api]`

A thin wrapper over `#[utoipa::path]` that derives the OpenAPI `path` from the handler's file location, so a typed `route.rs` handler never restates the URL the file convention already encodes:

```rust,ignore
// in app/api/ping/route.rs — no `path = "/api/ping"`
#[nextrs::api]
pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> { /* … */ }
```

The method is derived from the function name, request and response bodies from
`Json<T>`, and `operation_id` and `tag` from the route. Each can still be
overridden for a richer contract. For eligible `GET` handlers the macro also
emits a typed seed companion used by server-side React Query cache seeding
(`prefetch.rs`).

## License

Apache-2.0
