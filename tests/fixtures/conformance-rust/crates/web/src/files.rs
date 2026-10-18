use axum::Router;
use axum::routing::get;

async fn read() {}

pub fn router() -> Router {
    Router::new()
        .route("/files/*rest", get(read))
        .route("/blobs/{*rest}", get(read))
}
