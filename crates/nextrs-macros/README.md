# nextrs-macros

Proc-macros for [nextrs](https://crates.io/crates/nextrs). You normally depend on `nextrs` (which re-exports these), not on this crate directly.

## `#[nextrs::api]`

A thin wrapper over `#[utoipa::path]` that derives the OpenAPI `path` from the handler's file location, so a typed `route.rs` handler never restates the URL the file convention already encodes:

```rust,ignore
// in app/api/ping/route.rs — no `path = "/api/ping"`
#[nextrs::api(post, responses((status = 200, body = PingResponse)))]
pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> { /* … */ }
```

`operation_id` and `tag` are derived from the route when omitted (giving the generated client clean hook names), and left alone when supplied. For eligible `GET` handlers the macro also emits a typed seed companion used by the server-side React Query cache seeding (`prefetch.rs`).

## License

Apache-2.0
