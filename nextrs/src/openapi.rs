//! Serving the OpenAPI document that the build-time codegen produces.
//!
//! The codegen (`nextrs::build`) emits a `generated_openapi()` function next to
//! `generated_registry()`. It collects every `route.rs` method annotated with
//! `#[utoipa::path(...)]` into a single [`utoipa::openapi::OpenApi`]. This
//! module turns that document into something the app can serve so external
//! tooling (an OpenAPI-driven TypeScript/React Query client, Swagger UI, etc.)
//! can consume it.
//!
//! Wire it into your app by merging the spec router:
//!
//! ```ignore
//! let app = nextrs::router::build_router(generated_registry())
//!     .merge(nextrs::openapi::spec_router(generated_openapi()));
//! ```

use axum::{Json, Router, routing::get};
use utoipa::openapi::OpenApi;

/// Default path the spec is served at.
pub const SPEC_PATH: &str = "/openapi.json";

/// Clean up a freshly-built document for serving/export.
///
/// utoipa's `#[derive(OpenApi)]` defaults `info.license` to an empty
/// `{ "name": "" }`, which fails OpenAPI 3.1 schema validation (a license
/// object requires a non-empty name plus an `identifier` or `url`). That makes
/// strict consumers — orval, Swagger UI — complain. We drop the license when
/// it's effectively empty. The codegen-emitted `generated_openapi()` calls
/// this, so the spec is clean wherever it's used.
pub fn normalize(doc: &mut OpenApi) {
    if let Some(license) = &doc.info.license {
        if license.name.is_empty() {
            doc.info.license = None;
        }
    }
}

/// A [`Router`] that serves the given OpenAPI document as JSON at
/// [`SPEC_PATH`] (`/openapi.json`).
///
/// `.merge()` this into the framework router. The document is cloned into the
/// handler once at startup, so serving it is just a serialization.
pub fn spec_router(spec: OpenApi) -> Router {
    spec_router_at(SPEC_PATH, spec)
}

/// Like [`spec_router`] but lets you choose the path the spec is served at.
pub fn spec_router_at(path: &str, spec: OpenApi) -> Router {
    Router::new().route(
        path,
        get(move || {
            let spec = spec.clone();
            async move { Json(spec) }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use utoipa::openapi::OpenApiBuilder;

    #[tokio::test]
    async fn serves_spec_as_json() {
        let spec = OpenApiBuilder::new().build();
        let app = spec_router(spec);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        // Valid JSON with the OpenAPI marker key.
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("openapi").is_some(), "missing openapi version key");
    }
}
