use actix_web::{get, web};

/// A service configuration passed to `.configure`: its scope and route are served wherever the
/// app mounts it.
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/admin").service(dashboard))
        .route("/status", web::get().to(status));
}

/// Mounted under a scope: the handler is served under its prefix.
pub fn tools(cfg: &mut web::ServiceConfig) {
    cfg.service(audit);
}

pub fn prefix() -> String {
    std::env::var("ADMIN_PREFIX").unwrap_or_default()
}

#[get("/dashboard")]
async fn dashboard() {}

#[get("/audit")]
async fn audit() {}

#[get("/unreadable")]
pub async fn unreadable() {}

async fn status() {}
