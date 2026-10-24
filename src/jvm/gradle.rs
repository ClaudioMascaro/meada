//! A line-oriented scanner for Gradle's literal assignments and dependency coordinates.
//! Tokens only separate strings, comments and block boundaries; no build grammar is loaded.

use crate::package::{Entry, Summary};

#[derive(Debug)]
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
    let mut text = String::new();
    let mut computed = false;
    let mut quoted = 0;
    for token in args {
        match token {
            Token::String(s, dynamic) => {
                text.push_str(s);
                computed |= dynamic;
                quoted += 1;
            }
            Token::Mark('(' | ')') => {}
            Token::Mark('+') => computed = true,
            _ => {
                text.push('*');
                computed = true;
            }
        }
    }
    computed |= quoted != 1;
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
    let (group, artifact, range) = match parts.as_slice() {
        [g, a] => (*g, *a, None),
        [g, a, v] => (*g, *a, Some((*v).to_string())),
        _ if computed => ("*", "*", None),
        _ => return None,
    };
    let group = if computed {
        super::pattern(group)?
    } else {
        super::literal(group).then(|| group.to_string())?
    };
    let artifact = if computed {
        super::pattern(artifact)?
    } else {
        super::literal(artifact).then(|| artifact.to_string())?
    };
    let name = format!("{group}:{artifact}");
    if name.len() > crate::package::MAX_NAME {
        return None;
    }
    Some(Entry {
        field: field.into(),
        name,
        range,
        via: computed.then(|| "computed".into()),
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

/// The arguments of `include` that are a lone literal string, in `include(":a", ":b")` or
/// `include ':a', ':b'`; an argument that computes its value (a call, a concatenation) names
/// no project.
fn included(args: &[Token]) -> Vec<&str> {
    let args = match args {
        [Token::Mark('('), inner @ .., Token::Mark(')')] => inner,
        _ => args,
    };
    let mut depth = 0usize;
    let mut out = Vec::new();
    for arg in args.split(|t| {
        match t {
            Token::Mark('(' | '[') => depth += 1,
            Token::Mark(')' | ']') => depth = depth.saturating_sub(1),
            _ => {}
        }
        depth == 0 && matches!(t, Token::Mark(','))
    }) {
        if let [Token::String(s, false)] = arg {
            out.push(s.as_str());
        }
    }
    out
}

fn statement(tokens: &[Token], blocks: &[String], build: &mut super::Build, summary: &mut Summary) {
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
    let conditional = blocks.iter().any(|s| control(s))
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
    if blocks.len() == 1
        && blocks[0] == "dependencies"
        && let Some(kind) = tokens
            .first()
            .and_then(Token::word)
            .filter(|s| configuration(s))
        && let Some(entry) = dependency(kind, &tokens[1..])
    {
        summary.dependencies.push(entry);
    }
}

pub fn read(path: &str, src: &[u8]) -> Summary {
    if src.len() > crate::lang::MAX_BYTES {
        return Summary::default();
    }
    let Ok(text) = std::str::from_utf8(src) else {
        return Summary::default();
    };
    let mut build = super::Build::default();
    let mut summary = Summary::default();
    let mut blocks = Vec::new();
    let mut line = Vec::new();
    let mut enclosed = 0usize;
    for token in tokens(text) {
        match token {
            Token::Mark('{') => {
                blocks.push(
                    line.iter()
                        .filter_map(Token::word)
                        .next()
                        .unwrap_or("")
                        .to_string(),
                );
                line.clear();
            }
            Token::Mark('}') => {
                statement(&line, &blocks, &mut build, &mut summary);
                line.clear();
                blocks.pop();
            }
            Token::Mark('\n') if enclosed > 0 => {}
            Token::Mark('\n' | ';') if enclosed == 0 => {
                statement(&line, &blocks, &mut build, &mut summary);
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
    statement(&line, &blocks, &mut build, &mut summary);
    let settings = path
        .rsplit('/')
        .next()
        .is_some_and(|p| p.starts_with("settings."));
    if !settings {
        summary.roots = vec!["src/main/java".into(), "src/test/java".into()];
    }
    build.includes.sort();
    build.includes.dedup();
    summary.build = Some(build);
    summary
        .dependencies
        .sort_by(|a, b| (&a.field, &a.name, &a.range).cmp(&(&b.field, &b.name, &b.range)));
    summary.dependencies.dedup();
    summary
}
