//! Calls that are not an environment read, though they are spelled like one.

mod env {
    pub fn var(name: &str) -> String {
        format!("https://api.example.com/{name}")
    }
}

/// An `env` the file defines.
pub async fn own_module() {
    let base = env::var("OWN_URL");
    let _ = reqwest::get(base + "/own").await;
}

mod other {
    /// `env::var` with no `use std::env` in scope.
    pub async fn bare() {
        let base = env::var("BARE_URL").unwrap();
        let _ = reqwest::get(base + "/bare").await;
    }
}
