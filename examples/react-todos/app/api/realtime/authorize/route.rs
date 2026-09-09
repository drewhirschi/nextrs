//! Authorization callback used by the Durable Object gateway example.
//!
//! A real app would extract its session and verify membership in the requested
//! household here. Keeping the decision in Rust lets the Worker coordinate
//! sockets without becoming a second auth implementation.

use axum::extract::Query;
use axum::http::StatusCode;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AuthorizeQuery {
    topic: String,
}

pub async fn get(Query(query): Query<AuthorizeQuery>) -> StatusCode {
    if query.topic == "household.demo.todos" {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::FORBIDDEN
    }
}
