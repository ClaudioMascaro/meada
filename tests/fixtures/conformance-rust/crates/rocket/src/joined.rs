use rocket::get;

/// Mounted through `concat` and a macro of another crate, which Meada does not read: possibly
/// served, under a prefix it cannot tell, so served nowhere.
#[get("/joined-a")]
pub fn joined_a() {}

#[get("/joined-b")]
pub fn joined_b() {}

#[get("/documented")]
pub fn documented() {}
