//! The path rule of the HTTP contract kind, which a `[serve]` rewrite follows too.

/// What every parameter (`[id]`, `[...slug]`, `:id`, a template hole) becomes in a path.
pub const PARAM: &str = "{}";

/// Drops the query string, the fragment and a trailing `/`, and turns every segment holding
/// a parameter into `PARAM`. Case and API versions are kept. Template holes must already be
/// written as `PARAM`.
pub fn normalize_path(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let segments: Vec<&str> = path
        .split('/')
        .map(|s| {
            let param = s.contains(PARAM)
                || s.starts_with(':')
                || (s.starts_with('[') && s.ends_with(']'))
                || (s.contains('{') && s.contains('}'))
                || (s.starts_with('<') && s.ends_with('>'));
            if param { PARAM } else { s }
        })
        .collect();
    let path = segments.join("/");
    let path = path.trim_end_matches('/');
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}
