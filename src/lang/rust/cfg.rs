//! Conditional compilation (spec #30): whether a `#[cfg(..)]` predicate holds for the one
//! target Meada indexes, `x86_64` Linux, with the default features of the crate. Nothing
//! depends on the machine that runs Meada, and a predicate this module does not know is false.

use std::collections::BTreeSet;

/// How deep `all`, `any` and `not` may nest in one predicate; a deeper one is false.
// ponytail: real predicates nest two or three levels; the reader recurses once per level, so a
// crafted `not(not(not(..` must not reach the stack's end.
pub const PREDICATE_DEPTH: usize = 64;

/// How many features one crate's default set may reach through features that enable others; the
/// rest is not followed. A cycle (`a = ["b"]`, `b = ["a"]`) is cut by the visited set as well.
// ponytail: real crates define dozens of features; a crafted manifest of a million chained ones
// must not be walked whole on every refresh.
pub const FEATURES: usize = 4096;

/// Whether the text after `cfg`, `(unix, not(feature = "x"))` with its parentheses, holds with
/// `features` enabled. A malformed or too deep predicate is false.
pub fn holds(predicate: &str, features: &BTreeSet<String>) -> bool {
    let mut at = Reader {
        tokens: tokens(predicate),
        next: 0,
        features,
    };
    at.open() && at.predicate(0) == Some(true) && at.close() && at.next == at.tokens.len()
}

/// The default features of a manifest: `default`, the names it lists, and the ones those enable
/// in turn. Entries `dep:x` and `x/feat` enable no feature of this crate and are ignored.
pub fn defaults(manifest: &toml::Table) -> BTreeSet<String> {
    let table = manifest.get("features").and_then(|f| f.as_table());
    let mut out = BTreeSet::new();
    let mut queue = vec!["default".to_string()];
    while let Some(name) = queue.pop() {
        if out.len() >= FEATURES || !out.insert(name.clone()) {
            continue;
        }
        let listed = table.and_then(|t| t.get(&name)?.as_array());
        for entry in listed.into_iter().flatten().filter_map(|e| e.as_str()) {
            if !entry.contains(['/', ':']) {
                queue.push(entry.to_string());
            }
        }
    }
    out
}

#[derive(PartialEq)]
enum Token<'s> {
    Open,
    Close,
    Comma,
    Equals,
    Name(&'s str),
    Text(&'s str),
}

/// The tokens of a predicate, none at all when it holds a character it has no use for.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        match c {
            '(' => out.push(Token::Open),
            ')' => out.push(Token::Close),
            ',' => out.push(Token::Comma),
            '=' => out.push(Token::Equals),
            // No escapes: a name with one is none that is known.
            '"' => {
                let Some(end) = rest[1..].find('"') else {
                    return Vec::new();
                };
                out.push(Token::Text(&rest[1..=end]));
                rest = &rest[end + 2..];
                continue;
            }
            c if c.is_alphanumeric() || c == '_' => {
                let end = rest
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                out.push(Token::Name(&rest[..end]));
                rest = &rest[end..];
                continue;
            }
            c if c.is_whitespace() => {}
            _ => return Vec::new(),
        }
        rest = &rest[c.len_utf8()..];
    }
    out
}

struct Reader<'s, 'f> {
    tokens: Vec<Token<'s>>,
    next: usize,
    features: &'f BTreeSet<String>,
}

impl Reader<'_, '_> {
    fn eat(&mut self, token: Token) -> bool {
        let found = self.tokens.get(self.next) == Some(&token);
        self.next += found as usize;
        found
    }

    fn open(&mut self) -> bool {
        self.eat(Token::Open)
    }

    fn close(&mut self) -> bool {
        self.eat(Token::Close)
    }

    /// One predicate, or `None` when it is malformed or nests past the limit. Every operand is
    /// read, so the cursor ends after the whole predicate.
    fn predicate(&mut self, depth: usize) -> Option<bool> {
        let Some(&Token::Name(name)) = self.tokens.get(self.next) else {
            return None;
        };
        self.next += 1;
        if self.eat(Token::Equals) {
            let Some(&Token::Text(value)) = self.tokens.get(self.next) else {
                return None;
            };
            self.next += 1;
            return Some(self.pair(name, value));
        }
        if !self.open() {
            return Some(matches!(name, "unix" | "test" | "debug_assertions"));
        }
        if depth >= PREDICATE_DEPTH {
            return None;
        }
        let (mut all, mut any, mut count) = (true, false, 0);
        while !self.close() {
            let held = self.predicate(depth + 1)?;
            (all, any, count) = (all && held, any || held, count + 1);
            if !self.eat(Token::Comma) && self.tokens.get(self.next) != Some(&Token::Close) {
                return None;
            }
        }
        match name {
            "all" => Some(all),
            "any" => Some(any),
            "not" if count == 1 => Some(!all),
            _ => Some(false),
        }
    }

    fn pair(&self, key: &str, value: &str) -> bool {
        match (key, value) {
            ("feature", name) => self.features.contains(name),
            ("target_os", "linux")
            | ("target_family", "unix")
            | ("target_arch", "x86_64")
            | ("target_pointer_width", "64")
            | ("target_endian", "little")
            | ("target_env", "gnu") => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(predicate: &str) -> bool {
        holds(predicate, &["net".to_string()].into())
    }

    #[test]
    fn the_fixed_target_decides_the_atoms() {
        for yes in [
            "(unix)",
            "(test)",
            "(debug_assertions)",
            "(target_os = \"linux\")",
            "(target_family = \"unix\")",
            "(target_arch = \"x86_64\")",
            "(target_pointer_width = \"64\")",
            "(target_endian = \"little\")",
            "(target_env = \"gnu\")",
            "(feature = \"net\")",
        ] {
            assert!(on(yes), "{yes}");
        }
        for no in [
            "(windows)",
            "(target_os = \"windows\")",
            "(target_arch = \"aarch64\")",
            "(feature = \"tls\")",
            "(panic = \"abort\")",
            "(made_up)",
            "(true)",
        ] {
            assert!(!on(no), "{no}");
        }
    }

    #[test]
    fn all_any_and_not_combine() {
        assert!(on("(all(unix, feature = \"net\"))"));
        assert!(!on("(all(unix, windows))"));
        assert!(on("(any(windows, unix,))"));
        assert!(!on("(any(windows, feature = \"tls\"))"));
        assert!(on("(not(windows))"));
        assert!(!on("(not(test))"));
        assert!(on("(all())"));
        assert!(!on("(any())"));
        assert!(on("(not(any(windows, all(unix, feature = \"tls\"))))"));
    }

    #[test]
    fn a_malformed_predicate_is_false() {
        for bad in [
            "",
            "(",
            "(unix",
            "(unix) x",
            "(not(unix, test))",
            "(unix; test)",
            "(feature = net)",
        ] {
            assert!(!on(bad), "{bad}");
        }
    }

    #[test]
    fn nesting_past_the_limit_is_false() {
        let nest = |n: usize| format!("({}unix{})", "not(".repeat(n), ")".repeat(n));
        assert!(on(&nest(PREDICATE_DEPTH - 2)));
        assert!(!on(&nest(PREDICATE_DEPTH + 1)));
        assert!(!on(&nest(100_000)));
    }

    fn manifest(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    #[test]
    fn default_features_follow_features_that_enable_others() {
        let table = manifest(
            "[features]\ndefault = [\"net\", \"dep:x\", \"y/z\"]\nnet = [\"wire\"]\nwire = []\nmetrics = []\n",
        );
        let found: Vec<String> = defaults(&table).into_iter().collect();
        assert_eq!(found, ["default", "net", "wire"]);
        assert_eq!(defaults(&manifest("[package]\nname = \"p\"\n")).len(), 1);
    }

    #[test]
    fn a_feature_cycle_and_a_long_chain_end() {
        let cycle =
            manifest("[features]\ndefault = [\"a\"]\na = [\"b\"]\nb = [\"a\", \"default\"]\n");
        assert_eq!(defaults(&cycle).len(), 3);
        let mut text = String::from("[features]\ndefault = [\"f0\"]\n");
        for n in 0..FEATURES + 10 {
            text += &format!("f{n} = [\"f{}\"]\n", n + 1);
        }
        assert_eq!(defaults(&manifest(&text)).len(), FEATURES);
    }
}
