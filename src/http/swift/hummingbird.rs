//! Hummingbird providers (spec #29, #120): `get`, `post`, `put`, `patch`, `delete` and
//! `on(path, method:)` called on a `Router` or on a `group(prefix)` of it, at any depth. The
//! router value flow, the mounts and the handler rules are Vapor's (`vapor.rs`); only the root
//! (`Router(..)`), the child builder (`group`, whose path is one optional string) and the order of
//! `on`'s arguments differ. There is no `RouteCollection`: controllers add routes through
//! `addRoutes`, which is not read.

use super::vapor::Dialect;

pub(super) const HUMMINGBIRD: Dialect = Dialect {
    roots: &["Hummingbird.Router"],
    group: "group",
    method_first: false,
    collections: false,
    middleware: false,
};
