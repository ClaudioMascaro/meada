pub mod admin;
mod files;
mod health;
pub mod items;
mod prefix;

use axum::Router;
use axum::routing::{any, get};

use crate::prefix::API;

/// The service: routers nested and merged, one router under two prefixes, a router built in
/// another module, and a router a function receives and returns.
pub fn app() -> Router {
    let items = items::router();
    let api = Router::new()
        .nest("/v1", items.clone())
        .nest("/v2", items)
        .route("/ping", get(|| async { "pong" }));
    let public = Router::new()
        .route("/health", get(health::check))
        .route("/echo", any(health::echo))
        .merge(files::router());
    Router::new()
        .nest(API, api)
        .merge(public)
        .nest("/admin", admin::router())
}

pub fn unread(prefix: &str) -> Router {
    // The prefix is a parameter: what is mounted there is served under no prefix.
    Router::new().nest(prefix, orphans::router())
}

mod orphans {
    use axum::Router;
    use axum::routing::get;

    pub fn router() -> Router {
        Router::new().route("/never", get(super::health::check))
    }
}
