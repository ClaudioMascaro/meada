use actix_web::{App, Scope, get, services, web};

/// Registered by a macro, a `vec!` and a `cfg_if!` Meada does not read: possibly served, under
/// a prefix it cannot tell, so served nowhere.
#[get("/via-services")]
pub async fn via_services() {}

#[get("/via-vec")]
pub async fn via_vec() {}

#[get("/via-cfg-if")]
pub async fn via_cfg_if() {}

pub fn scoped() -> Scope {
    web::scope("/api").service(services![via_services])
}

pub fn listed(app: App<()>) -> App<()> {
    app.service(vec![via_vec])
}

pub fn conditional(app: App<()>) -> App<()> {
    cfg_if::cfg_if! {
        if #[cfg(unix)] {
            app.service(via_cfg_if)
        } else {
            app
        }
    }
}
