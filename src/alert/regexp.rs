//! Alertmanager/Go RE2 compatibility on the bounded Rust regex engine.
use super::{
    ALERT_REGEX_CLASSES_MAX, ALERT_REGEX_FOLD_MAX, ALERT_REGEX_MAX, ALERT_REGEX_POOL_MAX,
    ALERT_REGEX_SIZE_MAX,
};
use regex_syntax::ast::{
    self, Ast, ClassSetItem, Flag, GroupKind, RepetitionKind, RepetitionRange,
};
use regex_syntax::hir::{Class, ClassUnicode, ClassUnicodeRange, HirKind};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

// ponytail: Go/RE2 rejects counted repetitions, including nested products, above 1000.
const RE2_REPEAT_MAX: u32 = 1000;

// The `regexp/syntax` error codes (`ErrorCode`, whose value is this text) that Go's `regexp`
// answers a syntax error with and `refusal` can establish; any other failure is not known to be
// a refusal.
const MISSING_PAREN: &str = "missing closing )";
const UNEXPECTED_PAREN: &str = "unexpected )";
const MISSING_BRACKET: &str = "missing closing ]";
const MISSING_REPEAT: &str = "missing argument to repetition operator";
const CHAR_RANGE: &str = "invalid character class range";
const REPEAT_SIZE: &str = "invalid repeat count";
const REPEAT_OP: &str = "invalid nested repetition operator";
const ESCAPE: &str = "invalid escape sequence";
const PERL_OP: &str = "invalid or unsupported Perl syntax";
const NAMED_CAPTURE: &str = "invalid named capture";

/// `None`, noting in `refusal` that Go's `regexp` refuses the expression with error code `code`.
fn refuse<T>(refusal: &mut Option<&'static str>, code: &'static str) -> Option<T> {
    *refusal = Some(code);
    None
}

/// The `regexp/syntax` error code with which Go's `regexp` certainly refuses `pattern`, which
/// makes Alertmanager refuse the whole configuration holding it. Alertmanager hands Go the text
/// `^(?:pattern)$`, and so does this, so that a `)` of the pattern that closes the wrapper does
/// not stand alone.
///
/// `None` when Go compiles it or when this cannot tell: a pattern past `ALERT_REGEX_MAX` (valid
/// or not), a Unicode property outside the list (the tables follow Go's version), an ambiguous
/// count (`a{01}`), a nesting past the 250 of `regex-syntax` where Go allows 1000, a failure of
/// `regex-syntax` that Go does not share (`(?ii)`, `a{,5}`, `[\d-z]` are valid for Go), and
/// whatever `compile` leaves unmatched for its budgets. A failure is a refusal only if Go's
/// parser refuses what it names, for each class of the error codes above.
pub(super) fn refusal(pattern: &str) -> Option<&'static str> {
    // `regex_source` reads a `[:` more leniently than Go (`[[:]` is no POSIX class for Go) and
    // drops a `\Q\E` that Go counts between two operators, leaving the rest misread.
    if pattern.len() > ALERT_REGEX_MAX || pattern.contains("[:") || pattern.contains("\\Q") {
        return None;
    }
    let mut refusal = None;
    let Some(source) = regex_source(&format!("^(?:{pattern})$"), &mut refusal) else {
        return refusal;
    };
    let error = match ast::parse::Parser::new().parse(&source) {
        Ok(ast) => return re2_ast(&ast).err(),
        Err(error) => error,
    };
    use ast::ErrorKind::*;
    Some(match error.kind() {
        GroupUnclosed => MISSING_PAREN,
        GroupUnopened => UNEXPECTED_PAREN,
        ClassUnclosed => MISSING_BRACKET,
        // Go reads `(?` as a group of flags, which repeats nothing and lets a repetition after
        // it repeat what came before, and `{` as text unless it forms a count: only `*`, `+`
        // and `?` opening a group or an alternative lack an operand for certain.
        RepetitionMissing => {
            let at = error.span().start.offset;
            let before = source.get(..at)?.chars().next_back();
            let operator = source.get(at..)?.chars().next();
            let lacks = match before {
                Some('(') => matches!(operator, Some('*' | '+')),
                Some('|' | ':') => matches!(operator, Some('*' | '+' | '?')),
                _ => false,
            };
            if !lacks {
                return None;
            }
            MISSING_REPEAT
        }
        ClassRangeInvalid => CHAR_RANGE,
        RepetitionCountInvalid => REPEAT_SIZE,
        EscapeUnrecognized | UnsupportedBackreference => ESCAPE,
        FlagDanglingNegation | FlagRepeatedNegation { .. } => PERL_OP,
        _ => return None,
    })
}

/// An expression compiled within its part of the ceilings, and its automaton positions (each
/// character, dot or class and each state that reads none, once per copy its counted repetitions
/// make) by the byte offsets of a value each may start at: matching takes at most a step per
/// position and offset, whichever engine runs, as the lazy DFA gives way to the PikeVM when its
/// states do not fit.
pub(super) struct Compiled {
    pub(super) regex: regex::Regex,
    /// The positions an unbounded repetition, around or before them, lets start at any offset.
    looped: usize,
    /// The offsets each other position may start at, summed: one more than the characters what
    /// precedes it may vary by.
    spread: usize,
}
impl Compiled {
    /// An upper bound on the steps matching a value of `len` bytes takes.
    pub(super) fn cost(&self, len: usize) -> usize {
        (len.saturating_add(1))
            .saturating_mul(self.looped)
            .saturating_add(self.spread)
    }
    /// A position entered at `starts` offsets (`None`: at any).
    fn step(&mut self, starts: Option<usize>) {
        match starts {
            Some(starts) => self.spread = self.spread.saturating_add(starts),
            None => self.looped = self.looped.saturating_add(1),
        }
    }
    /// Counts the positions of `ast`, entered at `starts` offsets (`None`: at any), and returns
    /// the fewest and the most characters it matches (`None`: no most).
    fn add(&mut self, ast: &Ast, starts: Option<usize>) -> (usize, Option<usize>) {
        let widen = |starts: Option<usize>, (fewest, most): (usize, Option<usize>)| {
            Some(starts?.saturating_add(most?.saturating_sub(fewest)))
        };
        match ast {
            Ast::Flags(_) => (0, Some(0)),
            // A state that reads no character is still walked at each offset it may start at.
            Ast::Empty(_) | Ast::Assertion(_) => {
                self.step(starts);
                (0, Some(0))
            }
            Ast::Literal(_)
            | Ast::Dot(_)
            | Ast::ClassUnicode(_)
            | Ast::ClassPerl(_)
            | Ast::ClassBracketed(_) => {
                self.step(starts);
                (1, Some(1))
            }
            Ast::Group(group) => {
                let length = self.add(&group.ast, starts);
                // A capture saves its start and its end, each a state.
                if !matches!(group.kind, GroupKind::NonCapturing(_)) {
                    self.step(starts);
                    self.step(widen(starts, length));
                }
                length
            }
            Ast::Alternation(alternation) => {
                self.step(starts);
                (alternation.asts.iter()).fold((usize::MAX, Some(0)), |(fewest, most), ast| {
                    let (lo, hi) = self.add(ast, starts);
                    (fewest.min(lo), most.zip(hi).map(|(a, b)| a.max(b)))
                })
            }
            Ast::Concat(concat) => {
                let (mut fewest, mut most, mut starts) = (0, Some(0_usize), starts);
                for ast in &concat.asts {
                    let length = self.add(ast, starts);
                    starts = widen(starts, length);
                    fewest = length.0.saturating_add(fewest);
                    most = most.zip(length.1).map(|(a, b)| a.saturating_add(b));
                }
                (fewest, most)
            }
            Ast::Repetition(repetition) => {
                let (min, max) = match &repetition.op.kind {
                    RepetitionKind::ZeroOrOne => (0, Some(1)),
                    RepetitionKind::ZeroOrMore => (0, None),
                    RepetitionKind::OneOrMore => (1, None),
                    RepetitionKind::Range(RepetitionRange::Exactly(n)) => (*n, Some(*n)),
                    RepetitionKind::Range(RepetitionRange::AtLeast(n)) => (*n, None),
                    RepetitionKind::Range(RepetitionRange::Bounded(n, m)) => (*n, Some(*m)),
                };
                let (min, max) = (min as usize, max.map(|m| m as usize));
                // The automaton holds `max` copies, or `min` (at least one) with the last looping;
                // a copy starts where the ones before it may end.
                let copies = max.unwrap_or(min.max(1));
                let (mut length, mut entry) = ((0, Some(0)), starts);
                for copy in 1..=copies {
                    let looping = max.is_none() && copy == copies;
                    let at = entry.filter(|_| !looping);
                    // An optional or looping copy is entered through a choice.
                    if looping || copy > min {
                        self.step(at);
                    }
                    length = self.add(&repetition.ast, at);
                    entry = widen(entry, length);
                }
                let most = max.zip(length.1).map(|(m, hi)| m.saturating_mul(hi));
                (min.saturating_mul(length.0), most)
            }
        }
    }
}

/// The expressions one linking pass compiled, each once for its share, and how many routing
/// scopes share the pass's ceilings in equal parts.
pub(super) struct Pass<'a> {
    scopes: usize,
    compiled: HashMap<(&'a str, usize), Option<Arc<Compiled>>>,
}
impl<'a> Pass<'a> {
    /// A pass over `scopes` routing scopes, each compiling its expressions within an equal part
    /// of the ceilings, so what one scope compiles never depends on the scopes before it.
    pub(super) fn new(scopes: usize) -> Self {
        Self {
            scopes: scopes.max(1),
            compiled: HashMap::new(),
        }
    }
    /// `pattern` as Alertmanager reads it, anchored to the whole value, within its part of
    /// `ALERT_REGEX_POOL_MAX` (at most `ALERT_REGEX_SIZE_MAX` for its scope),
    /// `ALERT_REGEX_FOLD_MAX` and `ALERT_REGEX_CLASSES_MAX`: an equal share of its scope's part,
    /// one of `shares` distinct expressions. `None` past them, past `ALERT_REGEX_MAX` or for no
    /// RE2 expression.
    pub(super) fn compile(&mut self, pattern: &'a str, shares: usize) -> Option<Arc<Compiled>> {
        let scopes = self.scopes;
        let compiled = self.compiled.entry((pattern, shares));
        compiled
            .or_insert_with(|| compile(pattern, scopes, shares).map(Arc::new))
            .clone()
    }
}

/// `Pass::compile` in a pass of `scopes`.
fn compile(pattern: &str, scopes: usize, shares: usize) -> Option<Compiled> {
    if pattern.len() > ALERT_REGEX_MAX {
        return None;
    }
    let pattern = regex_source(pattern, &mut None)?;
    let ast = ast::parse::Parser::new().parse(&pattern).ok()?;
    re2_ast(&ast).ok()?;
    let shares = shares.max(1);
    let parts = scopes.saturating_mul(shares);
    let (insensitive, classes) = shape(&ast);
    if classes > ALERT_REGEX_CLASSES_MAX / parts {
        return None;
    }
    if insensitive {
        folds(&pattern, &ast, ALERT_REGEX_FOLD_MAX / parts)?;
    }
    let budget = ALERT_REGEX_SIZE_MAX.min(ALERT_REGEX_POOL_MAX / scopes) / shares;
    let regex = regex::RegexBuilder::new(&format!("\\A(?:{pattern})\\z"))
        .size_limit(budget)
        .dfa_size_limit(budget)
        .build()
        .ok()?;
    // Counted once the automaton fits its part, which bounds the copies to count.
    let mut compiled = Compiled {
        regex,
        looped: 0,
        spread: 0,
    };
    compiled.add(&ast, Some(1));
    Some(compiled)
}

/// Whether an `i` flag appears anywhere in `ast`, and how many Unicode classes it holds.
fn shape(ast: &Ast) -> (bool, usize) {
    #[derive(Default)]
    struct Shape {
        insensitive: bool,
        classes: usize,
    }
    impl ast::Visitor for Shape {
        type Output = (bool, usize);
        type Err = ();
        fn finish(self) -> Result<(bool, usize), ()> {
            Ok((self.insensitive, self.classes))
        }
        fn visit_pre(&mut self, ast: &Ast) -> Result<(), ()> {
            let flags = match ast {
                Ast::Flags(set) => &set.flags,
                Ast::Group(group) => match &group.kind {
                    GroupKind::NonCapturing(flags) => flags,
                    _ => return Ok(()),
                },
                Ast::ClassUnicode(_) => {
                    self.classes += 1;
                    return Ok(());
                }
                _ => return Ok(()),
            };
            self.insensitive |= flags.flag_state(Flag::CaseInsensitive) == Some(true);
            Ok(())
        }
        fn visit_class_set_item_pre(&mut self, item: &ClassSetItem) -> Result<(), ()> {
            self.classes += usize::from(matches!(item, ClassSetItem::Unicode(_)));
            Ok(())
        }
    }
    ast::visit(ast, Shape::default()).unwrap_or_default()
}

/// An upper bound on the code points `regex-syntax` 0.8 visits to fold the case of `ast`, parsed
/// from `pattern`, as if all of it were case-insensitive; `None` past `limit` or for a class it
/// cannot read. Each Unicode or ASCII class is folded on its own, and each bracket holding a
/// character or a range folds all it holds, its classes as folding left them: a fold visits
/// every code point of each range, before negation, that holds one of `CASED` (all 1.1 million
/// of `\p{Any}`, none of `\p{Han}`). A bracket of classes alone is folded already. A class in a
/// bracket is folded here too once its count fits, so the bracket holds the ranges
/// `regex-syntax` visits.
fn folds(pattern: &str, ast: &Ast, limit: usize) -> Option<usize> {
    /// An open bracket: its characters and ranges, and its classes as folding and negation left
    /// them.
    struct Bracket {
        ranges: Vec<ClassUnicodeRange>,
        classes: ClassUnicode,
    }
    struct Folds<'p> {
        pattern: &'p str,
        limit: usize,
        visits: usize,
        brackets: Vec<Bracket>,
    }
    impl<'p> Folds<'p> {
        fn open(&mut self) {
            let classes = ClassUnicode::empty();
            self.brackets.push(Bracket {
                ranges: Vec::new(),
                classes,
            });
        }
        fn fold(&mut self, class: &ClassUnicode) -> Result<(), ()> {
            self.visits = self.visits.saturating_add(cased(class));
            if self.visits > self.limit {
                Err(())
            } else {
                Ok(())
            }
        }
        /// A class folded alone, written `text`, then added folded to the bracket around it.
        fn alone(&mut self, text: Option<&str>, negated: bool) -> Result<(), ()> {
            let mut class = text.and_then(class).ok_or(())?;
            if negated {
                class.negate();
            }
            self.fold(&class)?;
            self.join(class, true, negated)
        }
        fn close(&mut self, negated: bool) -> Result<(), ()> {
            let bracket = self.brackets.pop().ok_or(())?;
            let mut class = ClassUnicode::new(bracket.ranges.iter().copied());
            class.union(&bracket.classes);
            // A bracket of classes alone holds them folded already.
            let unfolded = !bracket.ranges.is_empty();
            if unfolded {
                self.fold(&class)?;
            }
            self.join(class, unfolded, negated)
        }
        /// Adds `class` to the bracket around it, if any, as `regex-syntax` does: folded, once
        /// counted, then negated.
        fn join(&mut self, mut class: ClassUnicode, fold: bool, negated: bool) -> Result<(), ()> {
            if self.brackets.is_empty() {
                return Ok(());
            }
            if fold {
                class.try_case_fold_simple().map_err(|_| ())?;
            }
            if negated {
                class.negate();
            }
            self.brackets.last_mut().ok_or(())?.classes.union(&class);
            Ok(())
        }
        fn text(&self, span: &ast::Span) -> Option<&'p str> {
            self.pattern.get(span.start.offset..span.end.offset)
        }
    }
    impl ast::Visitor for Folds<'_> {
        type Output = usize;
        type Err = ();
        fn finish(self) -> Result<usize, ()> {
            Ok(self.visits)
        }
        fn visit_pre(&mut self, ast: &Ast) -> Result<(), ()> {
            match ast {
                Ast::ClassBracketed(_) => self.open(),
                Ast::ClassUnicode(class) => {
                    return self.alone(self.text(&class.span), class.is_negated());
                }
                _ => {}
            }
            Ok(())
        }
        fn visit_post(&mut self, ast: &Ast) -> Result<(), ()> {
            match ast {
                Ast::ClassBracketed(class) => self.close(class.negated),
                _ => Ok(()),
            }
        }
        fn visit_class_set_item_pre(&mut self, item: &ClassSetItem) -> Result<(), ()> {
            if let ClassSetItem::Bracketed(_) = item {
                self.open();
            }
            Ok(())
        }
        fn visit_class_set_item_post(&mut self, item: &ClassSetItem) -> Result<(), ()> {
            let range = match item {
                ClassSetItem::Literal(literal) => ClassUnicodeRange::new(literal.c, literal.c),
                ClassSetItem::Range(range) => ClassUnicodeRange::new(range.start.c, range.end.c),
                ClassSetItem::Unicode(class) => {
                    return self.alone(self.text(&class.span), class.is_negated());
                }
                ClassSetItem::Ascii(class) => {
                    let text = self.text(&class.span).map(|text| format!("[{text}]"));
                    return self.alone(text.as_deref(), class.negated);
                }
                ClassSetItem::Bracketed(class) => return self.close(class.negated),
                ClassSetItem::Empty(_) | ClassSetItem::Union(_) => return Ok(()),
                // `regex_source` writes Perl classes as brackets.
                ClassSetItem::Perl(_) => return Err(()),
            };
            self.brackets.last_mut().ok_or(())?.ranges.push(range);
            Ok(())
        }
        fn visit_class_set_binary_op_pre(&mut self, _: &ast::ClassSetBinaryOp) -> Result<(), ()> {
            // `regex_source` escapes or refuses the class operators.
            Err(())
        }
    }
    let folds = Folds {
        pattern,
        limit,
        visits: 0,
        brackets: Vec::new(),
    };
    ast::visit(ast, folds).ok()
}

/// The code points that change when case mapped, a superset of those simple case folding maps
/// (the keys of `regex-syntax`'s table, whose ranges folding visits).
static CASED: LazyLock<ClassUnicode> = LazyLock::new(|| {
    class(r"\p{CWCM}")
        .unwrap_or_else(|| ClassUnicode::new([ClassUnicodeRange::new('\0', char::MAX)]))
});

/// The code points of the ranges of `class` that hold one of `CASED`.
fn cased(class: &ClassUnicode) -> usize {
    let holds = |r: &ClassUnicodeRange| {
        let cased = CASED.ranges().binary_search_by(|c| {
            if c.end() < r.start() {
                std::cmp::Ordering::Less
            } else if c.start() > r.end() {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        cased.is_ok()
    };
    let ranges = class.ranges().iter().filter(|r| holds(r));
    ranges
        .map(|r| r.end() as usize - r.start() as usize + 1)
        .sum()
}

/// The class `text` writes, read alone and case-sensitively.
fn class(text: &str) -> Option<ClassUnicode> {
    Some(match regex_syntax::parse(text).ok()?.into_kind() {
        HirKind::Class(Class::Unicode(class)) => class,
        HirKind::Class(Class::Bytes(class)) if class.ranges().is_empty() => ClassUnicode::empty(),
        HirKind::Literal(literal) => {
            let chars = std::str::from_utf8(&literal.0).ok()?.chars();
            ClassUnicode::new(chars.map(|c| ClassUnicodeRange::new(c, c)))
        }
        _ => return None,
    })
}

/// Alertmanager uses Go's RE2 syntax: Perl classes and word boundaries are ASCII,
/// while ordinary characters and Unicode classes still operate on Unicode text.
///
/// `None` for what it cannot read; when Go's `regexp` certainly refuses it too, `refusal` also
/// names the `regexp/syntax` error code (see `refusal`).
fn regex_source(pattern: &str, refusal: &mut Option<&'static str>) -> Option<String> {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    let mut class = false;
    let mut class_head = 0;
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            if !class && ch == '{' {
                // Go treats malformed counted syntax as literal text, while Rust accepts
                // leading zeroes and spaces as a repetition. Refuse those ambiguous forms.
                let count = chars
                    .clone()
                    .take_while(|c| c.is_ascii_digit() || c.is_whitespace() || *c == ',')
                    .collect::<String>();
                if count.chars().any(|c| c.is_whitespace())
                    || count.split(',').any(|n| n.len() > 1 && n.starts_with('0'))
                {
                    return None;
                }
            }
            if class && class_head > 0 {
                if ch == '^' && class_head == 1 {
                    class_head = 2;
                    out.push(ch);
                    continue;
                }
                class_head = 0;
                if ch == ']' {
                    out.push('\\');
                    out.push(ch);
                    continue;
                }
            }
            if class && ch == '-' && chars.peek() == Some(&'-') {
                return None;
            }
            if !class && ch == '(' && chars.peek() == Some(&'?') {
                out.push(ch);
                out.push(chars.next()?);
                if chars.peek() == Some(&'P') {
                    out.push(chars.next()?);
                    if chars.peek() != Some(&'<') {
                        return refuse(refusal, PERL_OP);
                    }
                }
                if chars.peek() == Some(&'<') {
                    out.push(chars.next()?);
                    // Validate the original spelling before translating any escapes.
                    let start = out.len();
                    loop {
                        // Go names a group up to the first `>`: a name that is empty or holds
                        // another character than a letter, digit or `_` is refused.
                        let ch = chars.next()?;
                        if ch == '>' {
                            if out.len() == start {
                                return refuse(refusal, NAMED_CAPTURE);
                            }
                            out.push(ch);
                            break;
                        }
                        if !ch.is_ascii_alphanumeric() && ch != '_' {
                            return refuse(refusal, NAMED_CAPTURE);
                        }
                        out.push(ch);
                    }
                } else if chars.peek() != Some(&':') {
                    while let Some(&flag) = chars.peek() {
                        if matches!(flag, ':' | ')') {
                            break;
                        }
                        if !matches!(flag, 'i' | 'm' | 's' | 'U' | '-') {
                            return refuse(refusal, PERL_OP);
                        }
                        out.push(chars.next()?);
                    }
                }
                continue;
            }
            if class && ch == '[' {
                // RE2 has POSIX classes; another opening bracket inside a class is literal.
                if chars.peek() != Some(&':') {
                    out.push('\\');
                    out.push(ch);
                    continue;
                }
                out.push(ch);
                let mut previous = '[';
                loop {
                    let part = chars.next()?;
                    out.push(part);
                    if part == ']' {
                        if previous != ':' {
                            return None;
                        }
                        break;
                    }
                    previous = part;
                }
                continue;
            }
            // Rust's class intersection and symmetric difference are literals in RE2.
            if class && matches!(ch, '&' | '~') {
                out.push('\\');
            }
            if ch == '[' {
                class = true;
                class_head = 1;
            }
            if ch == ']' {
                class = false;
            }
            out.push(ch);
            continue;
        }
        class_head = 0;
        let escaped = chars.next()?;
        let ascii = match escaped {
            'd' => Some("[0-9]"),
            'D' => Some("[^0-9]"),
            'w' => Some("[A-Za-z0-9_]"),
            'W' => Some("[^A-Za-z0-9_]"),
            's' => Some(r"[\t\n\f\r ]"),
            'S' => Some(r"[^\t\n\f\r ]"),
            _ => None,
        };
        if let Some(ascii) = ascii {
            out.push_str(ascii);
            continue;
        }
        match escaped {
            'p' | 'P' => {
                let property = if chars.peek() == Some(&'{') {
                    chars.next();
                    let mut name = String::new();
                    loop {
                        // Go reads `\p{` to the closing brace and names no class without it.
                        let Some(ch) = chars.next() else {
                            return refuse(refusal, CHAR_RANGE);
                        };
                        if ch == '}' {
                            break;
                        }
                        name.push(ch);
                    }
                    name
                } else {
                    chars.next()?.to_string()
                };
                let (negated, name) = property
                    .strip_prefix('^')
                    .map_or((false, property.as_str()), |name| (true, name));
                if !unicode_property(name) {
                    return None;
                }
                let escaped = if negated {
                    if escaped == 'p' { 'P' } else { 'p' }
                } else {
                    escaped
                };
                out.push_str(&format!("\\{escaped}{{{name}}}"));
            }
            '0'..='7' => {
                // A lone `\1` to `\7` is a backreference, which Go does not read.
                if escaped != '0' && !chars.peek().is_some_and(|c| ('0'..='7').contains(c)) {
                    return refuse(refusal, ESCAPE);
                }
                let mut octal = escaped.to_digit(8)?;
                for _ in 0..2 {
                    if let Some(next) = chars.peek().and_then(|c| c.to_digit(8)) {
                        chars.next();
                        octal = octal * 8 + next;
                    } else {
                        break;
                    }
                }
                out.push_str(&format!("\\x{{{octal:x}}}"));
            }
            'Q' => {
                if class {
                    return None;
                }
                let mut literal = String::new();
                while let Some(ch) = chars.next() {
                    if ch == '\\' && chars.peek() == Some(&'E') {
                        chars.next();
                        break;
                    }
                    literal.push(ch);
                }
                out.push_str(&regex::escape(&literal));
            }
            'b' | 'B' if !class => out.push_str(&format!("(?-u:\\{escaped})")),
            // These Rust escapes are not RE2 escapes, which Go refuses.
            'u' | 'U' => return refuse(refusal, ESCAPE),
            _ => {
                out.push('\\');
                out.push(escaped);
            }
        }
    }
    Some(out)
}

/// Go bounds each nested counted-repeat product and refuses a repetition operator stacked on
/// another (`a**`, `a{2}*`): the code it refuses with.
fn re2_ast(ast: &Ast) -> Result<(), &'static str> {
    let mut pending = vec![(ast, 1u32)];
    while let Some((ast, parent)) = pending.pop() {
        match ast {
            Ast::Repetition(repetition) => {
                let copies = match &repetition.op.kind {
                    RepetitionKind::Range(
                        RepetitionRange::Exactly(n)
                        | RepetitionRange::AtLeast(n)
                        | RepetitionRange::Bounded(_, n),
                    ) => *n,
                    _ => 1,
                };
                let product = parent.checked_mul(copies).ok_or(REPEAT_SIZE)?;
                if matches!(*repetition.ast, Ast::Repetition(_)) {
                    return Err(REPEAT_OP);
                }
                if copies > RE2_REPEAT_MAX || product > RE2_REPEAT_MAX {
                    return Err(REPEAT_SIZE);
                }
                pending.push((&repetition.ast, product));
            }
            Ast::Group(group) => {
                pending.push((&group.ast, parent));
            }
            Ast::Concat(concat) => pending.extend(concat.asts.iter().map(|ast| (ast, parent))),
            Ast::Alternation(alternation) => {
                pending.extend(alternation.asts.iter().map(|ast| (ast, parent)))
            }
            _ => {}
        }
    }
    Ok(())
}

fn unicode_property(name: &str) -> bool {
    matches!(
        name,
        "Any"
            | "C"
            | "Cc"
            | "Cf"
            | "Co"
            | "Cs"
            | "L"
            | "Ll"
            | "Lm"
            | "Lo"
            | "Lt"
            | "Lu"
            | "M"
            | "Mc"
            | "Me"
            | "Mn"
            | "N"
            | "Nd"
            | "Nl"
            | "No"
            | "P"
            | "Pc"
            | "Pd"
            | "Pe"
            | "Pf"
            | "Pi"
            | "Po"
            | "Ps"
            | "S"
            | "Sc"
            | "Sk"
            | "Sm"
            | "So"
            | "Z"
            | "Zl"
            | "Zp"
            | "Zs"
    ) || SCRIPTS.contains(&name)
}

// Canonical script names accepted by Go unicode.Scripts, not Rust's additional aliases or
// binary properties. Source: https://github.com/golang/go/blob/master/src/unicode/tables.go
const SCRIPTS: &[&str] = &[
    "Adlam",
    "Ahom",
    "Anatolian_Hieroglyphs",
    "Arabic",
    "Armenian",
    "Avestan",
    "Balinese",
    "Bamum",
    "Bassa_Vah",
    "Batak",
    "Bengali",
    "Beria_Erfe",
    "Bhaiksuki",
    "Bopomofo",
    "Brahmi",
    "Braille",
    "Buginese",
    "Buhid",
    "Canadian_Aboriginal",
    "Carian",
    "Caucasian_Albanian",
    "Chakma",
    "Cham",
    "Cherokee",
    "Chorasmian",
    "Common",
    "Coptic",
    "Cuneiform",
    "Cypriot",
    "Cypro_Minoan",
    "Cyrillic",
    "Deseret",
    "Devanagari",
    "Dives_Akuru",
    "Dogra",
    "Duployan",
    "Egyptian_Hieroglyphs",
    "Elbasan",
    "Elymaic",
    "Ethiopic",
    "Garay",
    "Georgian",
    "Glagolitic",
    "Gothic",
    "Grantha",
    "Greek",
    "Gujarati",
    "Gunjala_Gondi",
    "Gurmukhi",
    "Gurung_Khema",
    "Han",
    "Hangul",
    "Hanifi_Rohingya",
    "Hanunoo",
    "Hatran",
    "Hebrew",
    "Hiragana",
    "Imperial_Aramaic",
    "Inherited",
    "Inscriptional_Pahlavi",
    "Inscriptional_Parthian",
    "Javanese",
    "Kaithi",
    "Kannada",
    "Katakana",
    "Kawi",
    "Kayah_Li",
    "Kharoshthi",
    "Khitan_Small_Script",
    "Khmer",
    "Khojki",
    "Khudawadi",
    "Kirat_Rai",
    "Lao",
    "Latin",
    "Lepcha",
    "Limbu",
    "Linear_A",
    "Linear_B",
    "Lisu",
    "Lycian",
    "Lydian",
    "Mahajani",
    "Makasar",
    "Malayalam",
    "Mandaic",
    "Manichaean",
    "Marchen",
    "Masaram_Gondi",
    "Medefaidrin",
    "Meetei_Mayek",
    "Mende_Kikakui",
    "Meroitic_Cursive",
    "Meroitic_Hieroglyphs",
    "Miao",
    "Modi",
    "Mongolian",
    "Mro",
    "Multani",
    "Myanmar",
    "Nabataean",
    "Nag_Mundari",
    "Nandinagari",
    "New_Tai_Lue",
    "Newa",
    "Nko",
    "Nushu",
    "Nyiakeng_Puachue_Hmong",
    "Ogham",
    "Ol_Chiki",
    "Ol_Onal",
    "Old_Hungarian",
    "Old_Italic",
    "Old_North_Arabian",
    "Old_Permic",
    "Old_Persian",
    "Old_Sogdian",
    "Old_South_Arabian",
    "Old_Turkic",
    "Old_Uyghur",
    "Oriya",
    "Osage",
    "Osmanya",
    "Pahawh_Hmong",
    "Palmyrene",
    "Pau_Cin_Hau",
    "Phags_Pa",
    "Phoenician",
    "Psalter_Pahlavi",
    "Rejang",
    "Runic",
    "Samaritan",
    "Saurashtra",
    "Sharada",
    "Shavian",
    "Siddham",
    "Sidetic",
    "SignWriting",
    "Sinhala",
    "Sogdian",
    "Sora_Sompeng",
    "Soyombo",
    "Sundanese",
    "Sunuwar",
    "Syloti_Nagri",
    "Syriac",
    "Tagalog",
    "Tagbanwa",
    "Tai_Le",
    "Tai_Tham",
    "Tai_Viet",
    "Tai_Yo",
    "Takri",
    "Tamil",
    "Tangsa",
    "Tangut",
    "Telugu",
    "Thaana",
    "Thai",
    "Tibetan",
    "Tifinagh",
    "Tirhuta",
    "Todhri",
    "Tolong_Siki",
    "Toto",
    "Tulu_Tigalari",
    "Ugaritic",
    "Vai",
    "Vithkuqi",
    "Wancho",
    "Warang_Citi",
    "Yezidi",
    "Yi",
    "Zanabazar_Square",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// `folds` counts a range only when it holds one of `CASED`, which holds every code point
    /// simple case folding maps: folding all the others leaves them as they are.
    #[test]
    fn case_folding_maps_only_cased_code_points() {
        let mut uncased = CASED.clone();
        uncased.negate();
        let before = uncased.clone();
        uncased.case_fold_simple();
        assert_eq!(uncased, before);
        let count = |text: &str| class(text).map(|class| cased(&class));
        let counts = [count("A"), count("[0-9]"), count("[0-9a]"), count("[0-a]")];
        assert_eq!(counts, [Some(1), Some(0), Some(1), Some(50)]);
    }

    /// `(?i)[\w-]` is read as `[[A-Za-z0-9_]-]`: the inner bracket visits its 52 letters, and the
    /// outer one visits them again with the Kelvin sign and long s their folding added, 54, not
    /// every cased code point.
    #[test]
    fn a_bracket_of_characters_and_classes_visits_what_its_classes_fold_to() {
        let pattern = regex_source(r"(?i)[\w-]", &mut None).unwrap();
        let ast = ast::parse::Parser::new().parse(&pattern).unwrap();
        assert_eq!(folds(&pattern, &ast, usize::MAX), Some(52 + 54));
    }

    /// Matching takes a step per position and offset it may start at: inside or after an
    /// unbounded repetition, every offset of the value and one more; elsewhere, one more than the
    /// characters what precedes it may vary by. A literal alternation costs its characters,
    /// whatever the value. A state that reads no character is a position too: an assertion, an
    /// empty expression, each end of a capture, the choice of an alternation and the loop or
    /// option of a repetition, so each `()` of `.*w(?:()[wy]){2}` is three.
    #[test]
    fn matching_costs_a_step_per_position_and_offset_it_may_start_at() {
        let cost = |pattern: &str, len: usize| compile(pattern, 1, 1).map(|c| c.cost(len));
        assert_eq!(
            [
                cost(".*a.{1000}", 99),
                cost("team-(?:a|bb)-x", 1_000_000),
                cost("a?b", 10),
                cost("(?:a?){3}", 10),
                cost("x{2,}y", 9),
                cost("[a-z0-9-]{1,63}", 1_000_000),
                cost("a{0}b", 10),
                cost(r".*(?:\b|\B){100}", 9),
                cost(".*w(?:()[wy]){2}", 9),
            ],
            [100_300, 13, 4, 12, 31, 125, 1, 3020, 110].map(Some)
        );
    }

    /// Each class of expression `refusal` names, all refused by Go's `regexp/syntax` with the
    /// error code beside it, as `regexp.Compile("^(?:" + pattern + ")$")` (Go 1.24; the wrapper
    /// Alertmanager puts around a `match_re` and a matcher value).
    #[test]
    fn go_refuses_these_classes_of_syntax_error_with_the_code_named() {
        for (code, patterns) in [
            (MISSING_PAREN, ["(unclosed", "team-(a|b", "(?i"].as_slice()),
            (UNEXPECTED_PAREN, &["team-a)", "a))"]),
            (MISSING_BRACKET, &["[", "team-[a-z"]),
            (
                MISSING_REPEAT,
                &["*prod*", "+x", "a|*", "(?:*)", "(+)", "(?i:?)"],
            ),
            (CHAR_RANGE, &["[z-a]", "(?i)[a-Z]", r"\p{Han"]),
            (
                REPEAT_SIZE,
                &[
                    "x{2,1}",
                    "x{1001}",
                    "x{1,1001}",
                    "(?:a{40}){40}",
                    "a{100000000}",
                ],
            ),
            (REPEAT_OP, &["a**", "a{2}*", "a*??"]),
            (ESCAPE, &[r"\y", r"\8", r"\1", r"\u0041", r"\U0001F600"]),
            (
                PERL_OP,
                &["(?x:a)", "(?=a)", "(?!a)", "(?P=n)", "(?i-)", "(?-)"],
            ),
            (
                NAMED_CAPTURE,
                &["(?P<a.b>x)", "(?P<>x)", "(?P<n", "(?<é>x)"],
            ),
        ] {
            for pattern in patterns {
                assert_eq!(refusal(pattern), Some(code), "{pattern}");
                assert!(compile(pattern, 1, 1).is_none(), "{pattern}");
            }
        }
    }

    /// What `refusal` leaves alone because Go compiles it, or because this cannot tell. None of
    /// it refuses a configuration: a pattern past `ALERT_REGEX_MAX` (valid, or not), one past the
    /// automaton budgets, syntax this module does not read, and what `regex-syntax` refuses and
    /// Go reads, so that a `compile` that answers `None` is no proof of a refusal. Go's own
    /// answer is in each comment.
    #[test]
    fn what_go_accepts_or_this_cannot_tell_is_no_refusal() {
        let deep = format!("{}a{}", "(".repeat(300), ")".repeat(300));
        let many = r"\pL".repeat(ALERT_REGEX_CLASSES_MAX + 1);
        for (why, pattern) in [
            // Past the budgets of Meada, valid for Go; invalid for Go, but never judged past
            // `ALERT_REGEX_MAX`.
            ("regex bytes", "a".repeat(ALERT_REGEX_MAX + 1)),
            ("regex bytes", format!("({}", "a".repeat(ALERT_REGEX_MAX))),
            ("classes", many),
            ("automaton", "x{1000}".repeat(1000)),
            // Valid for Go, which allows 1000 nested groups where `regex-syntax` allows 250.
            ("nesting", deep),
            // Valid for Go, and read differently by `regex-syntax`.
            ("count text", "a{,5}".into()),
            ("count text", "a{01}".into()),
            ("count text", "a{ 1}".into()),
            ("repeated flag", "(?ii)a".into()),
            ("class then dash", r"[\d-z]".into()),
            ("class in class", "[t[e]]eam-b".into()),
            ("empty flags", "(?)".into()),
            ("flags then repeat", "a(?i)*".into()),
            ("brace text", "{x".into()),
            // The wrapper's `)` closes a group this one opens, and its own `)` is left over.
            ("closing the wrapper", "a)(b".into()),
            ("POSIX class", "[[:alpha:]]".into()),
            ("POSIX look-alike", "[[:]".into()),
            // Refused by Go, but not told apart here: a Unicode property name follows Go's
            // version, and `[:` and `\Q` are read more leniently than Go does.
            ("property name", r"\p{Alphabetic}".into()),
            ("quoted tail", r"\Qteam".into()),
            ("POSIX tail", "([[:alpha:]]".into()),
        ] {
            assert_eq!(refusal(&pattern), None, "{why}");
        }
        // `compile` answers `None` for these as well.
        assert!(compile("a{,5}", 1, 1).is_none());
        assert!(compile(&"a".repeat(ALERT_REGEX_MAX + 1), 1, 1).is_none());
    }
}
