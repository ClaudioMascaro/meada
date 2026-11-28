type Segment = (u8, u64, String);

#[derive(Debug)]
struct Version {
    epoch: u64,
    release: Vec<u64>,
    pre: Option<(u8, u64)>,
    post: Option<u64>,
    dev: Option<u64>,
    local: Option<Vec<Segment>>,
}

type Key = (
    u64,
    Vec<u64>,
    (u8, u8, u64),
    Option<u64>,
    (u8, u64),
    Option<Vec<Segment>>,
);

fn number(s: &mut &str) -> Option<u64> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let n = s[..end].parse().ok()?;
    *s = &s[end..];
    Some(n)
}

fn tagged<'w>(s: &mut &str, words: &[&'w str]) -> Option<(&'w str, u64)> {
    let sep = ['-', '_', '.'];
    let rest = s.strip_prefix(sep).unwrap_or(s);
    let word = *words.iter().find(|w| rest.starts_with(**w))?;
    let mut rest = &rest[word.len()..];
    let mut after = rest.strip_prefix(sep).unwrap_or(rest);
    let n = number(&mut after).inspect(|_| rest = after);
    *s = rest;
    Some((word, n.unwrap_or(0)))
}

impl Version {
    fn parse(text: &str) -> Option<Version> {
        let lower = text.trim().to_ascii_lowercase();
        let mut s = lower.strip_prefix('v').unwrap_or(&lower);
        let mut epoch = 0;
        if let Some((e, rest)) = s.split_once('!') {
            epoch = e.parse().ok()?;
            s = rest;
        }
        let mut release = vec![number(&mut s)?];
        while let Some(rest) = s.strip_prefix('.') {
            let mut after = rest;
            match number(&mut after) {
                Some(n) => release.push(n),
                None => break,
            }
            s = after;
        }
        let pre = {
            let mut t = s;
            let words = ["alpha", "beta", "preview", "pre", "rc", "a", "b", "c"];
            tagged(&mut t, &words).map(|(w, n)| {
                s = t;
                let letter = match w {
                    "alpha" | "a" => 0,
                    "beta" | "b" => 1,
                    _ => 2,
                };
                (letter, n)
            })
        };
        let post = {
            let mut t = s;
            let implicit = t.strip_prefix('-').and_then(|mut r| {
                let n = number(&mut r)?;
                t = r;
                Some(n)
            });
            match implicit {
                Some(n) => Some(n),
                None => tagged(&mut t, &["post", "rev", "r"]).map(|(_, n)| n),
            }
            .inspect(|_| s = t)
        };
        let dev = {
            let mut t = s;
            tagged(&mut t, &["dev"]).map(|(_, n)| {
                s = t;
                n
            })
        };
        let local = match s.strip_prefix('+') {
            Some(label) => {
                s = "";
                let valid = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric());
                let parts: Vec<&str> = label.split(['-', '_', '.']).collect();
                if !parts.iter().all(|p| valid(p)) {
                    return None;
                }
                let segment = |p: &&str| match p.chars().all(|c| c.is_ascii_digit()) {
                    true => p.parse().ok().map(|n| (1, n, String::new())),
                    false => Some((0, 0, p.to_string())),
                };
                Some(parts.iter().map(segment).collect::<Option<_>>()?)
            }
            None => None,
        };
        s.is_empty().then_some(Version {
            epoch,
            release,
            pre,
            post,
            dev,
            local,
        })
    }

    fn normalized(&self) -> String {
        let join = |parts: Vec<String>, sep: &str| parts.join(sep);
        let mut out = String::new();
        if self.epoch != 0 {
            out += &format!("{}!", self.epoch);
        }
        out += &join(self.release.iter().map(u64::to_string).collect(), ".");
        if let Some((kind, n)) = self.pre {
            out += &format!("{}{n}", ["a", "b", "rc"][usize::from(kind.min(2))]);
        }
        if let Some(n) = self.post {
            out += &format!(".post{n}");
        }
        if let Some(n) = self.dev {
            out += &format!(".dev{n}");
        }
        if let Some(local) = &self.local {
            let segment = |(_, n, text): &Segment| {
                if text.is_empty() {
                    n.to_string()
                } else {
                    text.clone()
                }
            };
            out += &format!("+{}", join(local.iter().map(segment).collect(), "."));
        }
        out
    }

    fn key(&self, local: bool) -> Key {
        let mut release = self.release.clone();
        while release.len() > 1 && release.last() == Some(&0) {
            release.pop();
        }
        let pre = match (self.pre, self.post, self.dev) {
            (None, None, Some(_)) => (0, 0, 0),
            (None, ..) => (2, 0, 0),
            (Some((letter, n)), ..) => (1, letter, n),
        };
        let dev = self.dev.map_or((1, 0), |n| (0, n));
        let local = self.local.clone().filter(|_| local);
        (self.epoch, release, pre, self.post, dev, local)
    }

    fn is_pre(&self) -> bool {
        self.pre.is_some() || self.dev.is_some()
    }

    fn same_base(&self, other: &Version) -> bool {
        (self.epoch, self.key(false).1) == (other.epoch, other.key(false).1)
    }

    fn starts_with(&self, epoch: u64, release: &[u64]) -> bool {
        let at = |i: usize| self.release.get(i).copied().unwrap_or(0);
        self.epoch == epoch && release.iter().enumerate().all(|(i, n)| at(i) == *n)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Compatible,
}

enum Clause {
    Arbitrary(String),
    Compare(Op, Version, bool),
}

impl Clause {
    fn parse(text: &str) -> Option<Clause> {
        let text = text.trim();
        let ops = [
            ("===", None),
            ("==", Some(Op::Eq)),
            ("!=", Some(Op::Ne)),
            ("<=", Some(Op::Le)),
            (">=", Some(Op::Ge)),
            ("~=", Some(Op::Compatible)),
            ("<", Some(Op::Lt)),
            (">", Some(Op::Gt)),
        ];
        let (word, op) = ops.iter().find(|(w, _)| text.starts_with(w))?;
        let rest = text[word.len()..].trim();
        let Some(op) = *op else {
            return (!rest.is_empty()).then(|| Clause::Arbitrary(rest.to_ascii_lowercase()));
        };
        let (rest, wildcard) = match rest.strip_suffix(".*") {
            Some(head) => (head, true),
            None => (rest, false),
        };
        let version = Version::parse(rest)?;
        let plain = version.pre.is_none() && version.post.is_none() && version.dev.is_none();
        let fits = match op {
            Op::Eq | Op::Ne => !wildcard || (plain && version.local.is_none()),
            Op::Compatible => !wildcard && version.release.len() > 1 && version.local.is_none(),
            _ => !wildcard && version.local.is_none(),
        };
        fits.then_some(Clause::Compare(op, version, wildcard))
    }

    fn names_pre(&self) -> bool {
        match self {
            Clause::Arbitrary(text) => Version::parse(text).is_some_and(|v| v.is_pre()),
            Clause::Compare(op, v, _) => v.is_pre() && !matches!(op, Op::Ne | Op::Lt | Op::Gt),
        }
    }

    fn holds(&self, v: &Version) -> bool {
        let Clause::Compare(op, s, wildcard) = self else {
            return matches!(self, Clause::Arbitrary(want) if v.normalized() == *want);
        };
        let public = v.key(false);
        match op {
            Op::Eq | Op::Ne => {
                let equal = match (wildcard, &s.local) {
                    (true, _) => v.starts_with(s.epoch, &s.release),
                    (false, Some(_)) => v.key(true) == s.key(true),
                    (false, None) => public == s.key(false),
                };
                equal == (*op == Op::Eq)
            }
            Op::Le => public <= s.key(false),
            Op::Ge => public >= s.key(false),
            Op::Lt => v.key(true) < s.key(true) && !(!s.is_pre() && v.is_pre() && v.same_base(s)),
            Op::Gt => {
                v.key(true) > s.key(true)
                    && !(s.post.is_none() && v.post.is_some() && v.same_base(s))
                    && !(v.local.is_some() && v.same_base(s))
            }
            Op::Compatible => {
                let head = &s.release[..s.release.len() - 1];
                public >= s.key(false) && v.starts_with(s.epoch, head)
            }
        }
    }
}

pub fn accepts(range: &str, version: &str) -> Option<bool> {
    let v = Version::parse(version)?;
    let clauses: Vec<Clause> = range.split(',').map(Clause::parse).collect::<Option<_>>()?;
    if v.is_pre() && !clauses.iter().any(Clause::names_pre) {
        return Some(false);
    }
    Some(clauses.iter().all(|c| c.holds(&v)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(range: &str, version: &str) -> Option<bool> {
        accepts(range, version)
    }

    #[test]
    fn versions_are_ordered_as_pep_440_orders_them() {
        let key = |s: &str| Version::parse(s).unwrap().key(true);
        let sorted = [
            "1.0.dev1",
            "1.0a1.dev1",
            "1.0a1",
            "1.0a2",
            "1.0b1",
            "1.0rc1",
            "1.0",
            "1.0+abc",
            "1.0+abc.5",
            "1.0+5",
            "1.0.post1.dev1",
            "1.0.post1",
            "1.1.dev1",
            "1.1",
            "2!0.1",
        ];
        for pair in sorted.windows(2) {
            assert!(key(pair[0]) < key(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        for same in [
            ("1.0", "1.0.0"),
            ("1.0a1", "1.0-alpha.1"),
            ("1.0.post1", "1.0-1"),
        ] {
            assert_eq!(key(same.0), key(same.1), "{} = {}", same.0, same.1);
        }
        assert_eq!(key("1.0rc1"), key("V1.0C1"));
        assert_eq!(key("1.0.post"), key("1.0-r0"));
        for bad in [
            "",
            "1.",
            "1..0",
            "a1",
            "1.0.x",
            "1.0+",
            "1.0+a..b",
            "1!",
            "99999999999999999999",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn each_operator_is_checked_against_the_version() {
        assert_eq!(at("==24.4.2", "0.1.0"), Some(false));
        assert_eq!(at("==24.4.2", "24.4.2"), Some(true));
        assert_eq!(at("==1.0", "1.0.0"), Some(true));
        assert_eq!(at("!=1.0", "1.0"), Some(false));
        assert_eq!(at("!=1.0", "1.1"), Some(true));
        assert_eq!(at("<2", "1.9"), Some(true));
        assert_eq!(at("<2", "2.0"), Some(false));
        assert_eq!(at("<=2", "2.0.0"), Some(true));
        assert_eq!(at("<=2", "2.1"), Some(false));
        assert_eq!(at(">1", "1.1"), Some(true));
        assert_eq!(at(">1", "1.0"), Some(false));
        assert_eq!(at(">=1", "1"), Some(true));
        assert_eq!(at(">=1", "0.9"), Some(false));
        assert_eq!(at(">=0.1,<1", "0.5"), Some(true));
        assert_eq!(at(">= 0.1 , < 1", "1.0"), Some(false));
    }

    #[test]
    fn a_compatible_release_and_a_wildcard_fit_a_prefix() {
        assert_eq!(at("~=2.2", "2.9"), Some(true));
        assert_eq!(at("~=2.2", "3.0"), Some(false));
        assert_eq!(at("~=2.2", "2.1"), Some(false));
        assert_eq!(at("~=1.4.5", "1.4.9"), Some(true));
        assert_eq!(at("~=1.4.5", "1.5.0"), Some(false));
        assert_eq!(at("~=1.4.5", "1.4.4"), Some(false));
        assert_eq!(at("==1.4.*", "1.4.0"), Some(true));
        assert_eq!(at("==1.4.*", "1.4.7"), Some(true));
        assert_eq!(at("==1.4.*", "1.5"), Some(false));
        assert_eq!(at("==1.*", "1"), Some(true));
        assert_eq!(at("==1.0.*", "1"), Some(true));
        assert_eq!(at("!=1.4.*", "1.4.2"), Some(false));
        assert_eq!(at("!=1.4.*", "1.5"), Some(true));
        assert_eq!(at("==1!1.*", "1"), Some(false));
    }

    #[test]
    fn a_pre_release_is_accepted_only_when_the_set_names_one() {
        assert_eq!(at(">=1.0", "2.0rc1"), Some(false));
        assert_eq!(at(">=1.0", "2.0.dev1"), Some(false));
        assert_eq!(at(">=2.0rc1", "2.0rc2"), Some(true));
        assert_eq!(at(">=1.0,<3,!=2.0rc1", "2.0rc2"), Some(false));
        assert_eq!(at("==2.0rc1", "2.0rc1"), Some(true));
        assert_eq!(at("<2.0,>=1.0rc1", "2.0rc1"), Some(false));
        assert_eq!(at("<2.0", "1.0"), Some(true));
        assert_eq!(at("<2.0rc3", "2.0rc1"), Some(false));
        assert_eq!(at("<2.0rc3,>=2.0rc0", "2.0rc1"), Some(true));
        assert_eq!(at(">=2.0rc1", "2.0"), Some(true));
    }

    #[test]
    fn a_local_label_is_ignored_unless_the_clause_has_one() {
        assert_eq!(at("==1.0", "1.0+cpu"), Some(true));
        assert_eq!(at("==1.0+cpu", "1.0+cpu"), Some(true));
        assert_eq!(at("==1.0+cpu", "1.0+gpu"), Some(false));
        assert_eq!(at("==1.0+cpu", "1.0"), Some(false));
        assert_eq!(at("!=1.0", "1.0+cpu"), Some(false));
        assert_eq!(at(">=1.0", "1.0+cpu"), Some(true));
        assert_eq!(at("<=1.0", "1.0+cpu"), Some(true));
        assert_eq!(at(">1.0", "1.0+cpu"), Some(false));
        assert_eq!(at(">1.0", "1.0.post1"), Some(false));
        assert_eq!(at(">1.0.post1", "1.0.post2"), Some(true));
        assert_eq!(at(">1.0", "1.1+cpu"), Some(true));
        for clause in [">=1.0+a", "<1.0+a", "~=1.0+a", "==1.0+a.*"] {
            assert_eq!(at(clause, "1.0"), None, "{clause}");
        }
    }

    #[test]
    fn arbitrary_equality_compares_the_text_without_case() {
        assert_eq!(at("===1.0.0", "1.0.0"), Some(true));
        assert_eq!(at("===1.0.0", "1.0"), Some(false));
        assert_eq!(at("===1.0RC1", "1.0rc1"), Some(true));
        assert_eq!(at("===1.0-custom", "1.0.0"), Some(false));
        assert_eq!(at("===1.0rc1", "1.0-RC1"), Some(true));
        assert_eq!(at("===1.0", "v1.0"), Some(true));
        assert_eq!(
            at("===1!2.0b3.post4.dev5+Ab.6", "1!2.0-beta3-4.dev5+ab-6"),
            Some(true)
        );
        assert_eq!(at("===1.0-rc1", "1.0rc1"), Some(false));
        assert_eq!(at("===", "1.0"), None);
    }

    #[test]
    fn what_cannot_be_read_is_not_decided() {
        for range in [
            "",
            ",",
            ">=1,",
            "1.0",
            "^1.0",
            "~1.0",
            "*",
            ">=1 <2",
            ">=1 || <0",
            "~=1",
            "==1.0a1.*",
            "==1.0.post1.*",
            ">=1.*",
            "~=1.0.*",
            "=1.0",
            "<>1",
            ">=x",
            "==1.0.",
            "> =1",
        ] {
            assert_eq!(at(range, "1.0"), None, "{range:?}");
        }
        assert_eq!(at(">=1", ""), None);
        assert_eq!(at(">=1", "latest"), None);
    }
}
