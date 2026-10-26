//! What the JVM ecosystems and contract kinds share, so the Java pack and a Kotlin pack read
//! alike (spec #26, user story 28): the literal Maven and Gradle build readers (`maven`,
//! `gradle`), which the Maven package ecosystem (`package/maven.rs`) reads manifests with; the
//! Spring configuration reader and its `@Value` injector (`spring`), which the HTTP and channel
//! kinds' JVM forms (`http/java*`, `channel/java.rs`) read bases and names through; and the
//! questions those forms ask of a name a library outside the repository may bind (`names`).
//! The ecosystem's rules stay in `package/maven.rs` and the library forms in the kinds. The
//! readers read only the supplied bytes, never a build tool or another file.

use serde::{Deserialize, Serialize};

pub mod gradle;
pub mod maven;
pub mod names;
pub mod spring;

/// Cross-file Gradle metadata retained in a manifest summary, never the build source text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Build {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Project name and relative directory, as a settings file lists them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<(String, String)>,
}

/// The source roots every JVM module has, and the test ones among them: Java's and Kotlin's.
pub const ROOTS: [&str; 4] = [
    "src/main/java",
    "src/main/kotlin",
    "src/test/java",
    "src/test/kotlin",
];
pub const TEST_ROOTS: [&str; 2] = ["src/test/java", "src/test/kotlin"];

/// A group/artifact component of a literal coordinate.
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

/// The literal fragments of an unevaluated Maven/Gradle template, holes written `*`.
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

/// Matches only literal fragments around holes; no properties or expressions are evaluated.
pub fn fits(pattern: &str, value: &str) -> bool {
    let mut pieces = pattern.split('*');
    let first = pieces.next().unwrap_or("");
    let Some(mut rest) = value.strip_prefix(first) else {
        return false;
    };
    let pieces: Vec<_> = pieces.collect();
    if pieces.is_empty() {
        return rest.is_empty();
    }
    for (i, piece) in pieces.iter().enumerate() {
        if i + 1 == pieces.len() {
            return rest.ends_with(piece);
        }
        let Some(at) = rest.find(piece) else {
            return false;
        };
        rest = &rest[at + piece.len()..];
    }
    false
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

    #[test]
    fn fits_matches_only_the_literal_fragments() {
        assert!(fits("example:model*", "example:models"));
        assert!(fits("example:model*", "example:model"));
        assert!(fits("*:tools", "example:tools"));
        assert!(fits("a*b*c", "aXbYc"));
        assert!(fits("example:models", "example:models"));
        assert!(!fits("example:models", "example:models2"));
        assert!(!fits("*:tools", "example:toolsx"));
        assert!(!fits("ab*b", "ab"));
        assert!(!fits("a*b*c", "acb"));
        assert!(!fits("x*ab*b", "xab"));
    }
}
