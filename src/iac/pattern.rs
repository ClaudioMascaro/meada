//! Bounded matching of unresolved IaC names against literal names (ADR 0026).

// ponytail: below three literal characters a pattern gives too little evidence of a name.
const PATTERN_LITERAL_MIN: usize = 3;
// ponytail: a wildcard replaces whole segments, never part of a word.
const PATTERN_SEGMENTS: &str = "-_./";
// ponytail: cloud resource names are short; bound the backtracking on hostile input.
const PATTERN_TEXT_MAX: usize = 1024;

pub(crate) fn supported(pattern: &str) -> bool {
    if pattern.len() > PATTERN_TEXT_MAX
        || pattern.chars().filter(|&c| c != '*').count() < PATTERN_LITERAL_MIN
    {
        return false;
    }
    let p = pattern.as_bytes();
    !p.iter().enumerate().any(|(i, &c)| {
        c == b'*'
            && (i > 0 && !PATTERN_SEGMENTS.as_bytes().contains(&p[i - 1])
                || i + 1 < p.len() && !PATTERN_SEGMENTS.as_bytes().contains(&p[i + 1]))
    })
}

pub(crate) fn matches(pattern: &str, name: &str) -> bool {
    if !supported(pattern) || name.len() > PATTERN_TEXT_MAX || name.contains('*') {
        return false;
    }
    let p = pattern.as_bytes();
    let n = name.as_bytes();
    let (mut i, mut j, mut star) = (0, 0, None);
    while j < n.len() {
        if p.get(i) == Some(&b'*') {
            star = Some((i, j));
            i += 1;
        } else if p.get(i) == n.get(j) {
            i += 1;
            j += 1;
        } else if let Some((at, end)) = star.as_mut() {
            *end += 1;
            j = *end;
            i = *at + 1;
        } else {
            return false;
        }
    }
    while p.get(i) == Some(&b'*') {
        i += 1;
    }
    i == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_require_literal_evidence_and_whole_segments() {
        for (pattern, name, expected) in [
            ("ab-*", "ab-prod", true),
            ("a-*", "a-prod", false),
            ("orders*", "ordersprod", false),
            ("ord*rs", "orders", false),
            ("*-orders", "prod-orders", true),
            ("orders-*-dlq", "orders-prod-dlq", true),
            ("orders-*-dlq", "orders-dlq", false),
            ("orders_*", "orders_prod", true),
            ("orders.*", "orders.prod", true),
            ("orders/*", "orders/prod", true),
            ("orders-*", "orders-*", false),
        ] {
            assert_eq!(matches(pattern, name), expected, "{pattern} on {name}");
        }
    }

    #[test]
    fn a_pattern_or_name_past_the_text_ceiling_matches_nothing() {
        assert!(!matches(
            &format!("{}-*", "a".repeat(PATTERN_TEXT_MAX)),
            "abc-prod"
        ));
        assert!(!matches(
            "abc-*",
            &format!("abc-{}", "x".repeat(PATTERN_TEXT_MAX))
        ));
    }
}
