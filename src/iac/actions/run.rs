//! The gate of the command table (ADR 0028, "The `run` script and the command table"): the
//! `run` script of a CI step in, its commands and their words out. No shell is evaluated: an
//! assignment, `cd`, a function call and a called script are not followed, and the rows that
//! give a command its side live in the kinds (`package`, `image`).

use std::cell::{Cell, OnceCell};
use std::collections::HashSet;

use crate::config_values::VALUE_MAX;

/// A part of a word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// Literal text, its quotes removed.
    Text(String),
    /// What static evaluation leaves open, as written: a `${{ }}` expression or a variable no
    /// literal `env` gives, a command substitution, any other expansion.
    Open(String),
}

/// A word of a command: its parts in order, adjacent text joined.
pub(crate) type Word = Vec<Part>;

/// The commands a CI step's `run` script holds, by the identity of the CI step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Script {
    pub site: String,
    pub commands: Vec<Vec<Word>>,
}

/// The text of a word with no open part.
pub(crate) fn literal(word: &[Part]) -> Option<&str> {
    match word {
        [] => Some(""),
        [Part::Text(text)] => Some(text),
        _ => None,
    }
}

/// The words that open a command of a compound: the command inside `if` or a loop counts, since
/// the CI step may run it.
const KEYWORDS: [&str; 9] = [
    "if", "then", "else", "elif", "do", "while", "until", "!", "{",
];

/// The names `script` mentions other than by reading them (`$NAME`, `${NAME}`, `env.NAME`): an
/// assignment, `export`, `read`, `for NAME in`, a line written to `GITHUB_ENV`. Such a name may
/// hold another value than the one a literal `env` gives.
pub(crate) fn rebound(script: &str) -> HashSet<&str> {
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = script.as_bytes();
    let mut names = HashSet::new();
    let mut at = 0;
    while at < bytes.len() {
        let end = at + bytes[at..].iter().take_while(|&&b| word(b)).count();
        if end == at {
            at += 1;
            continue;
        }
        let before = &script[..at];
        let assigns = matches!(&bytes[end..], [b'=', ..] | [b':', b'=', ..]);
        let read = match before.ends_with("${") {
            true => !assigns,
            false => before.ends_with('$') || before.ends_with("env."),
        };
        if !read {
            names.insert(&script[at..end]);
        }
        at = end;
    }
    names
}

/// The offset past the bracket that closes the one at `at`, over quotes and nested ones of its
/// kind. `None` when it is left open or holds a heredoc.
fn closed(bytes: &[u8], at: usize) -> Option<usize> {
    let (opens, closes) = match bytes[at] {
        b'(' => (b'(', b')'),
        _ => (b'{', b'}'),
    };
    let (mut depth, mut i) = (0usize, at);
    loop {
        match *bytes.get(i)? {
            b if b == opens => depth += 1,
            b if b == closes => depth -= 1,
            b'\\' => i += 1,
            quote @ (b'\'' | b'"') => {
                i += 1;
                while *bytes.get(i)? != quote {
                    i += 1 + usize::from(quote == b'"' && bytes[i] == b'\\');
                }
            }
            b'<' if bytes[i..].starts_with(b"<<<") => i += 2,
            b'<' if bytes[i..].starts_with(b"<<") => return None,
            _ => {}
        }
        i += 1;
        if depth == 0 {
            return Some(i);
        }
    }
}

/// The commands of `script`: a command is the first word of a line or the one after `&&` or
/// `;` (a trailing `\` joins two lines), with the words up to the next operator. What follows
/// `||`, a pipe, `&`, a redirection or a parenthesis is not read up to the next line, `&&` or
/// `;`. A parenthesis that opens a command is a subshell, whose commands count; two of them
/// are an arithmetic command, and any other parenthesis (an array, a function's `()`) is
/// passed over to the one that closes it, lines included. A reserved word written bare opens
/// no command of its own (`KEYWORDS`); quoted, escaped or given by a variable it is a word
/// like any other. Text in a string or a comment gives no command. `None` for a script that
/// is not read: one that holds a heredoc (also the `<<` of an arithmetic expansion) or leaves
/// a quote, a parenthesis, an expansion or an expression open.
///
/// `env` gives the literal an `env` of the CI step, its CI job or its pipeline holds for a
/// name. It resolves `${{ env.NAME }}`, `$NAME` and `${NAME}` when the script does not rebind
/// the name (`rebound`) and the literal holds only characters the shell reads as they are.
pub(crate) fn commands<'v>(
    script: &str,
    env: impl Fn(&str) -> Option<&'v str>,
) -> Option<Vec<Vec<Word>>> {
    let plain = |v: &&str| {
        !v.is_empty() && (v.bytes()).all(|b| b.is_ascii_alphanumeric() || b"._=-".contains(&b))
    };
    // ponytail: the literals read add at most the script's own length and one value's, so a
    // script of `$X` alone never grows past a constant factor; past that a variable stays open.
    let room = Cell::new(script.len() + VALUE_MAX);
    let names = OnceCell::new();
    let value = |name: &str| {
        let found = env(name).filter(plain)?;
        if names.get_or_init(|| rebound(script)).contains(name) {
            return None;
        }
        room.set(room.get().checked_sub(found.len())?);
        Some(found)
    };
    let bytes = script.as_bytes();
    let mut out: Vec<Vec<Word>> = Vec::new();
    let mut words: Vec<Word> = Vec::new();
    let mut word: Option<Word> = None;
    // Whether the words read belong to a command: not after an operator that ends one.
    let mut reading = true;
    // Whether the word being read is written bare: no quote, escape or expansion in it.
    let mut bare = true;
    // The subshells left open.
    let mut subshells = 0usize;
    // The word read joins its command, unless it is a reserved word that opens it.
    let finish = |words: &mut Vec<Word>, word: &mut Option<Word>, bare: &mut bool, reading| {
        let keyword = |w: &Word| literal(w).is_some_and(|t| KEYWORDS.contains(&t));
        let opens = *bare && words.is_empty();
        words.extend(word.take().filter(|w| reading && !(opens && keyword(w))));
        *bare = true;
    };
    let text = |word: &mut Option<Word>, text: &str| {
        let word = word.get_or_insert_with(Vec::new);
        match word.last_mut() {
            Some(Part::Text(last)) => last.push_str(text),
            _ if text.is_empty() => {}
            _ => word.push(Part::Text(text.to_string())),
        }
    };
    let part = |word: &mut Option<Word>, part: Part| match part {
        Part::Text(found) => text(word, &found),
        open => word.get_or_insert_with(Vec::new).push(open),
    };
    let mut i = 0;
    while i < bytes.len() {
        let rest = &script[i..];
        // What ends the command, and whether the next words start one.
        let mut ends = None;
        match bytes[i] {
            b'\\' => match rest[1..].chars().next() {
                Some('\n') | None => i += 2,
                Some(next) => {
                    text(&mut word, &rest[1..1 + next.len_utf8()]);
                    bare = false;
                    i += 1 + next.len_utf8();
                }
            },
            b' ' | b'\t' | b'\r' => {
                finish(&mut words, &mut word, &mut bare, reading);
                i += 1;
            }
            b'\n' | b';' => (ends, i) = (Some(true), i + 1),
            b'&' if rest.starts_with("&&") => (ends, i) = (Some(true), i + 2),
            b'&' | b'|' => (ends, i) = (Some(false), i + 1),
            // A `)` with no subshell open closes a pattern of `case`.
            b')' => {
                subshells = subshells.saturating_sub(1);
                (ends, i) = (Some(false), i + 1);
            }
            b'(' if word.is_none() && words.is_empty() && reading => match bytes.get(i + 1) {
                // Arithmetic: no command inside.
                Some(b'(') => (ends, i) = (Some(false), closed(bytes, i)?),
                // A subshell, whose commands the CI step runs.
                _ => (subshells, i) = (subshells + 1, i + 1),
            },
            b'(' => (ends, i) = (Some(false), closed(bytes, i)?),
            b'<' if rest.starts_with("<<<") => (ends, i) = (Some(false), i + 3),
            b'<' if rest.starts_with("<<") => return None,
            b'<' | b'>' => {
                // The digits right before a redirection are its file descriptor.
                let descriptor =
                    |w: &Word| literal(w).is_some_and(|t| t.bytes().all(|b| b.is_ascii_digit()));
                word = word.filter(|w| !descriptor(w));
                (ends, i) = (Some(false), i + 1);
            }
            b'#' if word.is_none() => i += rest.find('\n').unwrap_or(rest.len()),
            b'\'' => {
                bare = false;
                let end = rest[1..].find('\'')?;
                // GitHub replaces an expression before the shell reads the quotes.
                let mut inner = &rest[1..1 + end];
                text(&mut word, "");
                while let Some(at) = inner.find("${{") {
                    text(&mut word, &inner[..at]);
                    let (found, length) = expansion(&inner[at..], value)?;
                    part(&mut word, found);
                    inner = &inner[at + length..];
                }
                text(&mut word, inner);
                i += end + 2;
            }
            b'"' => {
                bare = false;
                text(&mut word, "");
                i += 1;
                loop {
                    let rest = &script[i..];
                    let next = rest.chars().next()?;
                    match next {
                        '"' => break,
                        '\\' => match rest[1..].chars().next()? {
                            '\n' => i += 2,
                            c @ ('$' | '`' | '"' | '\\') => {
                                text(&mut word, &rest[1..1 + c.len_utf8()]);
                                i += 2;
                            }
                            _ => {
                                text(&mut word, "\\");
                                i += 1;
                            }
                        },
                        '$' | '`' => {
                            let (found, length) = expansion(rest, value)?;
                            part(&mut word, found);
                            i += length;
                        }
                        c => {
                            text(&mut word, &rest[..c.len_utf8()]);
                            i += c.len_utf8();
                        }
                    }
                }
                i += 1;
            }
            b'$' | b'`' => {
                bare = false;
                let (found, length) = expansion(rest, value)?;
                part(&mut word, found);
                i += length;
            }
            _ => {
                let c = rest.chars().next()?;
                text(&mut word, &rest[..c.len_utf8()]);
                i += c.len_utf8();
            }
        }
        if let Some(next) = ends {
            finish(&mut words, &mut word, &mut bare, reading);
            out.extend(Some(std::mem::take(&mut words)).filter(|w| !w.is_empty()));
            reading = next;
        }
    }
    finish(&mut words, &mut word, &mut bare, reading);
    out.extend(Some(words).filter(|w| !w.is_empty()));
    // A subshell left open is a script the shell does not run.
    (subshells == 0).then_some(out)
}

/// The expansion `rest` starts with (`$` or a backtick), as a part and its length in bytes:
/// text when `value` resolves it or the `$` stands alone, else open as written. `None` when it
/// is left unclosed or holds a heredoc.
fn expansion<'v>(rest: &str, value: impl Fn(&str) -> Option<&'v str>) -> Option<(Part, usize)> {
    let name = |text: &str| {
        let plain = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let length = text.bytes().take_while(|&b| plain(b)).count();
        (length > 0 && !text.as_bytes()[0].is_ascii_digit()).then_some(length)
    };
    let resolved = |name: &str, length: usize| {
        let found = value(name).map(|v| Part::Text(v.to_string()));
        (
            found.unwrap_or_else(|| Part::Open(rest[..length].to_string())),
            length,
        )
    };
    let open = |length: usize| (Part::Open(rest[..length].to_string()), length);
    let bytes = rest.as_bytes();
    if bytes[0] == b'`' {
        let mut i = 1;
        while *bytes.get(i)? != b'`' {
            i += 1 + usize::from(bytes[i] == b'\\');
        }
        let heredoc = rest[1..i].replace("<<<", "").contains("<<");
        return (!heredoc).then(|| open(i + 1));
    }
    if let Some(inner) = rest.strip_prefix("${{") {
        let length = 3 + inner.find("}}")? + 2;
        let inner = inner[..length - 5].trim();
        let named = inner.strip_prefix("env.");
        return Some(match named.filter(|n| name(n) == Some(n.len())) {
            Some(name) => resolved(name, length),
            None => open(length),
        });
    }
    match *bytes.get(1).unwrap_or(&b' ') {
        b'(' | b'{' => {
            let i = closed(bytes, 1)?;
            let inner = &rest[2..i - 1];
            Some(match bytes[1] == b'{' && name(inner) == Some(inner.len()) {
                true => resolved(inner, i),
                false => open(i),
            })
        }
        b'\'' => {
            let mut i = 2;
            while *bytes.get(i)? != b'\'' {
                i += 1 + usize::from(bytes[i] == b'\\');
            }
            Some(open(i + 1))
        }
        b if b.is_ascii_digit() || b"@*#?$!-".contains(&b) => Some(open(2)),
        _ => Some(match name(&rest[1..]) {
            Some(length) => resolved(&rest[1..1 + length], 1 + length),
            None => (Part::Text("$".into()), 1),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each command on a line of its own, an open part between `<` and `>`.
    fn read(script: &str) -> Option<Vec<String>> {
        let env = |name: &str| match name {
            "PKG" => Some("acme-lib"),
            "KW" => Some("then"),
            "SPACED" => Some("acme-lib acme-tool"),
            "EMPTY" => Some(""),
            _ => None,
        };
        let shown = |word: &Word| -> String {
            let part = |p: &Part| match p {
                Part::Text(text) => text.clone(),
                Part::Open(written) => format!("<{written}>"),
            };
            word.iter().map(part).collect()
        };
        let line = |command: &Vec<Word>| command.iter().map(shown).collect::<Vec<_>>().join(" ");
        Some(commands(script, env)?.iter().map(line).collect())
    }

    #[test]
    fn the_gate_reads_a_script_one_rule_at_a_time() {
        let cases: &[(&str, &str, Option<&[&str]>)] = &[
            // Unit: each line, a trailing backslash joined.
            (
                "lines",
                "pip install a\npip install b",
                Some(&["pip install a", "pip install b"]),
            ),
            (
                "continuation",
                "pip install \\\n  a",
                Some(&["pip install a"]),
            ),
            (
                "carriage return",
                "pip install a\r\npip install b\r\n",
                Some(&["pip install a", "pip install b"]),
            ),
            // A heredoc leaves the whole script unread; a here-string is a redirection.
            ("heredoc", "pip install a\ncat <<EOF\nx\nEOF", None),
            ("heredoc, tabs stripped", "cat <<-EOF\n\tx\n\tEOF", None),
            (
                "heredoc in a substitution",
                "X=$(cat <<EOF\nx\nEOF\n)",
                None,
            ),
            (
                "heredoc in backticks",
                "pip install a\nx=`cat <<EOF\nhi\nEOF\n`",
                None,
            ),
            (
                "here-string in backticks",
                "x=`cat <<< hi`\npip install a",
                Some(&["x=<`cat <<< hi`>", "pip install a"]),
            ),
            (
                "here-string",
                "cat <<< \"pip install a\" && pip install b",
                Some(&["cat", "pip install b"]),
            ),
            // Command: the first word of a line, or the one after `&&` or `;`.
            (
                "and",
                "cd x && pip install a",
                Some(&["cd x", "pip install a"]),
            ),
            (
                "semicolon",
                "cd x; pip install a",
                Some(&["cd x", "pip install a"]),
            ),
            ("or", "true || pip install a", Some(&["true"])),
            (
                "or, then and",
                "true || false && pip install a",
                Some(&["true", "pip install a"]),
            ),
            ("pipe from", "yes | pip install a", Some(&["yes"])),
            (
                "pipe into",
                "pip install a | tee log",
                Some(&["pip install a"]),
            ),
            ("background", "sleep 1 & pip install a", Some(&["sleep 1"])),
            (
                "prefix",
                "sudo pip install a",
                Some(&["sudo pip install a"]),
            ),
            (
                "assignment prefix",
                "X=1 pip install a",
                Some(&["X=1 pip install a"]),
            ),
            // Text inside `echo`, a string or a comment gives no command.
            ("echo", "echo pip install a", Some(&["echo pip install a"])),
            (
                "string",
                "echo \"x; pip install a\" 'y && pip install b'",
                Some(&["echo x; pip install a y && pip install b"]),
            ),
            (
                "comment line",
                "# pip install a\npip install b",
                Some(&["pip install b"]),
            ),
            (
                "trailing comment",
                "pip install a # pip install b",
                Some(&["pip install a"]),
            ),
            (
                "comment with a quote",
                "pip install a # don't",
                Some(&["pip install a"]),
            ),
            (
                "hash inside a word",
                "pip install a#b",
                Some(&["pip install a#b"]),
            ),
            // A command inside `if` or a loop counts.
            (
                "if",
                "if pip install a; then pip install b; else pip install c; fi",
                Some(&["pip install a", "pip install b", "pip install c", "fi"]),
            ),
            (
                "if, negated",
                "if ! pip install a; then exit 1; fi",
                Some(&["pip install a", "exit 1", "fi"]),
            ),
            (
                "for",
                "for p in x y; do pip install a; done",
                Some(&["for p in x y", "pip install a", "done"]),
            ),
            (
                "while",
                "while ! pip install a; do sleep 1; done",
                Some(&["pip install a", "sleep 1", "done"]),
            ),
            (
                "until, on lines",
                "until pip install a\ndo\n  sleep 1\ndone",
                Some(&["pip install a", "sleep 1", "done"]),
            ),
            (
                "braces",
                "{ pip install a; }",
                Some(&["pip install a", "}"]),
            ),
            (
                "subshell",
                "(cd x && pip install a) && pip install b",
                Some(&["cd x", "pip install a", "pip install b"]),
            ),
            (
                "function",
                "f() {\n  pip install a\n}",
                Some(&["f", "pip install a", "}"]),
            ),
            (
                "function on a line",
                "f() { pip install a; }",
                Some(&["f", "}"]),
            ),
            (
                "case",
                "case $1 in\n  a) pip install a ;;\n  *)\n    pip install b\n    ;;\nesac",
                Some(&["case <$1> in", "a", "*", "pip install b", "esac"]),
            ),
            // A reserved word opens no command of its own only when it is written bare.
            (
                "reserved word, quoted or escaped",
                "'if' pip install a\n\\if pip install b\n\"then\" pip install c",
                Some(&["if pip install a", "if pip install b", "then pip install c"]),
            ),
            (
                "reserved word a variable gives",
                "$KW pip install a",
                Some(&["then pip install a"]),
            ),
            (
                "reserved word in a subshell",
                "if (pip install a); then pip install b; fi",
                Some(&["pip install a", "pip install b", "fi"]),
            ),
            // A parenthesis that opens no command is passed over to the one that closes it.
            (
                "array over lines",
                "CMD=(\npip install a\n)\npip install b",
                Some(&["CMD=", "pip install b"]),
            ),
            (
                "arithmetic command",
                "(( pip install a ))\npip install b",
                Some(&["pip install b"]),
            ),
            (
                "arithmetic condition",
                "if ((x > 1)); then pip install a; fi",
                Some(&["pip install a", "fi"]),
            ),
            ("arithmetic left open", "(( pip install a", None),
            ("subshell left open", "( pip install a", None),
            ("array left open", "CMD=(\npip install a", None),
            // Word: a literal, a quoted literal.
            (
                "quoted",
                "pip install 'a>=1' \"b<2\" c\\>3",
                Some(&["pip install a>=1 b<2 c>3"]),
            ),
            (
                "joined quotes",
                "pip install a'-b'\"-c\"",
                Some(&["pip install a-b-c"]),
            ),
            ("empty word", "pip install ''", Some(&["pip install "])),
            (
                "escape in a string",
                "echo \"a\\\"b\\$c\\d\"",
                Some(&["echo a\"b$c\\d"]),
            ),
            ("redirection", "pip install a>=1", Some(&["pip install a"])),
            (
                "file descriptor",
                "pip install a 2>&1",
                Some(&["pip install a"]),
            ),
            ("input", "pip install a < answers", Some(&["pip install a"])),
            ("quote left open", "pip install a\necho don't", None),
            ("string left open", "pip install \"a", None),
            // An expression or a variable a literal `env` gives.
            (
                "expression",
                "pip install ${{ env.PKG }} ${{env.PKG}}",
                Some(&["pip install acme-lib acme-lib"]),
            ),
            (
                "expression in quotes",
                "pip install '${{ env.PKG }}' \"${{ env.PKG }}==1\"",
                Some(&["pip install acme-lib acme-lib==1"]),
            ),
            (
                "variable",
                "pip install $PKG ${PKG} \"$PKG\" \"${PKG}==1\"",
                Some(&["pip install acme-lib acme-lib acme-lib acme-lib==1"]),
            ),
            (
                "single quotes keep a variable",
                "pip install '$PKG'",
                Some(&["pip install $PKG"]),
            ),
            (
                "escaped variable",
                "pip install \\$PKG \"\\$PKG\"",
                Some(&["pip install $PKG $PKG"]),
            ),
            ("a lone dollar", "echo $ a$", Some(&["echo $ a$"])),
            (
                "literals longer than the script",
                "p $PKG $PKG",
                Some(&["p acme-lib acme-lib"]),
            ),
            // Unresolved, each form.
            (
                "secrets",
                "pip install ${{ secrets.X }}",
                Some(&["pip install <${{ secrets.X }}>"]),
            ),
            (
                "vars",
                "pip install ${{ vars.X }}",
                Some(&["pip install <${{ vars.X }}>"]),
            ),
            (
                "github",
                "pip install ${{ github.ref_name }}",
                Some(&["pip install <${{ github.ref_name }}>"]),
            ),
            (
                "matrix",
                "pip install ${{ matrix.package }}",
                Some(&["pip install <${{ matrix.package }}>"]),
            ),
            (
                "no literal env",
                "pip install $OTHER ${OTHER} ${{ env.OTHER }}",
                Some(&["pip install <$OTHER> <${OTHER}> <${{ env.OTHER }}>"]),
            ),
            (
                "substitution",
                "pip install $(cat name) \"$(cat 'a)' b)\"",
                Some(&["pip install <$(cat name)> <$(cat 'a)' b)>"]),
            ),
            (
                "backticks",
                "pip install `cat name`",
                Some(&["pip install <`cat name`>"]),
            ),
            (
                "part of a name",
                "pip install acme-${{ vars.X }}-suite \"acme-$OTHER\"",
                Some(&["pip install acme-<${{ vars.X }}>-suite acme-<$OTHER>"]),
            ),
            (
                "default",
                "pip install ${PKG:-b}",
                Some(&["pip install <${PKG:-b}>"]),
            ),
            (
                "positional",
                "pip install $1 $@",
                Some(&["pip install <$1> <$@>"]),
            ),
            (
                "ansi string",
                "pip install $'a\\'b'",
                Some(&["pip install <$'a\\'b'>"]),
            ),
            (
                "arithmetic",
                "echo $((1 + (2)))",
                Some(&["echo <$((1 + (2)))>"]),
            ),
            (
                "a value the shell would split",
                "pip install $SPACED $EMPTY",
                Some(&["pip install <$SPACED> <$EMPTY>"]),
            ),
            ("expression left open", "pip install ${{ env.PKG", None),
            ("substitution left open", "pip install $(cat name", None),
            // No shell is evaluated: a name the script rebinds keeps no literal.
            (
                "assignment",
                "PKG=b\npip install $PKG",
                Some(&["PKG=b", "pip install <$PKG>"]),
            ),
            (
                "export",
                "export PKG\npip install ${{ env.PKG }}",
                Some(&["export PKG", "pip install <${{ env.PKG }}>"]),
            ),
            (
                "loop variable",
                "for PKG in a b; do pip install $PKG; done",
                Some(&["for PKG in a b", "pip install <$PKG>", "done"]),
            ),
            (
                "default assigned",
                "pip install ${PKG:=b} $PKG",
                Some(&["pip install <${PKG:=b}> <$PKG>"]),
            ),
            (
                "written to the environment file",
                "echo \"PKG=b\" >> $GITHUB_ENV\npip install $PKG",
                Some(&["echo PKG=b", "pip install <$PKG>"]),
            ),
            (
                "another name",
                "PKGS=b\nMY_PKG=c\npip install $PKG",
                Some(&["PKGS=b", "MY_PKG=c", "pip install acme-lib"]),
            ),
        ];
        for (rule, script, expected) in cases {
            let expected = expected.map(|lines| lines.iter().map(|l| l.to_string()).collect());
            assert_eq!(read(script), expected, "{rule}: {script:?}");
        }
    }

    #[test]
    fn a_script_of_operators_alone_is_read_in_one_pass() {
        // One pass over the bytes: no rule looks back further than the word it is in, and a
        // parenthesis left open ends the reading at once.
        let script = "$(".repeat(400) + &")".repeat(400) + &"();|&\n".repeat(200);
        assert_eq!(read(&script).map(|c| c.len()), Some(1));
        assert_eq!(read(&"\\".repeat(1001)), Some(vec!["\\".repeat(500)]));
        assert_eq!(read(&("(".repeat(1000) + " pip install a")), None);
        assert_eq!(read(&"x(".repeat(1000)), None);
        // The literals a script reads add at most its own length and one value's: the rest
        // stays open.
        let read = read(&"$PKG ".repeat(400)).unwrap();
        assert!(read[0].starts_with("acme-lib acme-lib") && read[0].ends_with("<$PKG> <$PKG>"));
        assert!(read[0].len() <= 2 * 2000 + VALUE_MAX, "{}", read[0].len());
    }
}
