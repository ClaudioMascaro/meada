//! A line-oriented scanner for Gradle's literal assignments and dependency coordinates.
//! Tokens only separate strings, comments and block boundaries; no build grammar is loaded.

use std::collections::{BTreeMap, BTreeSet};

use crate::package::{Entry, Summary};

// ponytail: build scripts nest a few blocks; a deeper file reads as nothing.
const GRADLE_DEPTH: usize = 64;

// ponytail: a project names a few source directories; the rest of a hostile file is dropped.
const MAX_ROOTS: usize = 64;

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
                // Kotlin block comments nest; read so for Groovy too, where a nested opener is
                // rare and only turns a coordinate into a miss, never into a wrong `exact`.
                let mut depth = 1usize;
                while let Some(c) = chars.next() {
                    match (c, chars.peek()) {
                        ('*', Some('/')) => {
                            chars.next();
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        ('/', Some('*')) => {
                            chars.next();
                            depth += 1;
                        }
                        _ => {}
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

/// The head of a statement as a dotted path, with an index by a literal name read as a member
/// (`sourceSets["main"].java.srcDirs` is `sourceSets.main.java.srcDirs`), and what follows it.
fn dotted(tokens: &[Token]) -> Option<(String, &[Token])> {
    let head = tokens.first()?.word()?;
    match &tokens[1..] {
        [
            Token::Mark('['),
            Token::String(set, false),
            Token::Mark(']'),
            Token::Word(tail),
            rest @ ..,
        ] if super::literal(set) && tail.starts_with('.') => {
            Some((format!("{head}.{set}{tail}"), rest))
        }
        rest => Some((head.to_string(), rest)),
    }
}

/// The literal directories a statement adds to a source set of `sourceSets`, with whether the
/// set is `main`: `srcDirs = ['a']`, `srcDirs += 'a'`, `srcDir 'a'`, `srcDirs("a", "b")`, inside
/// `sourceSets { main { java { ... } } }` or written with the dotted path, for the `java` or the
/// `kotlin` directories alike. A directory that is not a literal relative path (`file(..)`,
/// `..`, a variable) is not read.
fn source_dirs(tokens: &[Token], blocks: &[String]) -> Option<(bool, Vec<String>)> {
    let (head, rest) = dotted(tokens)?;
    let path: Vec<&str> = (blocks.iter().map(String::as_str))
        .chain([head.as_str()])
        .flat_map(|s| s.split('.'))
        .collect();
    let at = path.iter().position(|s| *s == "sourceSets")?;
    let [set, "java" | "kotlin", "srcDirs" | "srcDir"] = path[at + 1..] else {
        return None;
    };
    // `named(set)` or `getByName(set)` with no literal name (`block_head`) is no known set.
    if matches!(set, "named" | "getByName") {
        return None;
    }
    let mut rest = rest;
    if let [Token::Mark('+'), tail @ ..] = rest {
        rest = tail;
    }
    if let [Token::Mark('='), tail @ ..] = rest {
        rest = tail;
    }
    if let [Token::Mark('['), inner @ .., Token::Mark(']')] = rest {
        rest = inner;
    }
    let relative = |dir: &str| {
        let dir = dir.strip_suffix('/').unwrap_or(dir);
        !dir.is_empty()
            && !dir.contains('\\')
            && dir.split('/').all(|s| !matches!(s, "" | "." | ".."))
    };
    let dirs = (arguments(rest).into_iter())
        .filter_map(|arg| match arg {
            [Token::String(s, false)] if relative(s) => {
                Some(s.strip_suffix('/').unwrap_or(s).to_string())
            }
            _ => None,
        })
        .collect();
    Some((set == "main", dirs))
}

/// The name a block's head line gives it: its first word, with a `named(..)` or `getByName(..)`
/// of a literal set read as that set (`sourceSets.named("main")` is `sourceSets.main`).
fn block_head(line: &[Token]) -> String {
    if let Some((head, [])) = dotted(line) {
        return head;
    }
    let head = line.iter().find_map(Token::word).unwrap_or("");
    if let [
        Token::Word(_),
        Token::Mark('('),
        Token::String(set, false),
        Token::Mark(')'),
    ] = line
    {
        let (before, last) = head.rsplit_once('.').unwrap_or(("", head));
        if matches!(last, "named" | "getByName") && super::literal(set) {
            return match before {
                "" => set.clone(),
                before => format!("{before}.{set}"),
            };
        }
    }
    head.to_string()
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
    /// The literal source directories of the source sets, with whether the set is `main`.
    sources: Vec<(bool, String)>,
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
    // Like the coordinates, a source directory counts only where it is certain to apply here:
    // never in a block that configures other projects (`allprojects`, `subprojects` and their
    // `.each`, `project(':x')`, `configure(..)`).
    let shared = blocks.iter().any(|b| {
        let head = b.split('.').next().unwrap_or(b);
        matches!(
            head,
            "allprojects" | "subprojects" | "project" | "configure"
        )
    });
    if !conditional
        && !shared
        && let Some((main, dirs)) = source_dirs(tokens, blocks)
    {
        scan.sources.extend(dirs.into_iter().map(|d| (main, d)));
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
                let head = block_head(&line);
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
        sources,
    } = scan;
    let settings = path
        .rsplit('/')
        .next()
        .is_some_and(|p| p.starts_with("settings."));
    if !settings {
        // Each directory once, a test one when any set other than `main` lists it; the defaults
        // stay past the limit, and the test roots are among the kept ones.
        let mut dirs: BTreeMap<String, bool> = BTreeMap::new();
        for (main, dir) in sources {
            *dirs.entry(dir).or_insert(true) &= main;
        }
        let mut roots: Vec<String> = super::ROOTS.map(Into::into).into();
        let mut tests: Vec<String> = super::TEST_ROOTS.map(Into::into).into();
        for (dir, main) in dirs.into_iter().take(MAX_ROOTS - roots.len()) {
            if !main {
                tests.push(dir.clone());
            }
            roots.push(dir);
        }
        for list in [&mut roots, &mut tests] {
            list.sort();
            list.dedup();
        }
        (summary.roots, summary.test_roots) = (roots, tests);
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
    fn block_comments_nest() {
        let found = tokens("a /* b /* c */ d */ e /* f */ g /*/ h */ i");
        let words: Vec<_> = found.iter().filter_map(Token::word).collect();
        assert_eq!(words, ["a", "e", "g", "i"]);
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

    fn roots(src: &str) -> (Vec<String>, Vec<String>) {
        let summary = read("build.gradle", src.as_bytes());
        (summary.roots, summary.test_roots)
    }

    #[test]
    fn a_literal_source_set_directory_adds_a_root_main_or_not() {
        let defaults = [
            "src/main/java",
            "src/main/kotlin",
            "src/test/java",
            "src/test/kotlin",
        ];
        let test_roots = |extra: &[&str]| -> Vec<String> {
            let mut all: Vec<String> = ["src/test/java", "src/test/kotlin"]
                .iter()
                .chain(extra)
                .map(|s| s.to_string())
                .collect();
            all.sort();
            all
        };
        let with = |extra: &[&str]| -> Vec<String> {
            let mut all: Vec<String> = defaults
                .iter()
                .chain(extra)
                .map(|s| s.to_string())
                .collect();
            all.sort();
            all
        };
        assert_eq!(roots(""), (with(&[]), test_roots(&[])));
        // Groovy and Kotlin forms, nested or dotted, replacing or adding.
        for (src, main, test) in [
            (
                "sourceSets { main { java { srcDirs = ['src/a', \"src/b/\"] } } }",
                &["src/a", "src/b"][..],
                &[][..],
            ),
            (
                "sourceSets {\n main {\n java.srcDirs(\"src/a\")\n }\n test { java { srcDir 'src/t' } }\n}",
                &["src/a"],
                &["src/t"],
            ),
            (
                "sourceSets.main.java.srcDirs += 'src/a'\nsourceSets.integration.java.srcDirs = ['src/i']",
                &["src/a"],
                &["src/i"],
            ),
            // The Kotlin source directory set, Groovy and Kotlin DSL.
            (
                "sourceSets { main { kotlin { srcDirs = ['src/a'] } } }",
                &["src/a"],
                &[],
            ),
            (
                "sourceSets {\n main { kotlin.srcDir(\"src/a\") }\n named(\"test\") { kotlin.srcDirs(\"src/t\", \"src/u\") }\n}",
                &["src/a"],
                &["src/t", "src/u"],
            ),
            // An indexed set, in the dotted path or as a block's head.
            (
                "sourceSets[\"main\"].kotlin.srcDir(\"src/a\")\nsourceSets['it'].java.srcDirs += 'src/i'",
                &["src/a"],
                &["src/i"],
            ),
            (
                "kotlin { sourceSets[\"main\"].kotlin.srcDir(\"src/a\") }\nsourceSets {\n getByName(\"test\") { kotlin.srcDir(\"src/t\") }\n}",
                &["src/a"],
                &["src/t"],
            ),
        ] {
            let (all, tests) = roots(src);
            let expected: Vec<_> = main.iter().chain(test).copied().collect();
            assert_eq!(all, with(&expected), "{src}");
            assert_eq!(tests, test_roots(test), "{src}");
        }
    }

    #[test]
    fn a_source_directory_that_is_not_certain_adds_no_root() {
        for src in [
            "sourceSets { main { java { srcDirs = [dir] } } }",
            "sourceSets { main { java { srcDirs = [file('src/a')] } } }",
            "sourceSets { main { java { srcDirs = [\"$dir/a\"] } } }",
            "sourceSets { main { java { srcDirs = ['../a', '/abs', 'a/../b', './c', ''] } } }",
            "if (flag) { sourceSets { main { java { srcDirs = ['src/a'] } } } }",
            "allprojects { sourceSets { main { java { srcDirs = ['src/a'] } } } }",
            "subprojects { sourceSets.main.java.srcDir 'src/a' }",
            "sourceSets { main { resources { srcDirs = ['src/a'] } } }",
            "project(':app') { sourceSets { main { java { srcDirs = ['src/a'] } } } }",
            "configure(subprojects) { sourceSets.main.java.srcDir 'src/a' }",
            "subprojects.each { sourceSets.main.java.srcDir 'src/a' }",
            "allprojects.forEach { sourceSets.main.java.srcDir 'src/a' }",
            "sourceSets.named(set) { java.srcDir 'src/a' }",
            "sourceSets[set].kotlin.srcDir(\"src/a\")",
            "sourceSets[\"main\"].kotlin.srcDir(dir)",
            "sourceSets { main { kotlin { exclude(\"src/a\") } } }",
            "other { main { java { srcDirs = ['src/a'] } } }",
        ] {
            assert_eq!(
                roots(src),
                (
                    vec![
                        "src/main/java".to_string(),
                        "src/main/kotlin".to_string(),
                        "src/test/java".to_string(),
                        "src/test/kotlin".to_string()
                    ],
                    vec!["src/test/java".to_string(), "src/test/kotlin".to_string()]
                ),
                "{src}"
            );
        }
    }

    #[test]
    fn source_directories_past_the_limit_are_dropped() {
        let dirs: Vec<_> = (0..MAX_ROOTS + 10).map(|i| format!("'d{i:03}'")).collect();
        let src = format!(
            "sourceSets {{ main {{ java {{ srcDirs = [{}] }} }} }}",
            dirs.join(", ")
        );
        assert_eq!(roots(&src).0.len(), MAX_ROOTS);
        // The defaults stay, and every test root is a kept root.
        let (all, tests) = roots(&src);
        assert!(all.iter().any(|r| r == "src/main/kotlin"), "{all:?}");
        assert!(tests.iter().all(|t| all.contains(t)), "{tests:?}");
        let src = format!(
            "sourceSets {{ main {{ java {{ srcDirs = [{}] }} }}\n test {{ java {{ srcDirs = [{}] }} }} }}",
            dirs.join(", "),
            dirs.join(", ").replace("'d", "'t"),
        );
        let (all, tests) = roots(&src);
        assert_eq!(all.len(), MAX_ROOTS);
        assert!(tests.iter().all(|t| all.contains(t)), "{tests:?}");
    }

    #[test]
    fn a_source_set_named_by_a_literal_is_that_set() {
        for src in [
            "sourceSets.named('main') { java.srcDir 'src/a' }",
            "sourceSets { getByName(\"main\") { java.srcDir(\"src/a\") } }",
            "sourceSets[\"main\"].kotlin { srcDir(\"src/a\") }",
        ] {
            let (all, tests) = roots(src);
            assert!(all.iter().any(|r| r == "src/a"), "{src}");
            assert_eq!(tests, ["src/test/java", "src/test/kotlin"], "{src}");
        }
        let (all, tests) = roots("sourceSets.named('it') { java.srcDir 'src/i' }");
        assert!(all.iter().any(|r| r == "src/i") && tests.iter().any(|t| t == "src/i"));
    }
}
