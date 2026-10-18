use rocket::routes;

use crate::stray::local;

fn rocket() {}

/// A module of the name of the crate hides it, whatever function the name is also given to:
/// this `build` is no Rocket instance.
mod rocket {
    pub struct Builder;

    impl Builder {
        pub fn mount(self, _prefix: &str, _routes: Vec<u8>) -> Builder {
            self
        }
    }

    pub fn build() -> Builder {
        Builder
    }
}

pub fn serve() {
    rocket::build().mount("/hidden", routes![local]);
}
