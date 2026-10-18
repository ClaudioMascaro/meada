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
    // plain name: it is left unresolved rather than linked to the wrong one.
    pub fn run_checks() -> u32 {
        super::compute() + compute()
    }

    pub fn verify() {
        super::start();
    }
}

fn main() {
    let s = store::Store::new();
    routes::serve(&s);
    util::legacy();
    inner::helper();
    crate::store::reset();
    // Calls inside a macro invocation are not indexed.
    println!("{}", compute());
    let run = || start();
    run();
    String::new();
    std::mem::drop(s);
}

fn compute() -> u32 {
    2
}

fn start() {}

fn run() {}
