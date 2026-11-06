//! A bounded, literal PCL condition subset. Unread syntax stays unknown as a whole.
use super::config::{Matcher, Op};
use serde::{Deserialize, Serialize};

// ponytail: nested PCL groups and unary conditions must not recurse with file size.
const ALERT_PCL_DEPTH_MAX: usize = 64;

#[derive(Clone, Serialize, Deserialize)]
pub(super) enum Test {
    Atom(usize),
    Unknown,
    And,
    Or,
}

pub(super) fn parse(source: &str) -> Option<(Vec<Matcher>, Vec<Test>)> {
    let mut parser = Parser {
        source,
        matchers: Vec::new(),
        tests: Vec::new(),
    };
    parser.or(0)?;
    parser.whitespace();
    parser
        .source
        .is_empty()
        .then_some((parser.matchers, parser.tests))
}
struct Parser<'a> {
    source: &'a str,
    matchers: Vec<Matcher>,
    tests: Vec<Test>,
}
impl Parser<'_> {
    fn whitespace(&mut self) {
        self.source = self.source.trim_start();
    }
    fn word(&mut self, word: &str) -> bool {
        self.whitespace();
        if let Some(rest) = self.source.strip_prefix(word)
            && rest
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
        {
            self.source = rest;
            true
        } else {
            false
        }
    }
    fn or(&mut self, depth: usize) -> Option<()> {
        self.and(depth)?;
        while self.word("or") {
            self.and(depth)?;
            self.tests.push(Test::Or);
        }
        Some(())
    }
    fn and(&mut self, depth: usize) -> Option<()> {
        self.atom(depth)?;
        while self.word("and") {
            self.atom(depth)?;
            self.tests.push(Test::And);
        }
        Some(())
    }
    fn atom(&mut self, depth: usize) -> Option<()> {
        if depth > ALERT_PCL_DEPTH_MAX {
            return None;
        }
        self.whitespace();
        if self.word("not") {
            let first_matcher = self.matchers.len();
            let first_test = self.tests.len();
            self.atom(depth + 1)?;
            // Push negation into atoms so exported matchers and their levels describe the
            // condition that was actually true, including negated parenthesized groups.
            for matcher in &mut self.matchers[first_matcher..] {
                matcher.op = matcher.op.negated();
            }
            for test in &mut self.tests[first_test..] {
                *test = match &*test {
                    Test::And => Test::Or,
                    Test::Or => Test::And,
                    other => other.clone(),
                };
            }
            return Some(());
        }
        if let Some(rest) = self.source.strip_prefix('(') {
            self.source = rest;
            self.or(depth + 1)?;
            self.whitespace();
            self.source = self.source.strip_prefix(')')?;
            return Some(());
        }
        let end = self
            .source
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.')))
            .unwrap_or(self.source.len());
        let field = &self.source[..end];
        if !field.starts_with("event.") || field.ends_with('.') {
            return None;
        }
        self.source = &self.source[end..];
        if !self.word("matches") {
            return None;
        }
        let op = if self.word("regex") {
            Op::Regex
        } else if self.word("part") {
            Op::Contains
        } else {
            Op::Equal
        };
        self.whitespace();
        let rest = self.source.strip_prefix('\'')?;
        let mut value = String::new();
        let mut escaped = false;
        let mut ended = false;
        for (i, ch) in rest.char_indices() {
            if escaped {
                if !matches!(ch, '\'' | '\\') {
                    value.push('\\');
                }
                value.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '\'' {
                self.source = &rest[i + 1..];
                ended = true;
                break;
            } else {
                value.push(ch);
            }
        }
        if !ended || escaped {
            return None;
        }
        let key = field
            .strip_prefix("event.custom_details.")
            .unwrap_or_else(|| field.strip_prefix("event.").unwrap_or(field));
        if !super::config::literal(&value) {
            return None;
        }
        self.tests.push(Test::Atom(self.matchers.len()));
        self.matchers.push(Matcher {
            key: key.into(),
            field: field.into(),
            value,
            op,
            tags: false,
            tag_value: String::new(),
        });
        Some(())
    }
}

pub(super) fn evaluate(tests: &[Test], values: &[Option<bool>]) -> Option<bool> {
    let mut stack = Vec::new();
    for test in tests {
        match *test {
            Test::Atom(i) => stack.push(*values.get(i)?),
            Test::Unknown => stack.push(None),
            Test::And | Test::Or => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(match (left, right, test) {
                    (Some(false), _, Test::And) | (_, Some(false), Test::And) => Some(false),
                    (Some(true), Some(true), Test::And) => Some(true),
                    (Some(true), _, Test::Or) | (_, Some(true), Test::Or) => Some(true),
                    (Some(false), Some(false), Test::Or) => Some(false),
                    _ => None,
                });
            }
        }
    }
    (stack.len() == 1).then(|| stack[0]).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn condition_groups_past_the_depth_ceiling_are_unread() {
        let expression = "event.custom_details.support matches 'team-b'";
        assert!(
            parse(&format!(
                "{}{}{}",
                "(".repeat(ALERT_PCL_DEPTH_MAX),
                expression,
                ")".repeat(ALERT_PCL_DEPTH_MAX)
            ))
            .is_some()
        );
        assert!(
            parse(&format!(
                "{}{}{}",
                "(".repeat(ALERT_PCL_DEPTH_MAX + 1),
                expression,
                ")".repeat(ALERT_PCL_DEPTH_MAX + 1)
            ))
            .is_none()
        );
    }
}
