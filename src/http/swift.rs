//! Swift HTTP providers and consumers (spec #29): Vapor's routes in `vapor`, the calls of
//! URLSession, Alamofire and AsyncHTTPClient in `client`.

mod client;
mod vapor;

use super::{PARAM, normalize_path};
use crate::contract::Found;
use crate::resolve::Code;

/// The providers of the Swift files of the repository.
pub(super) fn providers(code: &Code) -> Vec<Found> {
    vapor::providers(code)
}

/// The consumers of the Swift files of the repository.
pub(super) fn consumers(code: &Code) -> Vec<Found> {
    client::consumers(code)
}

/// `path` as the trunk reads it, a `*` or a `**` segment too a parameter (Vapor's `.anything` and
/// `.catchall`), the others as `normalize_path` reads them (`:id`, `{id}`).
fn normalized(path: &str) -> String {
    let segments: Vec<&str> = (path.split('/'))
        .map(|s| if matches!(s, "*" | "**") { PARAM } else { s })
        .collect();
    normalize_path(&segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::normalized;

    #[test]
    fn a_path_parameter_is_the_marker_in_each_form_vapor_writes_it() {
        for (written, normal) in [
            ("/users/:id", "/users/{}"),
            ("/users/{id}", "/users/{}"),
            ("/assets/*", "/assets/{}"),
            ("/assets/**", "/assets/{}"),
            ("/a/*/b/**", "/a/{}/b/{}"),
            ("/a/b*", "/a/b*"),
            ("api/v1/", "/api/v1"),
            ("", "/"),
        ] {
            assert_eq!(normalized(written), normal, "{written}");
        }
    }
}
