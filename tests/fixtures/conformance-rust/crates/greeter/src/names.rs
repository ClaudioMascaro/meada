#[get("/<name>")]
pub fn named(name: &str) -> String {
    format!("hello {name}")
}
