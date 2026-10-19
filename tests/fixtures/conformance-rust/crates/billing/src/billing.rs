pub mod invoice;

pub use invoice::Invoice;
use ledger_core::entries;

mod util {
    pub fn round(x: u32) -> u32 {
        x
    }
}

pub fn total() -> u32 {
    invoice::Invoice::new().amount() + invoice::tax()
}

pub fn settle() {
    ledger_core::post();
    entries::add();
}

// A name match keeps to the crates this one reaches: `ledger_core`'s `Ledger.clone`, never
// `api`'s `Config.clone`.
pub fn copies(ledgers: Vec<ledger_core::Ledger>) {
    for l in ledgers {
        l.clone();
    }
}
