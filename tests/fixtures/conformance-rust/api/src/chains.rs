pub struct Router<S> {
    s: S,
}

impl<S> Router<S> {
    pub fn new() -> Self {
        todo!()
    }
    pub fn route(self, path: &str) -> Self {
        self
    }
}

pub struct Plain;

impl Plain {
    pub fn new() -> Self {
        Plain
    }
    pub fn route(self, path: &str) -> Self {
        self
    }
}

pub struct Other;

impl Other {
    pub fn route(self, path: &str) -> Self {
        self
    }
}

pub async fn make() -> Router<()> {
    todo!()
}

pub fn chained() {
    Router::new().route("/").route("/a");
}

pub fn chained_plain() {
    Plain::new().route("/").route("/a");
}

pub fn bound() {
    let r = Router::new();
    r.route("/");
}

pub fn bound_plain() {
    let r = Plain::new();
    r.route("/");
}

pub async fn awaited() {
    make().await.route("/");
}
