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

pub trait Named {
    fn label(&self) -> u32 {
        1
    }
}

pub trait Tagged {
    fn label(&self) -> u32 {
        2
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

impl Named for Store {}

impl Tagged for Store {}

impl From<u32> for Store {
    fn from(n: u32) -> Self {
        Store { n }
    }
}

impl From<u8> for Store {
    fn from(n: u8) -> Self {
        Store { n: n.into() }
    }
}

fn private_helper() {}

pub fn reset() {
    Store::new();
    Persist::describe();
    // Two traits give `label`, two `impl From<_>` blocks give `from`: neither is exact.
    Store::label(&Store { n: 0 });
    Store::from(1u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::Store;

    // `use crate::util::Store` hides the file's own `Store`, which `use super::*` brings.
    fn t() {
        Store::new();
        private_helper();
    }
}
