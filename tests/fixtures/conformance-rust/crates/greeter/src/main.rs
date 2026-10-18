//! Rocket's canonical form: `#[macro_use] extern crate rocket;` brings its attributes and
//! macros into every module of the crate, with no `use`.

#[macro_use]
extern crate rocket;

mod names;

#[get("/")]
fn index() -> &'static str {
    "hello"
}

#[launch]
fn rocket() -> _ {
    rocket::build().mount("/hello", routes![index, names::named])
}
