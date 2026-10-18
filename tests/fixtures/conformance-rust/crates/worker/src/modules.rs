use reqwest::blocking;
use reqwest::Client;
use std::collections::HashMap;

pub async fn health() {
    let _ = reqwest::get("https://api.example.com/health").await;
}

pub fn blocking_health() {
    let _ = blocking::get("https://api.example.com/blocking/health");
    let _ = reqwest::blocking::get("https://api.example.com/blocking/ping");
}

pub async fn items() {
    let client = Client::new();
    let _ = client.get("https://api.example.com/items").send().await;
}

struct Cache;

impl Cache {
    fn get(&self, key: &str) -> Option<String> {
        Some(key.to_string())
    }
}

/// `get` on anything but a client is no consumer.
pub fn lookalikes(map: &HashMap<String, String>) {
    let cache = Cache;
    let _ = cache.get("https://api.example.com/cache");
    let _ = map.get("https://api.example.com/map");
}
