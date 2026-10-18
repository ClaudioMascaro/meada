mod accounts;
mod admin;
mod orders;
mod stray;

use actix_web::{App, HttpServer, web};

/// The app: handlers registered bare and in a tuple, a scope nested twice, a function that
/// returns a scope, route builders, resources and `.configure`.
pub async fn serve_catalog() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .service(accounts::list)
            .service((accounts::create, accounts::remove))
            .service(web::scope("/v1").service(web::scope("/admin").service(accounts::show)))
            .service(accounts::api())
            .route("/ping", web::get().to(orders::ping))
            .route("/echo", web::route().to(orders::echo))
            .route("/inline", web::put().to(|| async { "ok" }))
            .service(orders::collection())
            .service(orders::item())
            .configure(admin::routes)
            .service(web::scope("/tools").configure(admin::tools))
            // The prefix is read at run time: what is mounted there is served nowhere.
            .service(web::scope(&admin::prefix()).service(admin::unreadable))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
