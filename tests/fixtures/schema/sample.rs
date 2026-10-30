//! A sample of the facts the Rust pack stores, for the schema golden.

mod disk;
#[path = "legacy/util.rs"]
pub mod util;

use crate::disk::open;
use std::collections::{self as maps, HashMap as Map};
pub use super::*;

pub struct Store {
    n: u32,
}

pub trait Persist {
    fn describe() -> u32 {
        0
    }
}

impl Store {
    pub fn new(path: &str) -> Self {
        Self::empty((path, 1))
    }

    fn empty((name, size): (&str, u32)) -> Self {
        let run = || disk::read(name);
        run();
        Store { n: size }
    }
}

impl Persist for super::Store {}

mod inner {
    pub fn helper() {
        super::open();
        crate::util::go();
        std::mem::drop(1);
    }
}

pub fn save(store: &Store) {
    let made = Store::new("x");
    store.flush();
    made.flush();
    println!("{}", inner::helper());
}

// An item macro (#398): its definition and the braced invocations are stored facts.
macro_rules! cfg_disk {
    ($($item:item)*) => {
        $( #[cfg(feature = "disk")] $item )*
    }
}

cfg_disk! {
    pub fn sync() {}
}
