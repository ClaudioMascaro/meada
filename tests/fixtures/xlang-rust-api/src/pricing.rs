use std::env;

// Through a base URL variable, with a fallback.
pub async fn price_of(sku: &str) -> reqwest::Result<reqwest::Response> {
    let base = env::var("WEB_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    reqwest::get(format!("{base}/api/prices/{sku}")).await
}

// Through the web app's declared host.
pub async fn promotions() -> reqwest::Result<reqwest::Response> {
    reqwest::get("https://web.example.com/api/promotions").await
}
