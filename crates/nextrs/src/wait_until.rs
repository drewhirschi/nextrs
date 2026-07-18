//! Background work that outlives the response — `waitUntil`.
//!
//! Handlers often want to do work after the response is sent: audit logs,
//! analytics, cache writes, webhooks. On a long-lived server `tokio::spawn`
//! is fine, but on Vercel the instance may be frozen or terminated once the
//! invocation ends, so detached tasks silently die. [`WaitUntil`] papers over
//! the difference:
//!
//! - Extracted with no platform adapter present (local dev, Docker,
//!   self-hosted), it schedules via `tokio::spawn`.
//! - Behind [`StreamingVercelLayer`](crate::vercel::StreamingVercelLayer),
//!   the extension is backed by `vercel_runtime`'s `AppState::wait_until`,
//!   which registers the future with the runtime's shutdown-drained awaiter —
//!   the same guarantee as `waitUntil` in Vercel's Node runtime.
//!
//! The same handler code works in both environments:
//!
//! ```ignore
//! use axum::Json;
//! use nextrs::WaitUntil;
//!
//! pub async fn post(wait: WaitUntil, Json(req): Json<AddTodoRequest>) -> Json<Todo> {
//!     let todo = add(req.title).await;
//!     wait.wait_until(async move {
//!         audit_log(&todo).await; // runs after the response is sent
//!     });
//!     Json(todo)
//! }
//! ```
//!
//! Note for seeded GET handlers: adding any extractor beyond `Path`/`Query`
//! opts a GET handler out of the `#[nextrs::api]` seed companion (it still
//! routes normally). `waitUntil` consumers are typically mutation handlers,
//! where this doesn't apply.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Schedules background futures that must keep running after the response is
/// sent. Extract it in any handler; see the [module docs](self) for how the
/// backing scheduler is chosen.
#[derive(Clone, Default)]
pub struct WaitUntil {
    scheduler: Option<Arc<dyn Fn(BoxFuture) + Send + Sync>>,
}

impl WaitUntil {
    /// A `WaitUntil` backed by plain `tokio::spawn`. Suitable when the process
    /// outlives the request (local dev, Docker, self-hosted). This is what the
    /// extractor yields when no platform adapter installed an extension.
    pub fn detached() -> Self {
        Self::default()
    }

    /// A `WaitUntil` backed by a custom scheduler. Platform adapters use this
    /// to route futures into the platform's background-work registry
    /// (`nextrs::vercel` forwards to `vercel_runtime`'s `AppState::wait_until`).
    pub fn from_scheduler(scheduler: impl Fn(BoxFuture) + Send + Sync + 'static) -> Self {
        Self {
            scheduler: Some(Arc::new(scheduler)),
        }
    }

    /// Register `future` to run in the background. It starts making progress
    /// immediately and is not cancelled when the response is sent; on Vercel
    /// it is drained at instance shutdown. Its output is discarded — convey
    /// failures via logging inside the future.
    pub fn wait_until<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        match &self.scheduler {
            Some(scheduler) => scheduler(Box::pin(future)),
            None => {
                tokio::spawn(future);
            }
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for WaitUntil {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        Ok(parts
            .extensions
            .get::<WaitUntil>()
            .cloned()
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use tower::ServiceExt;

    #[tokio::test]
    async fn extractor_falls_back_to_spawn_without_extension() {
        let (tx, rx) = tokio::sync::oneshot::channel::<&'static str>();
        let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
        let app = Router::new().route(
            "/",
            post(move |wait: WaitUntil| {
                let tx = tx.clone();
                async move {
                    wait.wait_until(async move {
                        if let Some(tx) = tx.lock().unwrap().take() {
                            let _ = tx.send("ran");
                        }
                    });
                    "ok"
                }
            }),
        );

        let res = app
            .oneshot(Request::post("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(res.status().is_success());
        assert_eq!(rx.await.unwrap(), "ran");
    }

    #[tokio::test]
    async fn custom_scheduler_receives_the_future() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let wait = WaitUntil::from_scheduler(move |fut| {
            let _ = tx.send(());
            tokio::spawn(fut);
        });
        wait.wait_until(async {});
        rx.recv().await.expect("scheduler was not invoked");
    }
}
