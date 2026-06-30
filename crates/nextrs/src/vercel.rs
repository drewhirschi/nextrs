//! Vercel runtime integration for nextrs streaming.
//!
//! `vercel_runtime`'s default [`VercelLayer`] only treats responses as
//! streaming when `content-type` is `text/event-stream` or `application/json`
//! (see `vercel_runtime::axum::StreamingUtils::is_streaming_response`). The
//! framework streams `text/html`, so the upstream layer would silently buffer
//! the entire response — symptoms include TTFB ≈ total response time and the
//! loading shell arriving simultaneously with the page content.
//!
//! [`StreamingVercelLayer`] does the same request-side conversion as the
//! upstream layer but unconditionally calls `create_stream_body` for the
//! response, so HTML streaming flows through Vercel's Fluid compute correctly.
//!
//! [`VercelLayer`]: vercel_runtime::axum::VercelLayer
//!
//! # Usage
//!
//! ```ignore
//! use nextrs::vercel::StreamingVercelLayer;
//! use tower::ServiceBuilder;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), vercel_runtime::Error> {
//!     let router = nextrs::router::build_router(registry);
//!     let app = ServiceBuilder::new()
//!         .layer(StreamingVercelLayer)
//!         .service(router);
//!     vercel_runtime::run(app).await
//! }
//! ```
//!
//! Drop-in replacement for `vercel_runtime::axum::VercelLayer` when you want
//! HTML streaming. Non-streaming responses are unaffected — they just arrive
//! as a single frame, identical to the upstream behavior.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body as AxumBody;
use axum::http::Request as AxumRequest;
use axum::response::Response as AxumResponse;
use http_body_util::BodyExt;
use tower::{Layer, Service, ServiceExt};
use vercel_runtime::axum::StreamingUtils;
use vercel_runtime::{AppState, Error as VercelError, Request as VercelRequest, ResponseBody};

/// A [`tower::Layer`] that wraps an axum router for `vercel_runtime::run` while
/// preserving HTML response streaming.
///
/// See the [module docs](self) for the full story.
#[derive(Clone, Default)]
pub struct StreamingVercelLayer;

impl StreamingVercelLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for StreamingVercelLayer {
    type Service = StreamingVercelService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        StreamingVercelService { inner }
    }
}

/// The [`tower::Service`] produced by [`StreamingVercelLayer`].
///
/// Mirrors `vercel_runtime::axum::VercelService`'s request-side conversion
/// (collect Vercel body bytes → `axum::Body`, call inner axum service) but
/// always wraps the response body via `StreamingUtils::create_stream_body`,
/// bypassing the content-type gate that would otherwise buffer non-SSE
/// non-JSON responses.
#[derive(Clone)]
pub struct StreamingVercelService<S> {
    inner: S,
}

impl<S> Service<(AppState, VercelRequest)> for StreamingVercelService<S>
where
    S: Service<AxumRequest<AxumBody>, Response = AxumResponse<AxumBody>, Error = Infallible>
        + Send
        + Clone
        + 'static,
    S::Future: Send + 'static,
{
    type Response = hyper::Response<ResponseBody>;
    type Error = VercelError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(_)) => Poll::Ready(Err("Inner service error".into())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, (_state, req): (AppState, VercelRequest)) -> Self::Future {
        let mut service = self.inner.clone();
        Box::pin(async move {
            let (parts, body) = req.into_parts();
            let body_bytes = BodyExt::collect(body)
                .await
                .map_err(|e| Box::new(e) as VercelError)?
                .to_bytes();
            let axum_req = AxumRequest::from_parts(parts, AxumBody::from(body_bytes));

            let ready = ServiceExt::ready(&mut service)
                .await
                .map_err(|_| "Service not ready")?;
            let axum_resp = Service::call(ready, axum_req)
                .await
                .map_err(|_| "Service error")?;

            let (resp_parts, resp_body) = axum_resp.into_parts();
            let stream_body = StreamingUtils::create_stream_body(resp_body).await?;
            Ok(hyper::Response::from_parts(resp_parts, stream_body))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::routing::get;

    /// Type-level smoke test: a layered axum router has the right shape to
    /// hand to `vercel_runtime::run`. If `StreamingVercelLayer`'s service
    /// signature ever drifts from what `vercel_runtime::run` expects, this
    /// fails to compile.
    #[test]
    fn layer_composes_with_axum_router() {
        let _: StreamingVercelService<Router> =
            StreamingVercelLayer::new().layer(Router::new().route("/", get(|| async { "ok" })));
    }
}
