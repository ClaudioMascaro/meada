//! A line-oriented scanner for Gradle's literal assignments and dependency coordinates.
//! Tokens only separate strings, comments and block boundaries; no build grammar is loaded.

use std::collections::BTreeSet;

use crate::package::{Entry, Summary};

// ponytail: build scripts nest a few blocks; a deeper file reads as nothing.
const GRADLE_DEPTH: usize = 64;

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    String(String, bool),
    Mark(char),
    Unread,
}

impl Token {
    fn word(&self) -> Option<&str> {
        match self {
            Self::Word(s) => Some(s),
            _ => None,
        }
    }
}

/// Scans quoted strings before comments, so a comment or string never supplies build code.
fn tokens(src: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push(Token::Mark('\n'));
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
            }
            '\'' | '"' => {
                let mut value = String::new();
                let mut computed = false;
                let mut closed = false;
                let mut valid = true;
                let mut interpolation = 0usize;
                while let Some(at) = chars.next() {
                    if at == c && interpolation == 0 {
                        closed = true;
                        break;
                    }
                    if c == '"' {
                        if at == '{' && (interpolation > 0 || value.ends_with('$')) {
                            interpolation += 1;
                        }
                        if at == '}' {
                            interpolation = interpolation.saturating_sub(1);
                        }
                    }
                    if at == '\\' {
                        match chars.next() {
                            Some('n') => value.push('\n'),
                            Some('r') => value.push('\r'),
                            Some('t') => value.push('\t'),
                            Some(escaped @ ('\\' | '\'' | '"' | '$')) => value.push(escaped),
                            Some('u') => {
                                let digits: String = chars.by_ref().take(4).collect();
                                match (digits.len() == 4)
                                    .then(|| u32::from_str_radix(&digits, 16).ok())
                                    .flatten()
                                    .and_then(char::from_u32)
                                {
                                    Some(c) => value.push(c),
                                    None => valid = false,
                                }
                            }
                            Some(_) => valid = false,
                            None => break,
                        }
                    } else {
                        computed |= c == '"' && at == '$';
                        value.push(at);
                    }
                }
                if !closed {
                    break;
                }
                out.push(match valid {
                    true => Token::String(value, computed),
                    false => Token::Unread,
                });
            }
            c if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$') => {
                let mut word = String::from(c);
                while chars
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
                {
                    word.push(chars.next().unwrap());
                }
                out.push(Token::Word(word));
            }
            c if c.is_whitespace() && c != '\n' => {}
            c => out.push(Token::Mark(c)),
        }
    }
    out
}

fn assigned<'a>(tokens: &'a [Token], name: &str) -> Option<Option<&'a str>> {
    if tokens.first()?.word() != Some(name) || !matches!(tokens.get(1), Some(Token::Mark('='))) {
        return None;
    }
    Some(match &tokens[2..] {
        [Token::String(s, false)] => Some(s),
        _ => None,
    })
}

fn configuration(name: &str) -> bool {
    matches!(
        name,
        "implementation" | "api" | "compileOnly" | "runtimeOnly"
    ) || name.starts_with("test")
        && ["Implementation", "Api", "CompileOnly", "RuntimeOnly"]
            .iter()
            .any(|s| name.ends_with(s))
}

/// A value's text, with `*` for each part that is no string: `"g:a:" + v` is `g:a:*`. A string
/// holding `*`, or a `$` that is no interpolation, is no coordinate.
fn text(tokens: &[Token]) -> Option<String> {
    let mut text = String::new();
    for token in tokens {
        match token {
            Token::String(s, dynamic) => {
                if s.contains('*') || !dynamic && s.contains('$') {
                    return None;
                }
                text.push_str(s);
            }
            Token::Mark('(' | ')' | '+') => {}
            _ => text.push('*'),
        }
    }
    Some(text)
}

/// A group or artifact as a literal, or as the pattern of its literal fragments (`true`).
fn piece(part: &str) -> Option<(String, bool)> {
    match super::literal(part) {
        true => Some((part.to_string(), false)),
        false => super::pattern(part).map(|p| (p, true)),
    }
}

/// The parts of `"group:artifact[:version]"`, split at the colons outside `${...}`.
fn positional(text: &str) -> Option<(&str, &str, Option<&str>)> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut previous = None;
    for (at, c) in text.char_indices() {
        match c {
            '{' if depth > 0 || previous == Some('$') => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                parts.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
        previous = Some(c);
    }
    parts.push(&text[start..]);
    match parts.as_slice() {
        [g, a] => Some((g, a, None)),
        [g, a, v] => Some((g, a, Some(v))),
        _ => None,
    }
}

/// The `group`, `name` and `version` of map notation (`group: 'g', name: 'a'` or Kotlin's
/// `group = "g", name = "a"`); `None` when the arguments are not named.
fn named(args: &[Token]) -> Option<Option<(String, String, Option<String>)>> {
    let args = arguments(args);
    let key = |arg: &[Token]| match arg {
        [Token::Word(k), Token::Mark(':' | '='), ..] => Some(k.clone()),
        _ => None,
    };
    key(args.first()?)?;
    let mut found: [Option<String>; 3] = Default::default();
    for arg in args {
        let Some(k) = key(arg) else {
            return Some(None);
        };
        let at = match k.as_str() {
            "group" => 0,
            "name" => 1,
            "version" => 2,
            _ => continue,
        };
        let Some(value) = text(&arg[2..]) else {
            return Some(None);
        };
        found[at] = Some(value);
    }
    let [group, artifact, version] = found;
    Some(group.zip(artifact).map(|(g, a)| (g, a, version)))
}

fn dependency(field: &str, args: &[Token]) -> Option<Entry> {
    // A call (`project(...)`, `files(...)`, `platform(...)`) or no string at all
    // (`libs.models`) declares no `group:artifact` coordinate.
    let call = args
        .windows(2)
        .any(|w| matches!(w, [Token::Word(_), Token::Mark('(')]));
    if call
        || !args.iter().any(|t| matches!(t, Token::String(..)))
        || args.iter().any(|t| matches!(t, Token::Unread))
    {
        return None;
    }
    let (group, artifact, range) = match named(args) {
        Some(coordinate) => coordinate?,
        None => {
            let text = text(args)?;
            let (g, a, v) = positional(&text)?;
            (g.to_string(), a.to_string(), v.map(str::to_string))
        }
    };
    let (group, group_computed) = piece(&group)?;
    let (artifact, artifact_computed) = piece(&artifact)?;
    // A coordinate without one literal fragment fits every provider: it names none.
    if group == "*" && artifact == "*" {
        return None;
    }
    let name = format!("{group}:{artifact}");
    if name.len() > crate::package::MAX_NAME {
        return None;
    }
    // A computed version keeps the coordinate literal: the range is kept as written, a part
    // that is no string leaves none.
    let range = range.filter(|v| !v.contains('*'));
    Some(Entry {
        field: field.into(),
        name,
        range,
        via: (group_computed || artifact_computed).then(|| "computed".into()),
        ..Entry::default()
    })
}

fn control(name: &str) -> bool {
    matches!(
        name,
        "if" | "else" | "for" | "while" | "when" | "try" | "catch"
    )
}

fn assigns(tokens: &[Token], name: &str) -> bool {
    let tokens = if tokens.first().and_then(Token::word).is_some_and(control) {
        tokens
    } else {
        &tokens[..tokens.len().min(3)]
    };
    tokens.iter().enumerate().any(|(i, token)| {
        token.word() == Some(name)
            && (matches!(tokens.get(i + 1), Some(Token::Mark('=')))
                || matches!(
                    (tokens.get(i + 1), tokens.get(i + 2)),
                    (
                        Some(Token::Mark('+' | '-' | '*' | '/')),
                        Some(Token::Mark('='))
                    )
                ))
    })
}

/// The arguments of a call, `f(a, b)` or `f a, b`, split at the commas outside brackets.
fn arguments(args: &[Token]) -> Vec<&[Token]> {
    let args = match args {
        [Token::Mark('('), inner @ .., Token::Mark(')')] => inner,
        _ => args,
    };
    let mut depth = 0usize;
    args.split(|t| {
        match t {
            Token::Mark('(' | '[') => depth += 1,
            Token::Mark(')' | ']') => depth = depth.saturating_sub(1),
            _ => {}
        }
        depth == 0 && matches!(t, Token::Mark(','))
    })
    .collect()
}

/// The arguments of `include` that are a lone literal string, in `include(":a", ":b")` or
/// `include ':a', ':b'`; an argument that computes its value (a call, a concatenation) names
/// no project.
fn included(args: &[Token]) -> Vec<&str> {
    (arguments(args).into_iter())
        .filter_map(|arg| match arg {
            [Token::String(s, false)] => Some(s.as_str()),
            _ => None,
        })
        .collect()
}

/// What a scan has read so far.
#[derive(Default)]
struct Scan {
    build: super::Build,
    summary: Summary,
    /// The included project paths a `project(':p').name` or `.projectDir` assignment changes.
    moved: BTreeSet<String>,
    /// Some assignment changes a project's name or directory without saying which.
    unplaced: bool,
}

fn statement(tokens: &[Token], blocks: &[String], controlled: bool, scan: &mut Scan) {
    let Scan { build, summary, .. } = scan;
    if blocks.is_empty() {
        if let Some(group) = assigned(tokens, "group") {
            build.group = group.filter(|s| super::literal(s)).map(str::to_string);
        }
        if let Some(version) = assigned(tokens, "version") {
            summary.version = version.map(str::to_string);
        }
        if let Some(project) = assigned(tokens, "rootProject.name") {
            build.project = project.filter(|s| super::literal(s)).map(str::to_string);
        }
        if tokens.first().and_then(Token::word) == Some("include") {
            for project in included(&tokens[1..]) {
                let project = project.strip_prefix(':').unwrap_or(project);
                if project
                    .split(':')
                    .all(|s| super::literal(s) && !matches!(s, "." | ".."))
                {
                    build.includes.push((
                        project.rsplit(':').next().unwrap().into(),
                        project.replace(':', "/"),
                    ));
                }
            }
        }
    }
    // `allprojects` configures this project too, so its assignments are never literal here.
    let conditional = controlled
        || blocks.iter().any(|b| b == "allprojects")
        || tokens.first().and_then(Token::word).is_some_and(control);
    for (name, value) in [
        ("group", &mut build.group),
        ("rootProject.name", &mut build.project),
        ("version", &mut summary.version),
    ] {
        if assigns(tokens, name)
            && (conditional || blocks.is_empty() && assigned(tokens, name).is_none())
        {
            *value = None;
        }
    }
    let member = |t: &Token| {
        t.word().is_some_and(|w| {
            w != "rootProject.name" && (w.ends_with(".name") || w.ends_with(".projectDir"))
        })
    };
    if let Some(at) =
        (tokens.windows(2)).position(|w| member(&w[0]) && matches!(w[1], Token::Mark('=')))
    {
        match &tokens[..at] {
            [
                Token::Word(call),
                Token::Mark('('),
                Token::String(path, false),
                Token::Mark(')'),
            ] if call == "project" => {
                (scan.moved).insert(path.strip_prefix(':').unwrap_or(path).replace(':', "/"));
            }
            _ => scan.unplaced = true,
        }
    }
    if blocks.len() == 1
        && blocks[0] == "dependencies"
        && let Some(kind) = tokens
            .first()
            .and_then(Token::word)
            .filter(|s| configuration(s))
        && let Some(entry) = dependency(kind, &tokens[1..])
    {
        scan.summary.dependencies.push(entry);
    }
}

// ponytail: Groovy slashy strings (`/a/`, `$/a/$`) are not read as strings, and a triple-quoted
// string reads as quoted pairs; text inside them may be scanned as code when its quotes do not
// pair up.
pub fn read(path: &str, src: &[u8]) -> Summary {
    if src.len() > crate::lang::MAX_BYTES {
        return Summary::default();
    }
    let Ok(text) = std::str::from_utf8(src) else {
        return Summary::default();
    };
    let mut scan = Scan::default();
    let mut blocks: Vec<String> = Vec::new();
    let mut controls = 0usize;
    let mut line = Vec::new();
    let mut enclosed = 0usize;
    for token in tokens(text) {
        match token {
            Token::Mark('{') => {
                // The block's head is a statement too: `implementation("g:a:1") { ... }`.
                statement(&line, &blocks, controls > 0, &mut scan);
                if blocks.len() == GRADLE_DEPTH {
                    return Summary::default();
                }
                let head = line.iter().find_map(Token::word).unwrap_or("").to_string();
                controls += usize::from(control(&head));
                blocks.push(head);
                line.clear();
            }
            Token::Mark('}') => {
                statement(&line, &blocks, controls > 0, &mut scan);
                line.clear();
                if blocks.pop().is_some_and(|b| control(&b)) {
                    controls -= 1;
                }
            }
            Token::Mark('\n') if enclosed > 0 => {}
            Token::Mark('\n' | ';') if enclosed == 0 => {
                statement(&line, &blocks, controls > 0, &mut scan);
                line.clear();
            }
            Token::Mark('(' | '[') => {
                enclosed += 1;
                line.push(token);
            }
            Token::Mark(')' | ']') => {
                enclosed = enclosed.saturating_sub(1);
                line.push(token);
            }
            _ => line.push(token),
        }
    }
    statement(&line, &blocks, controls > 0, &mut scan);
    let Scan {
        mut build,
        mut summary,
        moved,
        unplaced,
    } = scan;
    let settings = path
        .rsplit('/')
        .next()
        .is_some_and(|p| p.starts_with("settings."));
    if !settings {
        summary.roots = vec!["src/main/java".into(), "src/test/java".into()];
    }
    // A renamed or moved project is not where, or not what, `include` says.
    build
        .includes
        .retain(|(_, path)| !unplaced && !moved.contains(path));
    build.includes.sort();
    build.includes.dedup();
    summary.build = Some(build);
    summary
        .dependencies
        .sort_by(|a, b| (&a.field, &a.name, &a.range).cmp(&(&b.field, &b.name, &b.range)));
    summary.dependencies.dedup();
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(path: &str, src: &str) -> super::super::Build {
        read(path, src.as_bytes()).build.unwrap_or_default()
    }

    #[test]
    fn tokens_keep_strings_and_drop_comments() {
        let found = tokens("a.b = 'x' // 'y'\n\"g:$v\" /* z */ 'q\\u0041' 'bad\\s' (");
        assert_eq!(
            found,
            [
                Token::Word("a.b".into()),
                Token::Mark('='),
                Token::String("x".into(), false),
                Token::Mark('\n'),
                Token::String("g:$v".into(), true),
                Token::String("qA".into(), false),
                Token::Unread,
                Token::Mark('('),
            ]
        );
        // An unclosed string ends the scan.
        assert_eq!(tokens("a 'open"), [Token::Word("a".into())]);
    }

    #[test]
    fn blocks_nested_past_the_limit_read_as_nothing() {
        let nested = |n: usize| format!("group = 'example'\n{}{}", "a {".repeat(n), "}".repeat(n));
        assert_eq!(
            build("build.gradle", &nested(GRADLE_DEPTH))
                .group
                .as_deref(),
            Some("example")
        );
        assert_eq!(
            read("build.gradle", nested(GRADLE_DEPTH + 1).as_bytes()),
            Summary::default()
        );
    }

    #[test]
    fn allprojects_leaves_the_group_unknown_until_assigned_again() {
        let src = "group = 'example'\nallprojects {\n    group = 'other'\n}\n";
        assert_eq!(build("build.gradle", src).group, None);
        let src = format!("{src}group = 'example'\n");
        assert_eq!(
            build("build.gradle", &src).group.as_deref(),
            Some("example")
        );
    }

    #[test]
    fn a_renamed_or_moved_project_names_no_artifact() {
        let settings = "rootProject.name = 'kit'\ninclude ':tools', ':app'\n";
        let names = |extra: &str| -> Vec<String> {
            (build("settings.gradle", &format!("{settings}{extra}"))
                .includes
                .into_iter())
            .map(|(name, _)| name)
            .collect()
        };
        assert_eq!(names(""), ["app", "tools"]);
        assert_eq!(names("project(':app').name = 'svc'\n"), ["tools"]);
        assert_eq!(names("project(':app').projectDir = file('x')\n"), ["tools"]);
        assert!(names("rootProject.children.each { it.name = 'x' }\n").is_empty());
        assert!(names("findProject(':app')?.name = 'svc'\n").is_empty());
    }
}
