pub mod admin;
mod files;
mod health;
pub mod items;
mod local;
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
        .nest_service("/legacy", legacy::router())
}

pub fn unread(prefix: &str) -> Router {
    // The prefix is a parameter: what is mounted there is served under no prefix.
    Router::new().nest(prefix, orphans::router())
}

mod late {
    use axum::Router;
    use axum::routing::get;

    pub fn router() -> Router {
        Router::new().route("/late", get(super::health::check))
    }
}

pub struct Holder {
    router: Router,
}

impl Holder {
    pub fn mount(&mut self) {
        // The router this is mounted on is a field: Meada cannot read it, so `late` is
        // served nowhere.
        self.router.nest("/held", late::router());
    }
}

mod legacy {
    use axum::Router;
    use axum::routing::get;

    pub fn router() -> Router {
        Router::new().route("/old", get(super::health::check))
    }
}

mod orphans {
    use axum::Router;
    use axum::routing::get;

    pub fn router() -> Router {
        Router::new().route("/never", get(super::health::check))
    }
}
