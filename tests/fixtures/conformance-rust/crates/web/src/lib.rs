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
        .route("/{{raw}}", get(health::check))
        .merge(files::router());
    Router::new()
        .nest(API, api)
        .merge(public)
        .nest("/admin", admin::router())
        .nest_service("/legacy", legacy::router())
        .nest("/alias", renamed::router())
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

mod renamed {
    use axum::Router;
    use axum::routing::{any as all, get as fetch};

    // The method is the one axum names, whatever the file calls it.
    pub fn router() -> Router {
        Router::new()
            .route("/renamed", fetch(super::health::check))
            .route("/renamed-any", all(super::health::echo))
    }
}

/// A router nested through a method only its name finds, or handed to a service, or held by a
/// `let mut` or a name assigned again, is served nowhere.
pub struct Builder;

impl Builder {
    pub fn routes(&self) -> Router {
        Router::new().route("/named-only", get(health::check))
    }
}

pub fn hop(pair: (Builder, u8)) -> Router {
    Router::new().nest("/named", pair.0.routes())
}

mod services {
    use axum::Router;
    use axum::routing::get;

    pub fn router() -> Router {
        Router::new().route("/svc-only", get(super::health::check))
    }

    pub fn fallback() -> Router {
        Router::new().route("/fallback-only", get(super::health::check))
    }

    pub fn wrapped() -> Router {
        Router::new().route("/wrapped-only", get(super::health::check))
    }
}

pub fn wrapper() -> Router {
    // A function of another crate gets the router: what it returns is not read.
    Router::new().nest("/wrapped", std::convert::identity(services::wrapped()))
}

pub fn served() -> Router {
    Router::new()
        .route_service("/svc", services::router())
        .fallback_service(services::fallback())
}

mod reassigned_items {
    use axum::Router;
    use axum::routing::get;

    pub fn router() -> Router {
        Router::new().route("/items-mut", get(super::health::check))
    }
}

pub fn reassigned(legacy: bool) -> Router {
    let mut prefix = "/v2";
    if legacy {
        prefix = "/v1";
    }
    let mut app = Router::new();
    app = app.route("/health-mut", get(health::check));
    Router::new()
        .nest(prefix, reassigned_items::router())
        .merge(app)
}

mod orphans {
    use axum::Router;
    use axum::routing::get;

    pub fn router() -> Router {
        Router::new().route("/never", get(super::health::check))
    }
}
