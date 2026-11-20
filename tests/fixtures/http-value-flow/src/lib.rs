#![allow(non_snake_case)]
use std::env;

struct Client;
impl Client {
    fn method_base(&self) -> String { env::var("API_URL").unwrap() }
    async fn ViaMethod(&self) {
        let _ = reqwest::get(self.method_base() + "/api/orders").await;
    }
    fn method_prefix(&self) -> String { env::var("API_URL").unwrap() + "/v2" }
    async fn MethodWithPrefix(&self) {
        let _ = reqwest::get(self.method_prefix() + "/orders").await;
    }
}

fn function_base() -> String { env::var("API_URL").unwrap() + "/v2" }
async fn ViaFunction() { let _ = reqwest::get(function_base() + "/orders").await; }
fn plain_base() -> String { env::var("API_URL").unwrap() }
async fn FunctionWithoutPrefix() { let _ = reqwest::get(plain_base() + "/api/orders").await; }

async fn env_get(key: &str, path: &str) {
    let _ = reqwest::get(env::var(key).unwrap() + path).await;
}
async fn ParamA() { env_get("API_URL", "/api/orders").await; }
async fn ParamB() { env_get("SECOND_URL", "/api/orders").await; }
async fn ParamUnknown(key: &str) { env_get(key, "/api/orders").await; }

async fn QueryAppend(flag: bool) {
    let mut path = String::from("/api/orders");
    if flag { path += "?x=1"; }
    let base = env::var("API_URL").unwrap();
    let _ = reqwest::get(format!("{base}{path}")).await;
}

async fn QueryRewrite(flag: bool) {
    let mut path = String::from("/api/orders");
    if flag { path = path + "?x=1"; }
    let base = env::var("API_URL").unwrap();
    let _ = reqwest::get(format!("{base}{path}")).await;
}

async fn QuerySuffix(flag: bool) {
    let mut query = "";
    if flag { query = "?x=1"; }
    let base = env::var("API_URL").unwrap();
    let _ = reqwest::get(format!("{base}/api/orders{query}")).await;
}

async fn QueryConditional(flag: bool) {
    let query = if flag { "?x=1" } else { "" };
    let base = env::var("API_URL").unwrap();
    let _ = reqwest::get(format!("{base}/api/orders{query}")).await;
}
