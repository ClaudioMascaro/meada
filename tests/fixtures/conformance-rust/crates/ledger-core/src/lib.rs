mod meter;

// A module and an item of one name: the item is what a call reaches.
pub use meter::meter;

pub fn post() {}

pub(crate) fn audit() {}

pub mod entries {
    // A `pub use` in a `pub` module written in place is one of its members.
    pub use super::internal::secret;

    pub fn add() {}

    fn seal() {}
}

mod internal {
    pub fn secret() {}
}
