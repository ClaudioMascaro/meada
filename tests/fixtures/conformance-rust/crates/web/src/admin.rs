use axum::Router;
use axum::routing::{get, post};

pub fn router() -> Router {
    mount(Router::new())
}

fn mount(base: Router) -> Router {
    base.route("/stats", get(stats)).route("/reset", post(reset))
}

async fn stats() {}

async fn reset() {}
