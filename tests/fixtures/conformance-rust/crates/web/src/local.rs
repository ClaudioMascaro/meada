// A `Router` of this crate, spelled as Express's: it is no Express app.
pub struct Router(u8);

impl Router {
    pub fn get(&self, _path: &str, _handler: fn()) {}
}

pub fn wire() {
    let router = Router(1);
    router.get("/express-like", wire);
}
