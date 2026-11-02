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

// In an `impl` or a trait, the body of an item macro is the associated items of the block.
pub struct Runtime;
impl Runtime {
    cfg_rt! {
        pub fn start(&self) { spawn(); }
        pub const SLOTS: usize = 4;
    }
    // Behind `net`, a macro that wraps and an invocation under a disabled `cfg`: out.
    cfg_net! { pub fn bind(&self) {} }
    wrapped! { pub fn unread_method(&self) {} }
    #[cfg(windows)]
    cfg_rt! { pub fn win_method(&self) {} }
    pub fn plain(&self) {}
}
pub trait Driver {
    cfg_rt! {
        fn drive(&self) { spawn(); }
        fn pending(&self);
    }
}

// Written `crate::m!`, it is the macro `m!`; any other path may be another crate's: out.
crate::cfg_rt! { pub fn via_crate() { spawn(); } }
other::cfg_rt! { pub fn via_other() {} }
self::cfg_rt! { pub fn via_self() {} }

pub fn enter() {
    rt::block_on();
    runtime::start();
}
