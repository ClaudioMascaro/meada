use reqwest::Client;

/// A trait the crate defines, with a method named after nothing in `reqwest`.
pub trait ApiExt {
    fn api(&self) -> Client;
}

impl ApiExt for Client {
    fn api(&self) -> Client {
        self.clone()
    }
}

/// Only the first `get` is a consumer: what a client's methods return is no client.
pub async fn derived() -> Result<(), reqwest::Error> {
    let client = Client::new();
    let resp = client.get("https://api.example.com/derived/real").send().await?;
    let _ = resp.headers().get("https://api.example.com/derived/header");
    let value: serde_json::Value = resp.json().await?;
    let _ = value.get("https://api.example.com/derived/json");
    let _ = client.api().get("https://api.example.com/derived/ext");
    Ok(())
}

struct Factory;

impl Factory {
    fn make(&self) -> Client {
        Client::new()
    }

    fn fetch_named(&self, path: &str) -> String {
        format!("https://api.example.com/named{path}")
    }
}

/// Wrapper reached only by a method matched by name.
async fn send_named(path: &str) -> Result<(), reqwest::Error> {
    reqwest::get(format!("https://api.example.com/by-name/{path}")).await?;
    Ok(())
}

struct Sender;

impl Sender {
    async fn send_named(&self, path: &str) -> Result<(), reqwest::Error> {
        let _ = path;
        Ok(())
    }
}

/// `factory` and `sender` have no written type: `make` and `send_named` are found by name, and a
/// name match is no proof of the client or of the caller.
pub async fn by_name(all: &[Factory], senders: &[Sender]) -> Result<(), reqwest::Error> {
    let factory = all[0];
    let client = factory.make();
    client.get("https://api.example.com/by-name/client").send().await?;
    let sender = senders[0];
    sender.send_named("called").await?;
    Ok(())
}

/// A name that a loop or a closure binds again is not the constant around it.
pub async fn shadowed(bases: &[&str]) {
    let base = "https://api.example.com/shadow";
    for base in bases {
        let _ = reqwest::get(format!("{base}/loop")).await;
    }
    let make = |base: &str| reqwest::get(format!("{base}/closure"));
    let _ = make("https://api.example.com/arg").await;
    let _ = reqwest::get(format!("{base}/outer")).await;
}
