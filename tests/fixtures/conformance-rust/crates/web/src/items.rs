use axum::Router;
use axum::routing::{get, post};

pub fn router() -> Router {
    Router::new()
        .route("/items", get(list).post(create))
        .route(
            "/items/:id",
            get(show).put(update).patch(update).delete(remove),
        )
        .route("/items/{id}/tags", post(create).head(show).options(show))
}

pub async fn list() {}

pub async fn create() {}

pub async fn show() {}

pub async fn update() {}

pub async fn remove() {}
