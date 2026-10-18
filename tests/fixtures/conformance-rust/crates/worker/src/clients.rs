use reqwest::blocking::Client as Blocking;
use reqwest::{Client, Method};
use std::time::Duration;

pub async fn verbs() -> Result<(), reqwest::Error> {
    let client = Client::new();
    client.get("https://api.example.com/verbs/get").send().await?;
    client.post("https://api.example.com/verbs/post").send().await?;
    client.put("https://api.example.com/verbs/put").send().await?;
    client.patch("https://api.example.com/verbs/patch").send().await?;
    client.delete("https://api.example.com/verbs/delete").send().await?;
    client.head("https://api.example.com/verbs/head").send().await?;
    Ok(())
}

pub async fn built() -> Result<(), reqwest::Error> {
    let client = Client::builder().timeout(Duration::from_secs(5)).build()?;
    client.get("https://api.example.com/built").send().await?;
    Ok(())
}

pub async fn requested() -> Result<(), reqwest::Error> {
    let client = Client::new();
    client
        .request(Method::PUT, "https://api.example.com/requested/put")
        .send()
        .await?;
    client
        .request(reqwest::Method::DELETE, "https://api.example.com/requested/delete")
        .send()
        .await?;
    let verb = Method::PATCH;
    client.request(verb, "https://api.example.com/requested/patch").send().await?;
    Ok(())
}

pub async fn inline() -> Result<(), reqwest::Error> {
    Client::new().get("https://api.example.com/inline").send().await?;
    reqwest::Client::new().post("https://api.example.com/inline/path").send().await?;
    Ok(())
}

pub fn blocking() -> Result<(), reqwest::Error> {
    let client = Blocking::new();
    client.get("https://api.example.com/blocking/get").send()?;
    client
        .request(Method::PATCH, "https://api.example.com/blocking/request")
        .send()?;
    reqwest::blocking::Client::builder()
        .build()?
        .delete("https://api.example.com/blocking/built")
        .send()?;
    Ok(())
}
