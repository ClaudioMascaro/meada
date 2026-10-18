mod disk;

pub struct Store {
    n: u32,
}

pub enum Mode {
    Fast,
    Slow,
}

pub trait Persist {
    fn persist(&self);

    fn describe() -> u32 {
        0
    }
}

impl Store {
    pub fn new() -> Self {
        Self::empty()
    }

    fn empty() -> Self {
        disk::open();
        Store { n: Store::reload() }
    }

    pub fn save(&self) {}
}

impl Persist for Store {
    fn persist(&self) {
        Self::describe();
    }
}

pub fn reset() {
    Store::new();
    Persist::describe();
}
