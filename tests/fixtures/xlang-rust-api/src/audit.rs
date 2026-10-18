// Through a base URL variable read at build time.
pub async fn audit() -> reqwest::Result<reqwest::Response> {
    let base = env!("WEB_URL");
    reqwest::Client::new()
        .post(format!("{base}/api/audit"))
        .send()
        .await
}
