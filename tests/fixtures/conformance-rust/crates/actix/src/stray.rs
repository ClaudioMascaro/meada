use actix_web::get;

/// Attributed and never registered: served at its own path, with no prefix.
#[get("/stray/{id}")]
pub async fn lonely() {}

/// Registered on a receiver Meada cannot read: served nowhere.
#[get("/held")]
pub async fn held() {}

pub struct Holder {
    app: actix_web::App<()>,
}

pub fn plug(holder: &mut Holder) {
    holder.app.service(held);
}

/// The name is a local that hides the function: nothing registers it.
#[get("/shadowed")]
pub async fn shadowed() {}

pub fn rebind(app: actix_web::App<()>, make: fn() -> u8) {
    let shadowed = make();
    app.service(shadowed);
}

/// Another crate's `get` is no route of actix-web.
mod elsewhere {
    pub fn get(_path: &str) {}
}

#[elsewhere::get("/not-a-route")]
pub async fn plain() {}
