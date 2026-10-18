mod pages;
mod posts;
mod stray;

use rocket::routes;

/// The instance: handlers listed in `routes!` under a prefix, a function that returns the list
/// and a prefix read at run time.
#[rocket::launch]
fn rocket() -> _ {
    rocket::build()
        .mount("/", routes![pages::index, pages::about])
        .mount(
            "/posts",
            routes![
                posts::list,
                posts::show,
                posts::create,
                posts::search,
                posts::files
            ],
        )
        .mount("/legacy", posts::legacy())
        // The prefix is read at run time: what is mounted there is served nowhere.
        .mount(stray::base(), routes![stray::moved])
}
