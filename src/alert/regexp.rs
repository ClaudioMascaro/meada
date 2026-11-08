//! Alertmanager/Go RE2 compatibility on the bounded Rust regex engine.
use super::{ALERT_REGEX_CLASSES_MAX, ALERT_REGEX_FOLD_MAX, ALERT_REGEX_MAX, ALERT_REGEX_SIZE_MAX};
use regex_syntax::ast::{
    self, Ast, ClassSetItem, Flag, GroupKind, RepetitionKind, RepetitionRange,
};
use regex_syntax::hir::{Class, ClassUnicode, ClassUnicodeRange, HirKind};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

// ponytail: Go/RE2 rejects counted repetitions, including nested products, above 1000.
const RE2_REPEAT_MAX: u32 = 1000;

/// The expressions one linking pass compiled, each once for its share, and what compiling may
/// still spend in the pass.
pub(super) struct Pass<'a> {
    left: Left,
    compiled: HashMap<(&'a str, usize), Option<Arc<regex::Regex>>>,
}
/// The case folding visits and Unicode classes a linking pass may still spend.
struct Left {
    folds: usize,
    classes: usize,
}
impl Default for Pass<'_> {
    fn default() -> Self {
        let left = Left {
            folds: ALERT_REGEX_FOLD_MAX,
            classes: ALERT_REGEX_CLASSES_MAX,
        };
        Self {
            left,
            compiled: HashMap::new(),
        }
    }
}
impl<'a> Pass<'a> {
    /// `pattern` as Alertmanager reads it, anchored to the whole value, within its share of
    /// `ALERT_REGEX_SIZE_MAX` and `ALERT_REGEX_FOLD_MAX` (one of `shares` distinct expressions of
    /// a route configuration) and the folding and Unicode classes left to the pass; `None` past
    /// them, past `ALERT_REGEX_MAX` or for no RE2 expression.
    pub(super) fn compile(&mut self, pattern: &'a str, shares: usize) -> Option<Arc<regex::Regex>> {
        let left = &mut self.left;
        let compiled = self.compiled.entry((pattern, shares));
        compiled
            .or_insert_with(|| compile(pattern, shares, left).map(Arc::new))
            .clone()
    }
}

/// `Pass::compile`, spending what is `left`.
fn compile(pattern: &str, shares: usize, left: &mut Left) -> Option<regex::Regex> {
    if pattern.len() > ALERT_REGEX_MAX {
        return None;
    }
    let pattern = regex_source(pattern)?;
    let ast = ast::parse::Parser::new().parse(&pattern).ok()?;
    re2_ast(&ast)?;
    let (insensitive, classes) = shape(&ast);
    left.classes = left.classes.checked_sub(classes)?;
    let shares = shares.max(1);
    if insensitive {
        let limit = (ALERT_REGEX_FOLD_MAX / shares).min(left.folds);
        left.folds -= folds(&pattern, &ast, limit)?;
    }
    let budget = ALERT_REGEX_SIZE_MAX / shares;
    regex::RegexBuilder::new(&format!("\\A(?:{pattern})\\z"))
        .size_limit(budget)
        .dfa_size_limit(budget)
        .build()
        .ok()
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
/// character or a range folds all it holds (its classes again, already folded): a fold visits
/// every code point of each range, before negation, that holds one of `CASED` (all 1.1 million
/// of `\p{Any}`, none of `\p{Han}`). A bracket of classes alone is folded already.
fn folds(pattern: &str, ast: &Ast, limit: usize) -> Option<usize> {
    /// An open bracket: its characters and ranges, and its classes as they are after negation.
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
        /// A class folded alone, written `text`, then added to the bracket around it.
        fn alone(&mut self, text: Option<&str>, negated: bool) -> Result<(), ()> {
            let mut class = text.and_then(class).ok_or(())?;
            if negated {
                class.negate();
            }
            self.fold(&class)?;
            if negated {
                class.negate();
            }
            if let Some(bracket) = self.brackets.last_mut() {
                bracket.classes.union(&class);
            }
            Ok(())
        }
        fn close(&mut self, negated: bool) -> Result<(), ()> {
            let bracket = self.brackets.pop().ok_or(())?;
            let mut class = ClassUnicode::new(bracket.ranges.iter().copied());
            class.union(&bracket.classes);
            if !bracket.ranges.is_empty() {
                // Folding a class adds only cased code points to what it holds.
                let mut folded = class.clone();
                if !bracket.classes.ranges().is_empty() {
                    folded.union(&CASED);
                }
                self.fold(&folded)?;
            }
            if negated {
                class.negate();
            }
            if let Some(outer) = self.brackets.last_mut() {
                outer.classes.union(&class);
            }
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
fn regex_source(pattern: &str) -> Option<String> {
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
                        return None;
                    }
                }
                if chars.peek() == Some(&'<') {
                    out.push(chars.next()?);
                    // Validate the original spelling before translating any escapes.
                    let start = out.len();
                    loop {
                        let ch = chars.next()?;
                        if ch == '>' {
                            if out.len() == start {
                                return None;
                            }
                            out.push(ch);
                            break;
                        }
                        if !ch.is_ascii_alphanumeric() && ch != '_' {
                            return None;
                        }
                        out.push(ch);
                    }
                } else if chars.peek() != Some(&':') {
                    while let Some(&flag) = chars.peek() {
                        if matches!(flag, ':' | ')') {
                            break;
                        }
                        if !matches!(flag, 'i' | 'm' | 's' | 'U' | '-') {
                            return None;
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
                        let ch = chars.next()?;
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
                if escaped != '0' && !chars.peek().is_some_and(|c| ('0'..='7').contains(c)) {
                    return None;
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
            // These Rust escapes are not RE2 escapes. Unsupported syntax stays unmatched.
            'u' | 'U' => return None,
            _ => {
                out.push('\\');
                out.push(escaped);
            }
        }
    }
    Some(out)
}

/// Go bounds each nested counted-repeat product.
fn re2_ast(ast: &Ast) -> Option<()> {
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
                let product = parent.checked_mul(copies)?;
                if copies > RE2_REPEAT_MAX || product > RE2_REPEAT_MAX {
                    return None;
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
    Some(())
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
}
