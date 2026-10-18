use actix_web::{HttpResponse, Scope, get, post, route, web};

#[get("/accounts")]
pub async fn list() -> HttpResponse {
    HttpResponse::Ok().finish()
}

#[get("/accounts/{id}")]
pub async fn show(_id: web::Path<u32>) -> HttpResponse {
    HttpResponse::Ok().finish()
}

#[post("/accounts")]
pub async fn create() -> HttpResponse {
    HttpResponse::Created().finish()
}

#[actix_web::delete("/accounts/{id}")]
pub async fn remove() -> HttpResponse {
    HttpResponse::NoContent().finish()
}

#[route("/accounts/{id}/sync", method = "PUT", method = "PATCH")]
pub async fn sync() -> HttpResponse {
    HttpResponse::Accepted().finish()
}

#[get("/files/{tail:.*}")]
pub async fn files() -> HttpResponse {
    HttpResponse::Ok().finish()
}

pub fn api() -> Scope {
    web::scope("/api").service(sync).service(files)
}
