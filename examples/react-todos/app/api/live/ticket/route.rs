//! Realtime tickets: the app decides who may watch a topic, then hands out a
//! short-lived signed URL (`nextrs::realtime::ticket`). The browser's
//! `useLiveTopic` asks here before every connect, so the transport — the
//! in-process hub locally, the Cloudflare relay on Vercel — never needs the
//! app's cookies or users.

use axum::Json;
use axum::extract::Query;
use nextrs::ApiError;
use nextrs::realtime::Ticket;
use serde::{Deserialize, Serialize};
use utoipa::IntoParams;

#[derive(Serialize, Deserialize, IntoParams)]
pub struct TicketQuery {
    pub topic: String,
}

#[nextrs::api]
pub async fn get(Query(q): Query<TicketQuery>) -> Result<Json<Ticket>, ApiError> {
    // A real app checks the session here ("may this user see household 42?").
    // The demo has one public list.
    if q.topic != react_todos::live::TODOS_TOPIC {
        return Err(ApiError::forbidden("no access to that topic"));
    }
    nextrs::realtime::ticket(&q.topic).map(Json).map_err(|e| {
        tracing::error!(error = %e, "realtime ticket failed");
        ApiError::internal("realtime is not configured")
    })
}
