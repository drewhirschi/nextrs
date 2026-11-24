// @generated nextrs Vercel Queues consumer for background jobs.
//
// DO NOT PUT APPLICATION LOGIC HERE. This is a second Vercel function next to
// api/index.rs. Vercel calls it once per message on the `nx-jobs` queue topic
// (the `queue/v2beta` trigger `nextrs generate` writes for this file); it
// claims the job row, runs the job from jobs/ to completion inside that
// request, records the outcome, and acknowledges the message. It is not
// reachable from outside the deployment.
//
// It builds the same app as api/index.rs so jobs get the same `Extension<T>`
// state. Delete this file and its `jobs` Cargo target to go back to
// self-POST delivery with a cron-driven sweep.

use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(std::io::stdout),
        )
        // Saves each job attempt's lines to the log store.
        .with(nextrs::logs::layer())
        .init();

    let consumer = nextrs::jobs::vercel_queues::consumer(react_todos::app());
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(consumer);

    vercel_runtime::run(app).await
}
