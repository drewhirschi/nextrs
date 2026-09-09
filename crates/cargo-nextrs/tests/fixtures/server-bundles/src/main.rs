#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap();
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    axum::serve(listener, server_bundles_fixture::app())
        .await
        .unwrap();
}
