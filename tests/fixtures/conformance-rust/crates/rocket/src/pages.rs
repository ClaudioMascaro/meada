use rocket::get;

#[get("/")]
pub fn index() -> &'static str {
    "home"
}

#[get("/about")]
pub fn about() -> &'static str {
    "about"
}
