use crate::settings;
use crate::settings::{API, USERS};

const LOCAL: &str = "https://api.example.com/local";
static LOCAL_BASE: &str = "https://api.example.com/base";

pub async fn consts() {
    // A const of the file, one imported through `use`, one through a path, a `concat!` const.
    let _ = reqwest::get(LOCAL).await;
    let _ = reqwest::get(USERS).await;
    let _ = reqwest::get(settings::API).await;
    let _ = reqwest::get(settings::TAGS).await;
    let _ = reqwest::get(format!("{LOCAL_BASE}/items")).await;
}

pub async fn formats() {
    let base = "https://api.example.com";
    let _ = reqwest::get(format!("{base}/fmt/inline")).await;
    let _ = reqwest::get(format!("{}/fmt/positional/{}", base, 1)).await;
    let _ = reqwest::get(format!("{0}/fmt/index", base)).await;
    let _ = reqwest::get(format!("{b}/fmt/named", b = base)).await;
    let _ = reqwest::get(format!("{API}/fmt/escaped/{{x}}")).await;
    let _ = reqwest::get(concat!("https://api.example.com", "/concat/", 7)).await;
}

pub async fn strings() {
    // `push_str`, `push` and `+=` on a String built in the function, and `+`.
    let mut url = String::from(API);
    url.push_str("/strings/push");
    url.push('/');
    url += "tail";
    let _ = reqwest::get(url).await;
    let sum = API.to_string() + "/strings/sum" + "/chain";
    let _ = reqwest::get(sum).await;
    let mut early = String::from("https://api.example.com/early");
    let first = reqwest::get(early.clone()).await;
    early.push_str("/late");
    let _ = (first, reqwest::get(early).await);
}
