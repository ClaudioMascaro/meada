use rocket::get;

/// Attributed and never mounted: served at its own path, with no prefix.
#[get("/stray")]
pub fn lonely() {}

/// Mounted under a prefix Meada cannot read: served nowhere.
#[get("/moved")]
pub fn moved() {}

pub fn base() -> String {
    std::env::var("BASE").unwrap_or_default()
}

/// Named by a mount on a module that hides the crate (`hiding.rs`): no Rocket instance.
#[get("/local")]
pub fn local() {}

/// A path of three segments is no attribute of Rocket's.
#[rocket::local::get("/three")]
pub fn three() {}

/// The actix crate's private `bound`, written in this other crate, is no registration of it.
pub fn elsewhere() {
    let _ = catalog::stray::bound;
}
