pub fn ignored() {}

// `scratch` lists no dependency: `ledger_core` is a crate outside the repository to it.
pub fn reach() {
    ledger_core::post();
}
