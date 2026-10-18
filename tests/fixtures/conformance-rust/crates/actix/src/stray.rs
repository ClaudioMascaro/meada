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

/// Its name is only bound in this crate (a `let`, a parameter, a closure's parameter, a field)
/// and written in another crate: nothing registers it, so it is served at its own path.
#[get("/bound")]
pub async fn bound() {}

pub struct Bounds {
    bound: u8,
}

pub fn binding(bound: u8) -> u8 {
    let raise = |bound: u8| bound + 1;
    let Bounds { bound: _ } = Bounds { bound };
    let bound = raise(bound);
    bound
}

/// Written before a `let` of its name: the mention counts, so it may be registered where Meada
/// cannot tell, and it is served nowhere.
#[get("/early")]
pub async fn early() {}

pub fn early_then_bound(app: actix_web::App<()>, make: fn() -> u8) {
    app.service(early);
    let early = make();
    let _ = early;
}
