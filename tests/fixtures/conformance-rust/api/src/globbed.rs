// Glob imports: a type a glob brings is the head of a path called, as a function it brings is
// a name called, with the module's own items and named imports before it. A glob of a crate
// outside the repository gives nothing.
use crate::store::Store;

pub struct Def;

impl Def {
    pub fn new() -> Self {
        Def
    }
}

pub fn free() {}

mod inner {
    use super::*;

    fn make() -> Def {
        free();
        Def::new()
    }

    // The parent imports `Store` by name.
    fn open() -> Store {
        Store::new()
    }

    mod deep {
        use super::super::*;

        fn open() -> Store {
            Store::new()
        }
    }
}

mod named {
    use crate::globbed::*;

    fn make() -> Def {
        Def::new()
    }
}

mod own {
    use super::*;

    struct Def;

    impl Def {
        fn new() -> Self {
            Def
        }
    }

    fn make() -> Def {
        Def::new()
    }
}

mod renamed {
    use super::*;
    use crate::store::Store as Def;

    fn make() -> Def {
        Def::new()
    }
}

mod outside {
    use serde::*;

    fn make() {
        Ext::new();
    }
}
