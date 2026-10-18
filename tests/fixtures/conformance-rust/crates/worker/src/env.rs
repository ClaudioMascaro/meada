//! Base URLs read from the environment: Meada names the variable and never reads its value.
use std::env;

/// `std::env::var` written in full, `unwrap`, and a `+` on the String.
pub async fn full_path() {
    let base = std::env::var("STD_URL").unwrap();
    let _ = reqwest::get(base + "/std").await;
}

/// `env::var` after `use std::env`, a default and a captured name in a format string.
pub async fn imported() {
    let base = env::var("IMPORTED_URL").unwrap_or_default();
    let _ = reqwest::get(format!("{base}/imported")).await;
}

/// `use std::env::var` inside the function.
pub async fn function_import() {
    use std::env::var;
    let base = var("FUNCTION_URL").expect("FUNCTION_URL is set");
    let _ = reqwest::get(format!("{base}/function")).await;
}

/// `env!`, bound to a name and written in a `concat!`.
pub async fn macros() {
    let base = env!("MACRO_URL");
    let _ = reqwest::get(format!("{base}/macro")).await;
    let _ = reqwest::get(concat!(env!("CONCAT_URL"), "/concat")).await;
}

/// `dotenvy::var` with `?`.
pub async fn dotenv() -> Result<(), Box<dyn std::error::Error>> {
    let base = dotenvy::var("DOTENV_URL")?;
    reqwest::get(format!("{base}/dotenv")).await?;
    Ok(())
}

/// A fallback and string methods keep the one read.
pub async fn fallbacks() {
    let a = env::var("OR_URL").unwrap_or("http://localhost:3000".to_string());
    let _ = reqwest::get(format!("{a}/or")).await;
    let b = env::var("ELSE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    let _ = reqwest::get(format!("{b}/else")).await;
    let c = env::var("TRIM_URL").unwrap_or_default().trim_end_matches('/').to_string();
    let _ = reqwest::get(c + "/trim").await;
}

/// A second read in the fallback is no single read: the base is a hole.
pub async fn two_reads() {
    let base = env::var("FIRST_URL").unwrap_or_else(|_| env::var("SECOND_URL").unwrap());
    let _ = reqwest::get(format!("{base}/two")).await;
    let other = env::var("THIRD_URL").unwrap_or(env!("FOURTH_URL").to_string());
    let _ = reqwest::get(other + "/macro-fallback").await;
}

/// `option_env!` is not read.
pub async fn optional() {
    let base = option_env!("OPTIONAL_URL").unwrap_or("https://api.example.com");
    let _ = reqwest::get(format!("{base}/optional")).await;
}
