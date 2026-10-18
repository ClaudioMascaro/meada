use axum::Router;
use axum::extract::Path;
use axum::routing::{get, post};

use crate::pricing;

pub fn router() -> Router {
    Router::new()
        .route("/orders/:id", get(status))
        .route("/orders/reserve", post(reserve))
}

async fn status(Path(id): Path<String>) {
    let _ = pricing::price_of(&id).await;
}

async fn reserve() {
    let _ = pricing::promotions().await;
}
