mod client;
mod hummingbird;
mod vapor;

use super::{PARAM, normalize_path};
use crate::contract::Found;
use crate::resolve::Code;

pub(super) fn providers(code: &Code) -> Vec<Found> {
    let mut found = vapor::providers(code, &vapor::VAPOR);
    found.extend(vapor::providers(code, &hummingbird::HUMMINGBIRD));
    found
}

pub(super) fn consumers(code: &Code) -> Vec<Found> {
    client::consumers(code)
}

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
