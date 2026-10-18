fn pick() -> String {
    std::env::var("API_PATH").unwrap_or_default()
}

/// A URL a function computes at run time: a consumer with no contract.
pub async fn unresolved() {
    let _ = reqwest::get(pick()).await;
}

/// A String a loop grows is unknown, as every `let mut` that is not a plain builder is.
pub async fn looped(parts: &[&str]) {
    let mut url = String::from("https://api.example.com/loop");
    for part in parts {
        url.push_str(part);
    }
    let _ = reqwest::get(url).await;
}

/// A format spec prints something other than the value: a hole.
pub async fn spec(id: u32) {
    let _ = reqwest::get(format!("https://api.example.com/spec/{id:>8}")).await;
}
