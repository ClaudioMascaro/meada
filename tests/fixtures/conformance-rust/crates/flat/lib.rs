//! A library whose root is not under `src/` (`[lib] path`): its `#[macro_use] extern crate`
//! still brings Rocket's attributes into every module of the crate.

#[macro_use]
extern crate rocket;

mod pages;

pub fn serve() {
    rocket::build().mount("/flat", routes![pages::page]);
}
