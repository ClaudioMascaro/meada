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

/// Passes its configuration on: what the function it calls registers is served where this one
/// is mounted.
pub fn more(cfg: &mut web::ServiceConfig) {
    extras(cfg);
}

fn extras(cfg: &mut web::ServiceConfig) {
    cfg.route("/extras", web::get().to(extra));
}

async fn extra() {}

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
