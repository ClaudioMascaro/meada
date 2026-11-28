use serde::{Deserialize, Serialize};

pub mod gradle;
pub mod maven;
pub mod names;
pub mod spring;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Build {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub catalogs: Vec<(String, String)>,
}

pub const ROOTS: [&str; 4] = [
    "src/main/java",
    "src/main/kotlin",
    "src/test/java",
    "src/test/kotlin",
];
pub const TEST_ROOTS: [&str; 2] = ["src/test/java", "src/test/kotlin"];

pub fn literal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= crate::package::MAX_NAME
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn coordinate(group: &str, artifact: &str) -> Option<String> {
    let name = format!("{group}:{artifact}");
    (literal(group) && literal(artifact) && name.len() <= crate::package::MAX_NAME).then_some(name)
}

pub fn pattern(value: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'{') {
                chars.next();
                let mut depth = 1usize;
                for c in chars.by_ref() {
                    match c {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        break;
                    }
                }
                if depth != 0 {
                    return None;
                }
            } else {
                let mut named = false;
                while chars
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    chars.next();
                    named = true;
                }
                if !named {
                    return None;
                }
            }
            if !out.ends_with('*') {
                out.push('*');
            }
        } else if c == '*' || c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else {
            return None;
        }
    }
    (!out.is_empty() && out.len() <= crate::package::MAX_NAME).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_keeps_literal_fragments_around_one_hole_each() {
        assert_eq!(pattern("model${suffix}").as_deref(), Some("model*"));
        assert_eq!(pattern("${a}${b}-core").as_deref(), Some("*-core"));
        assert_eq!(pattern("$name").as_deref(), Some("*"));
        assert_eq!(pattern("${a{b}}x").as_deref(), Some("*x"));
        assert_eq!(pattern("${open"), None);
        assert_eq!(pattern("$"), None);
        assert_eq!(pattern("a/b"), None);
        assert_eq!(pattern(""), None);
        assert_eq!(pattern(&"a".repeat(crate::package::MAX_NAME + 1)), None);
    }
}
