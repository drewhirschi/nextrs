//! Seed the realtime lab from the same typed list query it turns into a
//! TanStack DB collection. The WebSocket attaches after mount and immediately
//! reconciles this snapshot, closing the seed/subscribe race.

include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs"));

pub async fn prefetch(req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
    nextrs::QuerySeed::new()
        .seed(get_api_todos(
            api_todos::TodosFilter { status: None },
            req.extensions(),
        ))
        .await
}
