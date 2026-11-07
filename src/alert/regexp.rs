//! Alertmanager/Go RE2 compatibility on the bounded Rust regex engine.
use super::ALERT_REGEX_MAX;
use regex_syntax::ast::{Ast, RepetitionKind, RepetitionRange};

// ponytail: Go/RE2 rejects counted repetitions, including nested products, above 1000.
const RE2_REPEAT_MAX: u32 = 1000;

pub(super) fn compile(pattern: &str, budget: usize) -> Option<regex::Regex> {
    if pattern.len() > ALERT_REGEX_MAX {
        return None;
    }
    let pattern = regex_source(pattern)?;
    re2_ast(&pattern)?;
    regex::RegexBuilder::new(&format!("\\A(?:{pattern})\\z"))
        .size_limit(budget)
        .dfa_size_limit(budget)
        .build()
        .ok()
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
fn re2_ast(pattern: &str) -> Option<()> {
    let ast = regex_syntax::ast::parse::Parser::new()
        .parse(pattern)
        .ok()?;
    let mut pending = vec![(&ast, 1u32)];
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
