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
