pub mod invoice;

mod util {
    pub fn round(x: u32) -> u32 {
        x
    }
}

pub fn total() -> u32 {
    invoice::Invoice::new().amount() + invoice::tax()
}
