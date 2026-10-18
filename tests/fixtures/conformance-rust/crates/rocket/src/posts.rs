use std::path::PathBuf;

use rocket::{Route, get, post, routes};

#[get("/")]
pub fn list() {}

#[get("/<id>")]
pub fn show(id: u32) {}

#[post("/", data = "<form>")]
pub fn create(form: String) {}

#[get("/search?<q>&<page>")]
pub fn search(q: String, page: Option<u32>) {}

#[get("/files/<path..>")]
pub fn files(path: PathBuf) {}

#[get("/old/<id>")]
pub fn old(id: u32) {}

pub fn legacy() -> Vec<Route> {
    routes![old]
}
