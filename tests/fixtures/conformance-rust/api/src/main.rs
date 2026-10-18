mod routes;
mod store;
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
}

fn compute() -> u32 {
    2
}

fn start() {}

fn run() {}
