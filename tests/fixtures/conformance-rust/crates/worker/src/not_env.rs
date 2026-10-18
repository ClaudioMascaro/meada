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

mod hidden {
    use std::env::var;

    /// A parameter named like the import hides it.
    pub async fn parameter(var: impl Fn(&str) -> String) {
        let _ = reqwest::get(var("PARAM_URL") + "/parameter").await;
    }

    /// So does a closure bound to the name.
    pub async fn closure() {
        let var = |key: &str| format!("https://api.example.com/{key}");
        let _ = reqwest::get(var("CLOSURE_URL") + "/closure").await;
    }
}

mod foreign {
    use crate::cfg::var;
    use my_macros::{env, format};

    /// A `var` and an `env!` imported from anywhere but `std` are not std's, nor is a `format!`.
    pub async fn read() {
        let _ = reqwest::get(var("FOREIGN_URL") + "/foreign").await;
        let _ = reqwest::get(concat!(env!("FOREIGN_MACRO"), "/macro")).await;
        let _ = reqwest::get(format!("https://api.example.com/{}", "text")).await;
    }
}
