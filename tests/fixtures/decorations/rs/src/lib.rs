#[get("/x")]
pub fn fast() {
    slow();
}

pub fn slow() {}
