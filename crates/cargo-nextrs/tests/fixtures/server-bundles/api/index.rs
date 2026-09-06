#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let service = tower::ServiceBuilder::new()
        .layer(nextrs::vercel::StreamingVercelLayer::new())
        .service(server_bundles_fixture::app());
    vercel_runtime::run(service).await
}
