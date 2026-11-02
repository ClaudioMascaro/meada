//! A line-oriented scanner for Gradle's literal assignments and dependency coordinates.
//! Tokens only separate strings, comments and block boundaries; no build grammar is loaded.

use std::collections::{BTreeMap, BTreeSet};

use crate::lang::MAX_BYTES;
use crate::package::{Entry, MAX_NAME, Summary};

// ponytail: build scripts nest a few blocks; a deeper file reads as nothing.
const GRADLE_DEPTH: usize = 64;

// ponytail: a project names a few source directories; the rest of a hostile file is dropped.
const MAX_ROOTS: usize = 64;

// ponytail: a repository names a few thousand properties or catalog aliases; the rest of a
// hostile `gradle.properties` or version catalog is dropped.
const MAX_ENTRIES: usize = 8192;

// ponytail: a settings file names a few catalogs; the rest of a hostile one is dropped.
const MAX_CATALOGS: usize = 16;

#[derive(Clone, Debug, PartialEq)]
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

/// A template with each `$name` or `${name}` that `literals` gives a value replaced by it. A bare
/// `$name.rest` stays: Groovy reads it as a property of `name`, Kotlin as `name` and `.rest`.
fn filled(value: &str, literals: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (name, used) = match after.strip_prefix('{') {
            Some(braced) => braced
                .split_once('}')
                .map_or(("", 0), |(name, _)| (name, name.len() + 2)),
            None => {
                let len = after
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(after.len());
                let dotted = after[len..].starts_with('.');
                (if dotted { "" } else { &after[..len] }, len)
            }
        };
        match literals.get(name) {
            Some(literal) => {
                out.push_str(literal);
                rest = &after[used..];
            }
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `value` with each bare `$name` written `${name}`, the form `jvm::maven::fill` reads; a
/// `$name.rest` stays, as `filled` leaves it.
pub fn braced(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let len = after
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(after.len());
        if len > 0 && !after[len..].starts_with('.') {
            out.push_str(&format!("${{{}}}", &after[..len]));
            rest = &after[len..];
        } else {
            out.push('$');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// The names of the `${..}` holes of `value`.
fn holes(value: &str) -> impl Iterator<Item = &str> {
    value
        .split("${")
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
}

/// The `via` of a dependency written as a catalog accessor, `libs.models`, until
/// `Maven::inherit` reads the catalog.
pub const CATALOG: &str = "catalog ";

fn dependency(
    field: &str,
    args: &[Token],
    literals: &BTreeMap<String, String>,
    declared: &BTreeSet<String>,
) -> Option<Entry> {
    // `libs.models` (or `libs.models` in parentheses): a name in a catalog, if one says so.
    let lone = match args {
        [Token::Mark('('), inner @ .., Token::Mark(')')] => inner,
        _ => args,
    };
    if let [Token::Word(accessor)] = lone
        && !accessor.contains('$')
        && accessor.len() <= MAX_NAME
        && accessor.split('.').count() >= 2
        && accessor.split('.').all(|s| !s.is_empty())
    {
        return Some(Entry {
            field: field.into(),
            name: "*:*".into(),
            via: Some(format!("{CATALOG}{accessor}")),
            ..Entry::default()
        });
    }
    // A call (`project(...)`, `files(...)`, `platform(...)`) or no string at all declares no
    // `group:artifact` coordinate.
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
    let (group, artifact) = (filled(&group, literals), filled(&artifact, literals));
    // What the pom's seam can fill later: holes in the `${..}` form, none of them a name the
    // script assigns (the script's value wins over `gradle.properties`, and is not a literal).
    let template = (braced(&group), braced(&artifact));
    let template = [&template.0, &template.1]
        .iter()
        .all(|v| {
            v.match_indices('$')
                .all(|(i, _)| v[i + 1..].starts_with('{'))
        })
        .then_some(template)
        .filter(|(g, a)| holes(g).chain(holes(a)).all(|n| !declared.contains(n)));
    let (group, group_computed) = piece(&group)?;
    let (artifact, artifact_computed) = piece(&artifact)?;
    // A coordinate without one literal fragment fits every provider: it names none, unless
    // `gradle.properties` may still fill it (`Maven::inherit` drops it when it does not).
    if group == "*" && artifact == "*" && template.is_none() {
        return None;
    }
    let name = format!("{group}:{artifact}");
    if name.len() > MAX_NAME {
        return None;
    }
    let computed = group_computed || artifact_computed;
    // A computed version keeps the coordinate literal: the range is kept as written, a part
    // that is no string leaves none.
    let range = range.filter(|v| !v.contains('*'));
    // The script's own literals fill the version too; one naming a name the script assigns
    // otherwise is unknown, and `gradle.properties` never overrides the script.
    let range = (range.map(|v| filled(&v, literals)))
        .filter(|v| holes(&braced(v)).all(|n| !declared.contains(n)));
    Some(Entry {
        field: field.into(),
        name,
        range,
        via: computed.then(|| "computed".into()),
        template: template.filter(|_| computed),
        ..Entry::default()
    })
}

fn control(name: &str) -> bool {
    matches!(
        name,
        "if" | "else" | "for" | "while" | "when" | "try" | "catch"
    )
}

/// The names a statement assigns: `name =`, `name +=`, `val name =`, `val name: T =`, among its
/// first tokens, or anywhere in a control statement.
fn targets(tokens: &[Token]) -> impl Iterator<Item = &str> {
    let typed = matches!(tokens.first().and_then(Token::word), Some("val" | "var"))
        && matches!(tokens.get(2), Some(Token::Mark(':')));
    let tokens = if tokens.first().and_then(Token::word).is_some_and(control) {
        tokens
    } else {
        &tokens[..tokens.len().min(3)]
    };
    (tokens.iter().enumerate())
        .filter(move |&(i, _)| {
            typed && i == 1
                || matches!(tokens.get(i + 1), Some(Token::Mark('=')))
                || matches!(
                    (tokens.get(i + 1), tokens.get(i + 2)),
                    (
                        Some(Token::Mark('+' | '-' | '*' | '/')),
                        Some(Token::Mark('='))
                    )
                )
        })
        .filter_map(|(_, token)| token.word())
}

fn assigns(tokens: &[Token], name: &str) -> bool {
    targets(tokens).any(|target| target == name)
}

/// The name and the string of a statement that only gives a name a string with no interpolation:
/// `val name = "x"`, `def name = 'x'`, `name = 'x'`, `ext.name = 'x'`.
fn literal_assignment(tokens: &[Token]) -> Option<(&str, &str)> {
    let typed = tokens.first().and_then(Token::word);
    let tokens = match typed {
        Some("val" | "def") => &tokens[1..],
        _ => tokens,
    };
    // `val name: Type = "x"`
    if let [
        Token::Word(name),
        Token::Mark(':'),
        Token::Word(_),
        Token::Mark('='),
        Token::String(value, false),
    ] = tokens
    {
        return Some((name, value));
    }
    let target = tokens.first()?.word()?;
    let name = ["ext.", "project.ext."]
        .iter()
        .find_map(|prefix| target.strip_prefix(prefix))
        .unwrap_or(target);
    (!name.contains('.')).then_some(())?;
    Some((name, assigned(tokens, target)??))
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
        // `sourceSets.getByName("test").kotlin`: the set is the literal in the call.
        [
            Token::Mark('('),
            Token::String(set, false),
            Token::Mark(')'),
            Token::Word(tail),
            rest @ ..,
        ] if tail.starts_with('.') && super::literal(set) => {
            let (before, last) = head.rsplit_once('.')?;
            matches!(last, "named" | "getByName").then_some(())?;
            Some((format!("{before}.{set}{tail}"), rest))
        }
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

/// Whether `path` stays inside the directory it is relative to.
fn relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && path.split('/').all(|s| !matches!(s, "" | "." | ".."))
}

/// The accessor name and the file of `from(files("gradle/other.versions.toml"))` inside
/// `versionCatalogs { create("name") { .. } }` (Groovy: `name { .. }`).
fn catalog_file(tokens: &[Token], blocks: &[String]) -> Option<(String, String)> {
    let [.., parent, name] = blocks else {
        return None;
    };
    let files = tokens.iter().filter_map(Token::word).any(|w| w == "files");
    let [Token::Word(from), rest @ ..] = tokens else {
        return None;
    };
    let strings: Vec<&str> = (rest.iter())
        .filter_map(|t| match t {
            Token::String(s, false) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    let [path] = strings[..] else {
        return None;
    };
    (parent == "versionCatalogs"
        && from == "from"
        && files
        && relative(path)
        && super::literal(name))
    .then(|| (name.clone(), path.to_string()))
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
    let [set, lang @ ("java" | "kotlin"), "srcDirs" | "srcDir"] = path[at + 1..] else {
        return None;
    };
    // `named(set)` or `getByName(set)` with no literal name (`block_head`) is no known set.
    if matches!(set, "named" | "getByName") {
        return None;
    }
    // A Java directory of a set other than `main` is a test one. A Kotlin set is told by its
    // name (Gradle's `test`, the multiplatform `*Main` and `*Test`); any other is not read, as
    // its files may be production code (`Summary::roots`).
    let main = match (lang, set) {
        (_, "main") => true,
        ("kotlin", set) if set.ends_with("Main") => true,
        ("kotlin", set) if set != "test" && !set.ends_with("Test") => return None,
        _ => false,
    };
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
    let dirs = (arguments(rest).into_iter())
        .filter_map(|arg| match arg {
            [Token::String(s, false)] if relative(s.strip_suffix('/').unwrap_or(s)) => {
                Some(s.strip_suffix('/').unwrap_or(s).to_string())
            }
            _ => None,
        })
        .collect();
    Some((main, dirs))
}

/// The name a block's head line gives it: its first word, with a `named(..)`, `getByName(..)`,
/// `create(..)` or `register(..)` of a literal name read as that name (`sourceSets.named("main")`
/// is `sourceSets.main`, `create("libs")` in `versionCatalogs` is `libs`).
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
        if matches!(last, "named" | "getByName" | "create" | "register") && super::literal(set) {
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
    /// Each name the script assigns, with its string when that is its one assignment, at the
    /// top level or in `ext { }`, outside any condition.
    names: BTreeMap<String, Option<String>>,
    /// The version catalogs a settings file names, `(accessor, path)`.
    catalogs: Vec<(String, String)>,
    /// The `dependencies` statements, read once `names` is complete.
    pending: Vec<(String, Vec<Token>)>,
}

fn statement(tokens: &[Token], blocks: &[String], controlled: bool, scan: &mut Scan) {
    // `allprojects` configures this project too, so its assignments are never literal here.
    let conditional = controlled
        || blocks.iter().any(|b| b == "allprojects")
        || tokens.first().and_then(Token::word).is_some_and(control);
    let sole = literal_assignment(tokens)
        .filter(|_| !conditional && (blocks.is_empty() || blocks == ["ext"]));
    for target in targets(tokens) {
        let name = target.rsplit('.').next().unwrap_or(target);
        let literal = sole.filter(|(n, _)| *n == name).map(|(_, v)| v.to_string());
        // A second assignment leaves no literal.
        (scan.names.entry(name.to_string()))
            .and_modify(|v| *v = None)
            .or_insert(literal);
    }
    if !conditional
        && scan.catalogs.len() < MAX_CATALOGS
        && let Some(catalog) = catalog_file(tokens, blocks)
    {
        scan.catalogs.push(catalog);
    }
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
    {
        scan.pending.push((kind.to_string(), tokens[1..].to_vec()));
    }
}

// ponytail: Groovy slashy strings (`/a/`, `$/a/$`) are not read as strings, and a triple-quoted
// string reads as quoted pairs; text inside them may be scanned as code when its quotes do not
// pair up.
pub fn read(path: &str, src: &[u8]) -> Summary {
    if src.len() > MAX_BYTES {
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
        names,
        catalogs,
        pending,
    } = scan;
    build.catalogs = catalogs;
    let declared: BTreeSet<String> = names.keys().cloned().collect();
    let literals: BTreeMap<String, String> = (names.into_iter())
        .filter_map(|(name, value)| Some((name, value.filter(|v| super::literal(v))?)))
        .collect();
    summary.dependencies = (pending.iter())
        .filter_map(|(field, args)| dependency(field, args, &literals, &declared))
        .collect();
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
    build.catalogs.sort();
    build.catalogs.dedup();
    summary.build = Some(build);
    summary
        .dependencies
        .sort_by(|a, b| (&a.field, &a.name, &a.range).cmp(&(&b.field, &b.name, &b.range)));
    summary.dependencies.dedup();
    summary
}

/// The properties a `gradle.properties` gives, as `Summary::properties`: each `name=value` (or
/// `name:value`) line whose name is a plain key and whose value is no longer than a package
/// name and holds no escape, the last line of a name winning. Comments (`#`, `!`) and the other
/// lines are skipped; a file past `MAX_BYTES` reads as nothing.
pub fn properties(src: &[u8]) -> Summary {
    let text = (src.len() <= MAX_BYTES)
        .then(|| std::str::from_utf8(src).ok())
        .flatten()
        .unwrap_or("");
    let mut found = BTreeMap::new();
    for line in text.lines().map(str::trim_start) {
        let Some((key, value)) = line.split_once(['=', ':']) else {
            continue;
        };
        let (key, value) = (key.trim_end(), value.trim());
        if !line.starts_with(['#', '!'])
            && super::literal(key)
            && value.len() <= MAX_NAME
            && !value.contains('\\')
            && (found.len() < MAX_ENTRIES || found.contains_key(key))
        {
            found.insert(key.to_string(), value.to_string());
        }
    }
    Summary {
        properties: found,
        ..Summary::default()
    }
}

/// The libraries of a version catalog (`gradle/libs.versions.toml`), as `Summary::properties`:
/// each alias as Gradle's accessor spells it (`my-lib` and `my_lib` are `my.lib`) with its
/// `group:artifact` and, when `[versions]` or the library gives one, `:version`. A file past
/// `MAX_BYTES`, one that is no TOML, and a library with no literal coordinate read as nothing.
pub fn catalog(src: &[u8]) -> Summary {
    use toml::Value;
    let table = (src.len() <= MAX_BYTES)
        .then(|| std::str::from_utf8(src).ok())
        .flatten()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .unwrap_or_default();
    let versions = table.get("versions").and_then(Value::as_table);
    // A version as a string, or a rich version's strictest bound.
    fn version(v: &Value) -> Option<&str> {
        match v {
            Value::String(s) => Some(s),
            Value::Table(t) => ["strictly", "require", "prefer"]
                .iter()
                .find_map(|k| t.get(*k)?.as_str()),
            _ => None,
        }
    }
    let coordinate = |library: &Value| {
        let (group, artifact, wanted) = match library {
            Value::String(s) => {
                let mut parts = s.splitn(3, ':');
                (parts.next()?, parts.next()?, parts.next())
            }
            Value::Table(t) => {
                let text = |k: &str| t.get(k).and_then(Value::as_str);
                let (group, artifact) = match text("module") {
                    Some(module) => module.split_once(':')?,
                    None => (text("group")?, text("name")?),
                };
                let wanted = match t.get("version") {
                    Some(Value::Table(v)) if v.contains_key("ref") => {
                        let named = v.get("ref").and_then(Value::as_str);
                        named.and_then(|r| versions?.get(r)).and_then(version)
                    }
                    Some(v) => version(v),
                    None => None,
                };
                (group, artifact, wanted)
            }
            _ => return None,
        };
        let name = super::coordinate(group, artifact)?;
        let wanted = wanted.filter(|v| !v.is_empty() && v.len() <= MAX_NAME && !v.contains(':'));
        Some(wanted.map_or(name.clone(), |v| format!("{name}:{v}")))
    };
    let libraries = (table.get("libraries").and_then(Value::as_table).into_iter())
        .flatten()
        .take(MAX_ENTRIES);
    let properties = libraries
        .filter_map(|(alias, library)| Some((alias.replace(['-', '_'], "."), coordinate(library)?)))
        .filter(|(alias, _)| super::literal(alias))
        .collect();
    Summary {
        properties,
        ..Summary::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(path: &str, src: &str) -> super::super::Build {
        read(path, src.as_bytes()).build.unwrap_or_default()
    }

    #[test]
    fn gradle_properties_read_plain_keys_and_short_values_the_last_winning() {
        let found = |src: &str| properties(src.as_bytes()).properties;
        let src = "# c\n! c\na=1\n b : 2 \na=3\nspace key=x\nlong=\\u0041\nnone\nempty=\n";
        let read = found(src);
        let read: Vec<_> = read.iter().map(|(k, v)| (&k[..], &v[..])).collect();
        assert_eq!(read, [("a", "3"), ("b", "2"), ("empty", "")]);
        let long = format!("a={}\nb=1\n", "x".repeat(MAX_NAME + 1));
        assert_eq!(found(&long).keys().collect::<Vec<_>>(), ["b"]);
        let wide: String = (0..MAX_ENTRIES + 10).map(|i| format!("p{i}=1\n")).collect();
        assert_eq!(found(&wide).len(), MAX_ENTRIES);
        assert!(properties(&vec![b'a'; MAX_BYTES + 1]).properties.is_empty());
        assert!(properties(b"a=\xff").properties.is_empty());
    }

    #[test]
    fn a_catalog_maps_each_alias_to_its_coordinate_and_version() {
        let src = r#"
[versions]
guava = "33.0"
rich = { strictly = "2.0", prefer = "2.1" }

[libraries]
guava = { module = "com.google.guava:guava", version.ref = "guava" }
my-lib_x = { group = "com.acme", name = "lib", version = "1.0" }
rich = { module = "com.acme:rich", version.ref = "rich" }
short = "com.acme:short:3"
bare = "com.acme:bare"
absent = { module = "com.acme:absent", version.ref = "nowhere" }
wild = { module = "com.*:wild" }
no-colon = "bare"
list = ["com.acme:list"]
"#;
        let found = catalog(src.as_bytes()).properties;
        let found: Vec<_> = found.iter().map(|(k, v)| (&k[..], &v[..])).collect();
        assert_eq!(
            found,
            [
                ("absent", "com.acme:absent"),
                ("bare", "com.acme:bare"),
                ("guava", "com.google.guava:guava:33.0"),
                ("my.lib.x", "com.acme:lib:1.0"),
                ("rich", "com.acme:rich:2.0"),
                ("short", "com.acme:short:3"),
            ]
        );
        assert!(catalog(&vec![b' '; MAX_BYTES + 1]).properties.is_empty());
        assert!(catalog(b"[libraries\n").properties.is_empty());
    }

    #[test]
    fn a_catalog_and_a_settings_file_stop_at_their_limits() {
        let wide: String = (0..MAX_ENTRIES + 10)
            .map(|i| format!("a{i} = \"g:a{i}\"\n"))
            .collect();
        let found = catalog(format!("[libraries]\n{wide}").as_bytes());
        assert_eq!(found.properties.len(), MAX_ENTRIES);
        let many: String = (0..MAX_CATALOGS + 5)
            .map(|i| format!("create(\"c{i}\") {{ from(files(\"c{i}.versions.toml\")) }}\n"))
            .collect();
        let settings = format!("versionCatalogs {{\n{many}}}\n");
        let catalogs = build("settings.gradle.kts", &settings).catalogs;
        assert_eq!(catalogs.len(), MAX_CATALOGS);
    }

    #[test]
    fn the_script_decides_a_version_before_gradle_properties_can() {
        let range = |src: &str| {
            let summary = read("build.gradle", src.as_bytes());
            summary
                .dependencies
                .into_iter()
                .map(|e| e.range)
                .collect::<Vec<_>>()
        };
        let dep = "dependencies { implementation \"g:a:$version\" }";
        let some = |v: &str| vec![Some(v.to_string())];
        assert_eq!(range(&format!("version = '1.0'\n{dep}")), some("1.0"));
        assert_eq!(
            range(&format!("version = '1.0'\nversion = '2'\n{dep}")),
            [None]
        );
        assert_eq!(range(dep), some("$version"));
    }

    #[test]
    fn a_settings_file_names_its_catalogs_by_a_relative_file() {
        let catalogs = |src: &str| build("settings.gradle.kts", src).catalogs;
        let kts = "versionCatalogs {\n create(\"deps\") {\n  from(files(\"gradle/deps.versions.toml\"))\n }\n}\n";
        assert_eq!(
            catalogs(kts),
            [("deps".to_string(), "gradle/deps.versions.toml".to_string())]
        );
        let groovy = "versionCatalogs {\n libs {\n  from files('x.versions.toml')\n }\n}\n";
        assert_eq!(
            catalogs(groovy),
            [("libs".to_string(), "x.versions.toml".to_string())]
        );
        for bad in [
            "from(files(\"x.versions.toml\"))",
            "versionCatalogs { create(\"a\") { from(files(\"../x.versions.toml\")) } }",
            "versionCatalogs { create(\"a\") { from(files(\"/x.versions.toml\")) } }",
            "versionCatalogs { create(\"a\") { from(files(\"${d}.versions.toml\")) } }",
            "versionCatalogs { create(\"a\") { from(\"g:a:1\") } }",
        ] {
            assert!(catalogs(bad).is_empty(), "{bad}");
        }
    }

    #[test]
    fn a_bare_hole_is_braced_unless_a_property_follows_it() {
        assert_eq!(braced("a$b:${c}:$d_e"), "a${b}:${c}:${d_e}");
        assert_eq!(braced("$x.y $ $"), "$x.y $ $");
    }

    #[test]
    fn an_entry_keeps_its_holes_for_the_seam_unless_the_script_assigns_the_name() {
        let template = |src: &str| {
            let summary = read("build.gradle", src.as_bytes());
            summary
                .dependencies
                .into_iter()
                .map(|e| e.template)
                .collect::<Vec<_>>()
        };
        let dep = "dependencies { implementation \"example:$a:${b}\" }";
        assert_eq!(
            template(dep),
            [Some(("example".to_string(), "${a}".to_string()))]
        );
        assert_eq!(template(&format!("def a = 'x'\na = 'y'\n{dep}")), [None]);
        assert_eq!(
            template("dependencies { implementation 'example:a:1' }"),
            [None]
        );
        let accessor = read("build.gradle", b"dependencies { implementation(libs.a.b) }");
        assert_eq!(
            accessor.dependencies[0].via.as_deref(),
            Some("catalog libs.a.b")
        );
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

    /// The `(name, via)` of each dependency of a script.
    fn coordinates(src: &str) -> Vec<(String, Option<String>)> {
        let summary = read("build.gradle", src.as_bytes());
        (summary.dependencies.into_iter())
            .map(|e| (e.name, e.via))
            .collect()
    }

    #[test]
    fn a_name_assigned_once_to_a_plain_string_reads_as_it() {
        let written = |name: &str| (name.to_string(), None);
        let computed = |name: &str| (name.to_string(), Some("computed".to_string()));
        let dep = r#"dependencies { implementation "example:a${s}:1" }"#;
        for (declaration, expected) in [
            ("def s = 'pi'", written("example:api")),
            ("val s = \"pi\"", written("example:api")),
            ("ext.s = 'pi'", written("example:api")),
            ("ext {\n s = 'pi'\n}", written("example:api")),
            ("project.ext.s = 'pi'", written("example:api")),
            ("s = 'pi'", written("example:api")),
            // Assigned twice, however it is written, or not once at the top level.
            ("def s = 'pi'\ns = 'qa'", computed("example:a*")),
            ("def s = 'pi'\next.s = 'qa'", computed("example:a*")),
            ("def s = 'pi'\nif (c) { s = 'qa' }", computed("example:a*")),
            ("def s = 'pi'\nif (c) s = 'qa'", computed("example:a*")),
            ("val s: String = \"pi\"", written("example:api")),
            ("val s: String = \"pi\"\ns = \"qa\"", computed("example:a*")),
            ("def s = 'p'\ns += 'i'", computed("example:a*")),
            // Not a plain string: interpolated, computed, in a condition, a block or a call.
            ("def s = \"p${i}\"", computed("example:a*")),
            ("def s = 'p' + 'i'", computed("example:a*")),
            ("def s = prop('s')", computed("example:a*")),
            ("if (c) { def s = 'pi' }", computed("example:a*")),
            ("allprojects { ext.s = 'pi' }", computed("example:a*")),
            ("subprojects { def s = 'pi' }", computed("example:a*")),
            ("dependencies { def s = 'pi' }", computed("example:a*")),
            // Declared nowhere in the script.
            ("def other = 'pi'", computed("example:a*")),
        ] {
            assert_eq!(
                coordinates(&format!("{declaration}\n{dep}")),
                [expected],
                "{declaration}"
            );
        }
    }

    #[test]
    fn the_projects_own_group_fills_a_hole_and_the_holes_left_stay_computed() {
        let src = |group: &str| {
            format!(
                "{group}\ndependencies {{\n  api \"$group:models:1\"\n  api \"${{group}}.x:\" + v\n  \
                 api \"$group.models:tools\"\n  api \"$other:${{group}}-kit\"\n}}"
            )
        };
        let (written, computed) = (None, Some("computed".to_string()));
        assert_eq!(
            coordinates(&src("group = 'example'")),
            [
                ("*.models:tools".to_string(), computed.clone()),
                ("*:example-kit".to_string(), computed.clone()),
                ("example.x:*".to_string(), computed),
                ("example:models".to_string(), written),
            ]
        );
        // A group the script sets again leaves every hole.
        let again = coordinates(&src("group = 'a'\ngroup = 'b'"));
        assert!(again.iter().all(|(_, via)| via.is_some()), "{again:?}");
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
            // A set looked up by a literal name, then its directories.
            (
                "sourceSets.getByName(\"test\").kotlin.srcDir(\"src/t\")",
                &[],
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
    fn a_kotlin_directory_is_main_or_test_by_the_name_of_its_set() {
        let tested = |set: &str| {
            let (all, tests) = roots(&format!(
                "sourceSets {{ {set} {{ kotlin.srcDir(\"src/x\") }} }}"
            ));
            (
                all.iter().any(|r| r == "src/x"),
                tests.iter().any(|t| t == "src/x"),
            )
        };
        // Multiplatform: `*Main` is production, `*Test` is test code.
        for main in ["main", "commonMain", "jvmMain"] {
            assert_eq!(tested(main), (true, false), "{main}");
        }
        for test in ["test", "commonTest", "jvmTest", "integrationTest"] {
            assert_eq!(tested(test), (true, true), "{test}");
        }
        // Any other set is read as nothing: its files resolve by their package.
        for other in ["integration", "val jvmMain by getting", "benchmarks"] {
            assert_eq!(tested(other), (false, false), "{other}");
        }
        // A Java directory of a set other than `main` stays a test root.
        let (_, tests) = roots("sourceSets.integration.java.srcDirs = ['src/i']");
        assert!(tests.iter().any(|t| t == "src/i"));
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
