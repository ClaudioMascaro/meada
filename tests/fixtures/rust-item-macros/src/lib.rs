// Item macros (#398): the body of a `macro_rules!` of the repository that passes items through
// is read as items of the module that invokes it.
#[macro_use]
mod macros;

macro_rules! cfg_rt { ($($i:item)*) => { $($i)* } }
cfg_rt! {
    pub fn spawn() {}
    pub mod rt;
}
pub fn caller() { spawn(); }

// Defined in `macros.rs`, behind the default feature `rt`.
cfg_runtime! {
    pub mod runtime;
    #[cfg(windows)]
    pub mod windows;
}

// Behind `net`, no default feature: out.
cfg_net! {
    pub mod net;
}

// A macro that wraps its items is no item macro: its body stays unread.
wrapped! {
    pub fn unread() {}
    pub mod skipped;
}

pub fn enter() {
    rt::block_on();
    runtime::start();
}
