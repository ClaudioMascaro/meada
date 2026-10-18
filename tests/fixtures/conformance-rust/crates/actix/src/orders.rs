use actix_web::{Resource, web};

pub async fn ping() -> &'static str {
    "pong"
}

pub async fn echo() -> &'static str {
    "echo"
}

async fn list() {}

async fn create() {}

async fn update() {}

async fn show() {}

pub fn collection() -> Resource {
    web::resource("/orders")
        .route(web::get().to(list))
        .route(web::post().to(create))
}

pub fn item() -> Resource {
    web::resource("/orders/{id}")
        .route(web::patch().to(update))
        .route(web::head().to(show))
        .route(web::delete().to(|| async { "gone" }))
}
