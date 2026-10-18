use reqwest::Client;

/// One level: the path is the caller's.
async fn api_get(path: &str) -> Result<String, reqwest::Error> {
    let client = Client::new();
    client
        .get(format!("https://api.example.com{path}"))
        .send()
        .await?
        .text()
        .await
}

/// Two levels: `api_get` called with a path built from this function's own parameter.
async fn resource(name: &str) -> Result<String, reqwest::Error> {
    api_get(&format!("/resources/{name}")).await
}

pub async fn wrapped() -> Result<(), reqwest::Error> {
    api_get("/wrapped/one").await?;
    resource("tags").await?;
    resource("users").await?;
    Ok(())
}

/// The client is a parameter too: each caller passes one it built.
async fn send(client: &Client, path: &str) -> Result<String, reqwest::Error> {
    client
        .post(format!("https://api.example.com{path}"))
        .send()
        .await?
        .text()
        .await
}

pub async fn through_a_parameter() -> Result<(), reqwest::Error> {
    let client = Client::new();
    send(&client, "/param/one").await?;
    Ok(())
}

/// A client held by a struct is not followed.
pub struct Api {
    client: Client,
}

impl Api {
    pub async fn ping(&self) -> Result<(), reqwest::Error> {
        self.client.get("https://api.example.com/field/ping").send().await?;
        Ok(())
    }
}

/// A public function nobody here calls: its path is a caller's in another crate, so the URL is
/// not known.
pub async fn open_ended(path: &str) -> Result<String, reqwest::Error> {
    reqwest::get(format!("https://api.example.com/open/{path}"))
        .await?
        .text()
        .await
}
