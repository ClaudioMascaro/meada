use super::vapor::Dialect;

pub(super) const HUMMINGBIRD: Dialect = Dialect {
    roots: &["Hummingbird.Router"],
    group: "group",
    method_first: false,
    collections: false,
    middleware: false,
};
