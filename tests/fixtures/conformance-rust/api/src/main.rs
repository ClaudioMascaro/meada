mod extras;
#[cfg(not(target_has_atomic = "64"))]
mod fallback;
mod gated;
mod imp;
mod launch;
#[cfg(test)]
mod mock;
#[cfg(target_has_atomic = "64")]
mod native;
mod real;
#[cfg(feature = "metrics")]
mod metrics;
mod methods;
mod prelude;
mod ring;
mod routes;
mod store;
#[cfg(windows)]
mod winonly;

use crate::extras::*;
use crate::prelude::*;
use crate::store::{self as stores, Store as Repo};
use billing::{invoice::Invoice as Bill, total};
use inner::helper as aid;
use launch::launch;
use ledger_core::entries as books;
use self::nested::deep::dive;
use std::collections::HashMap;
#[path = "../extra/legacy_util.rs"]
mod util;

mod inner {
    pub fn helper() -> u32 {
        1
    }

    pub fn twice() -> u32 {
        helper() + self::helper()
    }
}

mod checks {
    fn compute() -> u32 {
        3
    }

    // `super::compute` names the file's `compute`, which this module's own hides from a
    // plain name.
    pub fn run_checks() -> u32 {
        super::compute() + compute()
    }

    pub fn verify() {
        super::start();
    }
}

// A `use` inside a module binds, and hides an outer binding of its name, in that module alone.
mod scoped {
    use crate::checks::run_checks as aid;
    use crate::inner::twice;

    pub fn go() -> u32 {
        aid() + twice()
    }
}

// What a `use` of the file binds is not in scope in a module written in place, nor is a
// module of the file: both names are crates outside it there.
mod isolated {
    pub fn go() {
        aid();
    }

    pub fn far() {
        inner::helper();
    }
}

// An item of the module beats the name a glob gives.
mod local_wins {
    use super::*;

    fn compute() -> u32 {
        9
    }

    pub fn go() -> u32 {
        compute()
    }
}

// Imports that name each other give nothing.
mod loops {
    use b::x as a;
    use a::y as b;

    pub fn f() {
        a();
    }
}

// `use super::*` takes the private items of the module around it.
mod below {
    use super::*;

    pub fn go() -> u32 {
        compute()
    }
}

// A `mod` declaration and a `#[path]` inside a module written in place.
mod nested {
    #[path = "deep.rs"]
    pub mod deep;
    pub mod plain;
}

fn main() {
    // A `let` of the module's name does not hide the module a path names.
    let store = store::Store::new();
    routes::serve(&store);
    util::legacy();
    inner::helper();
    crate::store::reset();
    nested::deep::dive();
    nested::plain::walk();
    // Calls inside a macro invocation are not indexed.
    println!("{}", compute());
    let run = || start();
    run();
    String::new();
    std::mem::drop(store);
    // Names a `use` binds: a rename, a group, a `self` in a group, `self::`, a crate name
    // with and without a `use`, a workspace member's hyphen turned into an underscore.
    aid();
    dive();
    Repo::new();
    stores::reset();
    total();
    Bill::new();
    billing::invoice::tax();
    billing::Invoice::new();
    ledger_core::post();
    books::add();
    // Crates outside the repository are external: no name match.
    HashMap::new();
    // A glob takes the public items alone: the prelude's private `dup` is not taken.
    dup();
    greet();
    // A chain of `pub use` renames, and a cycle of them.
    ring::e::Wheel::spin();
    ring::Gear::spin();
    ring::x::Loop::unheard();
    scoped::go();
    below::go();
    isolated::go();
    isolated::far();
    local_wins::go();
    // A module is no value: `launch()` is the function the `use` brings, and the path reaches
    // the module's.
    launch();
    launch::launch();
    // A `pub use` in a `pub` module written in place, and a module and an item of one name.
    ledger_core::entries::secret();
    ledger_core::meter();
}

// A `use` of a module is not in scope outside it.
fn outside() -> u32 {
    twice()
}

// A function's own `use` binds in it alone.
fn within() {
    use crate::inner::helper as local_aid;
    local_aid();
}

fn elsewhere() {
    local_aid();
}

// A parameter hides the `use` that spells its name, as a `let` does.
fn with_param(aid: fn()) {
    aid();
}

// A binding of the name hides the `use` that spells it.
fn hidden() {
    let aid = || 0;
    aid();
}

fn compute() -> u32 {
    2
}

fn start() {}

fn run() {}
