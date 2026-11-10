//! The paths a Compose build takes from its build context (#744, ADR 0026): the Dockerfile it
//! builds with is read while indexing, and the build runs the `COPY`/`ADD` sources and the bind
//! mounts of its `RUN`s, in every stage. A Dockerfile this reader cannot read leaves the whole
//! context, at `inferred`. `.dockerignore` is not read: a path may cover files the image
//! excludes, never fewer.

use super::code::Code;
use crate::contract::{self, Probe};
use crate::iac::{Dockerfile, Node};
use std::collections::BTreeSet;
use std::path::Path;

/// Why a Dockerfile is not read, as the execution's reason says it.
type Unread = &'static str;

/// How many sources a Dockerfile may name, and how many paths they may reach in the repository.
// ponytail: 256 paths bound the executions one build stores; a Dockerfile past it is not read.
const PATHS_MAX: usize = 256;

/// How many directory entries the wildcard sources of one Dockerfile may examine.
// ponytail: 16,384 entries bound the listing a hostile wildcard (`*/*/*/*`) walks; past it the
// Dockerfile is not read.
const WORK_MAX: usize = 16_384;

/// The sources a Dockerfile's `COPY`, `ADD` and `RUN --mount=type=bind` take from the build
/// context, each normalized from the context root (`.` for the whole of it) with a wildcard still
/// in it. `COPY --from`, `ADD` of a URL or a git repository, a heredoc and every other mount read
/// nothing from the context. An error says why the Dockerfile is not read: a source with a
/// variable or a `**` under `--parents`, a `# escape=` directive, a flag or an instruction this
/// reader does not know.
pub(super) fn sources(text: &str) -> Result<Vec<String>, Unread> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for line in text.lines() {
        // A parser directive is a comment of the form `# name=value` before anything else.
        let Some((name, value)) = line
            .trim()
            .strip_prefix('#')
            .and_then(|c| c.split_once('='))
        else {
            break;
        };
        let name = name.trim();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
            break;
        }
        if name.eq_ignore_ascii_case("escape") && value.trim() != "\\" {
            return Err("an escape directive");
        }
    }
    let mut found = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let mut piece = line.trim_start();
        if piece.is_empty() || piece.starts_with('#') {
            continue;
        }
        // A continuing line joins the next one that is neither blank nor a comment, which keeps
        // its leading blanks: `COPY a\` and `  b ./` are the words `a` and `b`.
        let mut logical = String::new();
        while let Some(head) = continued(piece) {
            logical.push_str(head);
            piece = "";
            for next in lines.by_ref() {
                let bare = next.trim_start();
                if !(bare.is_empty() || bare.starts_with('#')) {
                    piece = next;
                    break;
                }
            }
        }
        logical.push_str(piece);
        let logical = logical.trim();
        if !logical.is_empty() {
            instruction(logical, &mut lines, &mut found)?;
        }
    }
    Ok(found)
}

/// A line without the backslash that continues it, as Docker's parser reads the default escape
/// (`([^\\])\\[ \t]*$|^\\[ \t]*$`): a backslash, and the blanks after it, ends the line, unless
/// another backslash precedes it (`\\` is a complete line).
fn continued(line: &str) -> Option<&str> {
    let head = line.trim_end_matches([' ', '\t']).strip_suffix('\\')?;
    (!head.ends_with('\\')).then_some(head)
}

fn instruction(
    line: &str,
    lines: &mut std::str::Lines<'_>,
    found: &mut Vec<String>,
) -> Result<(), Unread> {
    let (word, args) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let (word, args) = (word.to_ascii_uppercase(), args.trim());
    match word.as_str() {
        "COPY" | "ADD" => copy(word == "ADD", args, found)?,
        "RUN" => run(args, found)?,
        // `ONBUILD` runs in a build of the image, whose context is another.
        "ONBUILD" | "FROM" | "ARG" | "ENV" | "WORKDIR" | "CMD" | "ENTRYPOINT" | "EXPOSE"
        | "LABEL" | "USER" | "VOLUME" | "HEALTHCHECK" | "STOPSIGNAL" | "SHELL" | "MAINTAINER" => {}
        _ => return Err("an instruction this reader does not know"),
    }
    if found.len() > PATHS_MAX {
        return Err("more sources than the reader keeps");
    }
    if line.contains("<<") && takes_heredocs(&word, args)? {
        // The lines of each heredoc are its body, up to its delimiter, not instructions.
        for (delimiter, tabs) in heredocs(line)? {
            let ends = |l: &str| {
                let l = l.trim_end_matches('\r');
                (if tabs { l.trim_start_matches('\t') } else { l }) == delimiter
            };
            if !lines.by_ref().any(ends) {
                return Err("a heredoc with no end");
            }
        }
    }
    Ok(())
}

/// Whether Docker reads heredocs after an instruction (`canContainHeredoc`): `RUN`, `COPY` or
/// `ADD`, also as the instruction of an `ONBUILD`, in shell form rather than a JSON list.
fn takes_heredocs(word: &str, args: &str) -> Result<bool, Unread> {
    let (word, args) = match word {
        "ONBUILD" => {
            let (word, args) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
            (word.to_ascii_uppercase(), args.trim())
        }
        _ => (word.to_string(), args),
    };
    Ok(matches!(word.as_str(), "RUN" | "COPY" | "ADD")
        && serde_json::from_str::<Vec<String>>(flags(args)?.1).is_err())
}

/// The heredocs a line opens, in order: each delimiter, and whether `<<-` strips the tabs before
/// it, from the words Docker's shell lexer splits the line into (`heredocsFromLine`).
fn heredocs(line: &str) -> Result<Vec<(String, bool)>, Unread> {
    let words = words(line)?.unwrap_or_default();
    Ok(words.iter().filter_map(|word| heredoc(word)).collect())
}

/// The name a word opens a heredoc with, and whether `<<-` strips its tabs, as Docker's parser
/// reads it: `^(\d*)<<(-?)\s*([^<]*)$`, then the name as one word without its quotes and escapes.
fn heredoc(word: &str) -> Option<(String, bool)> {
    let rest = word.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = rest.strip_prefix("<<")?;
    let (tabs, rest) = rest.strip_prefix('-').map_or((false, rest), |r| (true, r));
    let rest = rest.trim_start_matches(['\t', '\n', '\x0c', '\r', ' ']);
    if rest.contains('<') {
        return None;
    }
    let (mut name, mut quote, mut chars) = (String::new(), None, rest.chars().peekable());
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '\\') => name.extend(chars.next()),
            (Some('"'), '\\') => name.push(
                chars
                    .next_if(|e| matches!(e, '"' | '$' | '\\'))
                    .unwrap_or(c),
            ),
            (None, c) if c.is_whitespace() => return None,
            (_, c) => name.push(c),
        }
    }
    (quote.is_none() && !name.is_empty()).then_some((name, tabs))
}

/// The words Docker's shell lexer splits a line into to find its heredocs, keeping quotes, escapes
/// and unset variables as written: a blank ends a word unless it is quoted, escaped or right after
/// `<<`. `None` when the lexer fails on the line, which then opens no heredoc.
fn words(line: &str) -> Result<Option<Vec<String>>, Unread> {
    let (mut out, mut word, mut chars) = (Vec::new(), String::new(), line.chars().peekable());
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            continue;
        }
        word.push(c);
        match c {
            '\\' => word.extend(chars.next()),
            '\'' | '"' => loop {
                let Some(q) = chars.next() else {
                    return Ok(None);
                };
                word.push(q);
                match q {
                    _ if q == c => break,
                    '\\' if c == '"' => word.extend(chars.next()),
                    '$' if c == '"' => variable(&mut chars, &mut word)?,
                    _ => {}
                }
            },
            '$' => variable(&mut chars, &mut word)?,
            '<' if chars.next_if_eq(&'<').is_some() => {
                word.push('<');
                word.extend(std::iter::from_fn(|| {
                    chars.next_if(|b| matches!(b, ' ' | '\t' | '\r'))
                }));
            }
            _ => {}
        }
    }
    out.extend((!word.is_empty()).then_some(word));
    Ok(Some(out))
}

/// A `$` on a heredoc's line: Docker keeps `$name` and `${name}` as written, and splits the other
/// `${...}` forms (`${a:-b c}`) into words of their own or fails on them.
fn variable(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    word: &mut String,
) -> Result<(), Unread> {
    if chars.next_if_eq(&'{').is_none() {
        return Ok(());
    }
    let name: String =
        std::iter::from_fn(|| chars.next_if(|c| c.is_ascii_alphanumeric() || *c == '_')).collect();
    let plain = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
    if !plain || chars.next_if_eq(&'}').is_none() {
        return Err("a variable form this reader does not expand");
    }
    word.push('{');
    word.push_str(&name);
    word.push('}');
    Ok(())
}

/// The `--name=value` flags of an instruction, as (lowercase name, value) pairs.
type Flags = Vec<(String, String)>;

/// The leading flags of an instruction and what follows them.
fn flags(mut args: &str) -> Result<(Flags, &str), Unread> {
    let mut out = Vec::new();
    while let Some(rest) = args.strip_prefix("--") {
        let (flag, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if flag.contains(['"', '\'']) {
            return Err("a quoted flag");
        }
        let (name, value) = flag.split_once('=').unwrap_or((flag, ""));
        out.push((name.to_ascii_lowercase(), value.to_string()));
        args = tail.trim_start();
    }
    Ok((out, args))
}

fn copy(add: bool, args: &str, found: &mut Vec<String>) -> Result<(), Unread> {
    let (flags, args) = flags(args)?;
    let (mut from, mut parents) = (false, false);
    for (name, value) in &flags {
        match name.as_str() {
            // `--from=` copies from the context.
            "from" => from = !value.is_empty(),
            "parents" => parents = !value.eq_ignore_ascii_case("false"),
            "chown" | "chmod" | "link" | "exclude" | "keep-git-dir" | "checksum" | "unpack" => {}
            _ => return Err("a COPY or ADD flag this reader does not know"),
        }
    }
    // A JSON list is read as one; anything else is split at blanks, as Docker's parser does.
    let words = serde_json::from_str::<Vec<String>>(args)
        .unwrap_or_else(|_| args.split_whitespace().map(str::to_string).collect());
    let Some((_, sources)) = words.split_last().filter(|(_, s)| !s.is_empty()) else {
        return Err("a COPY or ADD with no destination");
    };
    for source in sources.iter().filter(|_| !from) {
        let remote = add && (source.contains("://") || source.starts_with("git@"));
        if remote || heredoc(source).is_some() {
            continue;
        }
        // Under `--parents` Docker matches with its pattern matcher, where `**` crosses directories.
        if parents && source.contains("**") {
            return Err("a ** wildcard with --parents");
        }
        found.push(normalize(source)?);
    }
    Ok(())
}

fn run(args: &str, found: &mut Vec<String>) -> Result<(), Unread> {
    for (_, value) in flags(args)?.0.iter().filter(|(name, _)| name == "mount") {
        // buildkit's `parseMount` reads the keys and the type in any case; `from=` is the context.
        let (mut kind, mut source, mut from) = (String::from("bind"), ".", "");
        for option in value.split(',') {
            let (key, v) = option.split_once('=').unwrap_or((option, ""));
            match key.to_ascii_lowercase().as_str() {
                "type" => kind = v.to_ascii_lowercase(),
                "source" | "src" => source = v,
                "from" => from = v,
                _ => {}
            }
        }
        if kind.contains('$') {
            return Err("a mount type with a variable");
        }
        if kind == "bind" && from.is_empty() {
            found.push(normalize(source)?);
        }
    }
    Ok(())
}

/// A source from the context root: no leading `/`, `.` and empty steps dropped, `..` popped.
fn normalize(source: &str) -> Result<String, Unread> {
    if source.contains('$') {
        return Err("a source with a variable");
    }
    if source.contains(['"', '\'', '`', '\\']) {
        return Err("a quoted or escaped source");
    }
    let mut parts: Vec<&str> = Vec::new();
    for step in source.split('/') {
        match step {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or("a source outside the build context")?;
            }
            step => parts.push(step),
        }
    }
    Ok(match parts.is_empty() {
        true => ".".into(),
        false => parts.join("/"),
    })
}

/// One element of a path segment's pattern. A `*` matches any run of characters but `/`, which a
/// segment never holds.
enum Tok {
    Star,
    Any,
    Lit(char),
    Class(bool, Vec<(char, char)>),
}

/// The tokens of a segment with a wildcard, as Go's `filepath.Match` reads it (`*`, `?`,
/// `[a-z]`, `[^a-z]`), or `None` for a malformed one.
fn compile(segment: &str) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let mut chars = segment.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(match c {
            '*' if matches!(out.last(), Some(Tok::Star)) => continue,
            '*' => Tok::Star,
            '?' => Tok::Any,
            '[' => {
                let negated = chars.next_if_eq(&'^').is_some();
                let mut ranges = Vec::new();
                loop {
                    let lo = chars.next()?;
                    if lo == ']' && !ranges.is_empty() {
                        break;
                    }
                    if matches!(lo, ']' | '-') {
                        return None;
                    }
                    let hi = match chars.next_if_eq(&'-') {
                        Some(_) => chars.next().filter(|c| !matches!(c, ']' | '-'))?,
                        None => lo,
                    };
                    ranges.push((lo, hi));
                }
                Tok::Class(negated, ranges)
            }
            c => Tok::Lit(c),
        });
    }
    Some(out)
}

/// Whether `name` is matched whole by `tokens`. A mismatch restarts from the last star only, one
/// character on, so it costs at most the name's length times the pattern's, however many stars
/// the pattern holds; `name` is one directory entry, which file systems cap at 255 bytes.
fn fits(tokens: &[Tok], name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let one = |t: &Tok, c: char| match t {
        Tok::Any => true,
        Tok::Lit(l) => *l == c,
        Tok::Class(negated, ranges) => {
            ranges.iter().any(|&(lo, hi)| lo <= c && c <= hi) != *negated
        }
        Tok::Star => false,
    };
    let (mut at, mut i, mut back) = (0, 0, None);
    while i < name.len() {
        match tokens.get(at) {
            Some(Tok::Star) => {
                back = Some((at + 1, i));
                at += 1;
                continue;
            }
            Some(t) if one(t, name[i]) => {
                at += 1;
                i += 1;
                continue;
            }
            _ => {}
        }
        let Some((token, from)) = back else {
            return false;
        };
        (at, i, back) = (token, from + 1, Some((token, from + 1)));
    }
    tokens[at..].iter().all(|t| matches!(t, Tok::Star))
}

/// What is at `path`: the kind of its first step that is missing, a link or no directory, else of
/// its last. Every step is probed, so a refresh sees one change.
fn reach(root: &Path, path: &str) -> Probe {
    let (mut at, mut last) = (String::new(), Probe::Dir);
    let mut steps = path
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .peekable();
    while let Some(step) = steps.next() {
        if !at.is_empty() {
            at.push('/');
        }
        at.push_str(step);
        last = contract::probe(root, &at);
        match last {
            Probe::Dir => {}
            Probe::File if steps.peek().is_some() => return Probe::Missing,
            _ => return last,
        }
    }
    last
}

fn join(dir: &str, name: &str) -> String {
    match dir.is_empty() {
        true => name.into(),
        false => format!("{dir}/{name}"),
    }
}

/// The repository paths the sources reach, found by walking the repository from the context
/// directory one segment at a time, as Docker matches: a segment with a wildcard lists its
/// directory, any other is probed. A path that is not there reaches nothing.
struct Walk<'a> {
    root: &'a Path,
    work: usize,
    found: BTreeSet<String>,
}

impl Walk<'_> {
    fn step(&mut self, dir: &str, steps: &[&str]) -> Result<(), Unread> {
        let Some((head, rest)) = steps.split_first() else {
            return Ok(());
        };
        if !head.contains(['*', '?', '[']) {
            let path = join(dir, head);
            let kind = contract::probe(self.root, &path);
            return self.take(path, kind, rest);
        }
        let tokens = compile(head).ok_or("a malformed wildcard")?;
        let entries = contract::list(self.root, dir).ok_or("a directory too large to list")?;
        self.work += entries.len();
        for (name, kind) in entries.into_iter().filter(|(name, _)| fits(&tokens, name)) {
            self.take(join(dir, &name), kind, rest)?;
        }
        Ok(())
    }

    fn take(&mut self, path: String, kind: Probe, rest: &[&str]) -> Result<(), Unread> {
        self.work += 1;
        if self.work > WORK_MAX {
            return Err("more files than the reader examines");
        }
        match (kind, rest.is_empty()) {
            (Probe::Link, _) => Err("a symbolic link"),
            (Probe::Dir | Probe::File, true) => {
                self.found.insert(path);
                match self.found.len() > PATHS_MAX {
                    true => Err("more paths than the reader keeps"),
                    false => Ok(()),
                }
            }
            (Probe::Dir, false) => self.step(&path, rest),
            // Missing, a socket, or a file with steps after it: nothing there.
            _ => Ok(()),
        }
    }
}

/// The Dockerfile a Compose build reads: the repository path (or what names it, when it has
/// none) and its text, or why it is not read.
fn load(
    root: &Path,
    context: &str,
    dockerfile: Option<&Dockerfile>,
) -> (String, Result<String, Unread>) {
    if let Some(inline) = dockerfile.filter(|d| d.inline) {
        let text = match inline.text.trim().is_empty() {
            true => Err("an empty dockerfile_inline"),
            false => Ok(inline.text.clone()),
        };
        return ("dockerfile_inline".into(), text);
    }
    let written = dockerfile.map_or("Dockerfile", |d| d.text.as_str());
    if written.is_empty() || written.starts_with('/') || written.contains(['$', ':', '\\', '~']) {
        return (written.into(), Err("a path this reader does not follow"));
    }
    let dir = if context == "." { "" } else { context };
    let Some(path) = crate::iac::joined(dir, written) else {
        return (written.into(), Err("a path outside the repository"));
    };
    let why = match reach(root, &path) {
        Probe::File => None,
        Probe::Missing => Some("missing"),
        Probe::Link => Some("a symbolic link"),
        _ => Some("not a file"),
    };
    let text = match why {
        Some(why) => Err(why),
        None => contract::read(root, &path)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or("unreadable, too large or not UTF-8"),
    };
    (path, text)
}

/// The repository paths the Dockerfile `text` takes from the context `context` (`.` for the
/// root), or why it is not read.
fn taken(root: &Path, context: &str, text: &str) -> Result<BTreeSet<String>, Unread> {
    let dir = if context == "." { "" } else { context };
    let mut walk = Walk {
        root,
        work: 0,
        found: BTreeSet::new(),
    };
    for source in sources(text)? {
        match source.as_str() {
            "." => {
                walk.found.insert(context.to_string());
            }
            _ => walk.step(dir, &source.split('/').collect::<Vec<_>>())?,
        }
    }
    Ok(walk.found)
}

/// The executions of a Compose service that builds from a context of the repository: one per path
/// its Dockerfile takes, or one of the whole context at `inferred` when the Dockerfile is not
/// read. A service whose Dockerfile takes nothing runs no code directly. `room` is what the
/// summary may still store; a build whose paths do not fit is not read, and one that cannot store
/// even that runs nothing here.
pub(super) fn executions(root: &Path, node: &Node, room: usize) -> Vec<Code> {
    let Some(context) = node.build.as_ref().filter(|_| {
        node.ty == "service"
            && crate::iac::types::workload("compose", &node.ty).is_some_and(|w| w.code.is_some())
    }) else {
        return Vec::new();
    };
    if context.text.contains([':', '\\']) || reach(root, &context.text) != Probe::Dir {
        return Vec::new();
    }
    let base = format!("Compose build {}#{}.build", node.path, node.block);
    let (label, text) = load(root, &context.text, node.dockerfile.as_ref());
    let made = |paths: BTreeSet<String>, reason: String, inferred: bool| -> Vec<Code> {
        paths
            .into_iter()
            .map(|path| Code {
                address: node.address.clone(),
                root: node.root.clone(),
                path,
                reason: reason.clone(),
                inferred,
            })
            .collect()
    };
    let cost = |codes: &[Code]| {
        (codes.iter())
            .map(|c| c.address.len() + c.root.len() + c.path.len() + c.reason.len())
            .sum::<usize>()
    };
    let overridable =
        context.overridable || node.dockerfile.as_ref().is_some_and(|d| d.overridable);
    let unread = |why: Unread| {
        let reason = format!("{base}; Dockerfile {label} not read: {why}");
        made(BTreeSet::from([context.text.clone()]), reason, true)
    };
    let read = text.and_then(|text| taken(root, &context.text, &text));
    let codes = match read {
        Ok(paths) => made(paths, format!("{base}; Dockerfile {label}"), overridable),
        Err(why) => unread(why),
    };
    if cost(&codes) <= room {
        return codes;
    }
    let codes = unread("more stored paths than the summary has room for");
    if cost(&codes) <= room {
        codes
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iac::Resolved;
    use std::path::PathBuf;

    fn ok(text: &str) -> Vec<String> {
        sources(text).unwrap()
    }

    fn unread(text: &str) -> Unread {
        sources(text).unwrap_err()
    }

    /// A repository with an empty file at each path.
    fn repo(case: &str, files: &[&str]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("meada-dockerfile-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for file in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        dir
    }

    fn node(context: &str, dockerfile: Option<(&str, bool)>) -> Node {
        Node {
            address: "services.app".into(),
            root: ".".into(),
            ty: "service".into(),
            path: "compose.yaml".into(),
            block: "services.app".into(),
            build: Some(Resolved {
                text: context.into(),
                overridable: false,
            }),
            dockerfile: dockerfile.map(|(text, inline)| Dockerfile {
                text: text.into(),
                overridable: false,
                inline,
            }),
            ..Default::default()
        }
    }

    fn paths(codes: &[Code]) -> Vec<&str> {
        codes.iter().map(|c| c.path.as_str()).collect()
    }

    #[test]
    fn copy_add_and_bind_mounts_give_their_sources_in_every_stage() {
        let text = "# syntax=docker/dockerfile:1\nFROM node:20 AS build\nWORKDIR /app\n\
            COPY package.json bun.lock ./\nCOPY --chown=1:1 ./frontend /app/frontend\n\
            ADD --link vendor/lib.tar.gz /opt/\n\
            RUN --mount=type=bind,source=src,target=/src --mount=type=cache,target=/c make\n\
            FROM scratch\nCOPY --from=build /app/dist /dist\nCOPY --from=nginx /etc/n.conf /etc/\n\
            ADD https://example.com/x.tgz /x\nADD git@github.com:acme/lib.git /lib\n\
            COPY [\"a b.txt\", \"c.txt\", \"/dest/\"]\nonbuild copy hooks /h\nrun echo $HOME\n";
        assert_eq!(
            ok(text),
            [
                "package.json",
                "bun.lock",
                "frontend",
                "vendor/lib.tar.gz",
                "src",
                "a b.txt",
                "c.txt"
            ]
        );
    }

    #[test]
    fn a_bind_mount_with_no_source_is_the_whole_context_and_other_mounts_read_nothing() {
        assert_eq!(ok("RUN --mount=type=bind,target=/x make"), ["."]);
        assert_eq!(ok("RUN --mount=target=/x,src=a/b make"), ["a/b"]);
        for mount in [
            "type=cache,target=/c",
            "type=tmpfs,target=/t",
            "type=secret,id=s",
            "type=ssh",
            "type=bind,from=build,source=src,target=/s",
            "type=bind,from=build,target=/s",
        ] {
            assert_eq!(
                ok(&format!("RUN --mount={mount} make")),
                Vec::<String>::new(),
                "{mount}"
            );
        }
    }

    #[test]
    fn a_heredoc_is_not_a_source_and_its_lines_are_not_instructions() {
        let text = "RUN <<EOF\nCOPY secret /x\nFOO unknown\nEOF\nCOPY <<-END /etc/f\n\
            \tCOPY more /y\n\tEND\nCOPY real /z\nRUN cat <<'X' <<\"Y\"\nCOPY a b\nX\nCOPY c d\nY\n";
        assert_eq!(ok(text), ["real"]);
        assert_eq!(unread("RUN <<EOF\nCOPY a b\n"), "a heredoc with no end");
    }

    // Docker finds heredocs in the words its shell lexer splits the line into
    // (`heredocsFromLine`), and only after `RUN`, `COPY` or `ADD` in shell form, also as the
    // instruction of an `ONBUILD`. Must catch lines Docker reads as instructions skipped as the
    // body of a heredoc it does not open.
    #[test]
    fn a_double_angle_docker_does_not_read_as_a_heredoc_opens_none() {
        let text = "FROM x\nRUN echo 'use <<EOF here'\nCOPY backend /app\nRUN cat <<EOF >/x\nEOF\n";
        assert_eq!(ok(text), ["backend"]);
        for line in [
            "RUN echo \"a <<EOF\"",
            "RUN echo a\\ <<EOF",
            "RUN [\"sh\", \"-c\", \"cat <<EOF\"]",
            "RUN --network=none [\"cat\", \"<<EOF\"]",
            "ONBUILD ENV note <<EOF",
            "ONBUILD RUN [\"cat\", \"<<EOF\"]",
            "RUN echo 'open <<EOF",
        ] {
            assert_eq!(
                ok(&format!("{line}\nCOPY a /b\nRUN true\n")),
                ["a"],
                "{line}"
            );
        }
        // Its end would be an instruction, read as Docker reads it.
        assert_eq!(
            ok("ONBUILD ENV note <<'RUN true'\nCOPY a /b\nRUN true\n"),
            ["a"]
        );
        // A COPY source is a heredoc only as Docker reads one: `<<<x` is a path.
        assert_eq!(ok("COPY <<<x /d"), ["<<<x"]);
    }

    // What Docker does open: blanks after `<<`, a name with its quotes and escapes removed, more
    // than one heredoc on a line, and one under `ONBUILD`.
    #[test]
    fn a_heredoc_ends_at_its_name_as_docker_lexes_it() {
        assert_eq!(ok("RUN cat << EOF\nCOPY no /x\nEOF\nCOPY a /b\n"), ["a"]);
        assert_eq!(
            ok("RUN cat <<E\"O\"F 3<<'X Y'\nCOPY no /x\nEOF\nCOPY no /y\nX Y\nCOPY a /b\n"),
            ["a"]
        );
        assert_eq!(ok("ONBUILD RUN <<EOF\nCOPY no /x\nEOF\nCOPY a /b\n"), ["a"]);
        assert_eq!(
            ok("COPY <<EOF ${DIR}/f\nCOPY no /x\nEOF\nCOPY a /b\n"),
            ["a"]
        );
        // Docker splits `${a:-b c}` into words of its own, which this reader does not.
        assert_eq!(
            unread("RUN echo ${X:-a <<EOF b}\nCOPY a /b\n"),
            "a variable form this reader does not expand"
        );
    }

    // buildkit's `parseMount` reads the keys and the type in any case, and a mount `from` an empty
    // name is of the context, as is `COPY --from=`. Must catch a bind mount Docker makes read as
    // none.
    #[test]
    fn a_mount_is_read_in_any_case_and_an_empty_from_is_the_context() {
        assert_eq!(
            ok("RUN --mount=type=BIND,source=backend,target=/b make"),
            ["backend"]
        );
        assert_eq!(ok("RUN --mount=TYPE=Bind,SRC=lib,target=/l make"), ["lib"]);
        assert_eq!(ok("RUN --mount=Source=lib,target=/l make"), ["lib"]);
        assert_eq!(
            ok("RUN --mount=type=bind,from=,source=x,target=/x make"),
            ["x"]
        );
        assert_eq!(ok("COPY --from= src /x"), ["src"]);
        assert_eq!(
            ok("RUN --mount=type=CACHE,target=/c --mount=type=bind,FROM=b,target=/s make"),
            Vec::<String>::new()
        );
        assert_eq!(
            unread("RUN --mount=type=${KIND},source=src,target=/s make"),
            "a mount type with a variable"
        );
    }

    // Under `--parents` Docker matches a source with its pattern matcher, where `**` crosses any
    // number of directories; without it, `filepath.Match` reads `**` as one `*`. Must catch `**`
    // read as one segment under `--parents`.
    #[test]
    fn a_double_star_under_parents_leaves_the_dockerfile_unread() {
        for text in [
            "COPY --parents ./src/**/*.py /app/",
            "COPY --parents=TRUE src/a**.py /app/",
        ] {
            assert_eq!(unread(text), "a ** wildcard with --parents", "{text}");
        }
        assert_eq!(ok("COPY src/**/*.py /app/"), ["src/**/*.py"]);
        assert_eq!(
            ok("COPY --parents=false src/**/*.py /app/"),
            ["src/**/*.py"]
        );
        assert_eq!(ok("COPY --parents ./src/*/x.py /app/"), ["src/*/x.py"]);
    }

    #[test]
    fn continuations_comments_and_case_do_not_hide_a_source() {
        let text =
            "copy a \\\n  # a comment inside\n  b \\\n\n  c /dest\n# COPY nope /x\n   Copy  d /e\n";
        assert_eq!(ok(text), ["a", "b", "c", "d"]);
        // A backslash at the end of the file ends the instruction.
        assert_eq!(ok("COPY a /b \\"), ["a"]);
        assert_eq!(ok("\u{feff}COPY a /b"), ["a"]);
    }

    // Docker's parser (buildkit `parser.go`) keeps the leading blanks of a continued line, so
    // the words on either side of the join stay apart.
    #[test]
    fn a_continued_line_keeps_its_leading_blanks_so_two_sources_stay_two() {
        assert_eq!(
            ok("FROM x\nCOPY backend\\\n     frontend ./\n"),
            ["backend", "frontend"]
        );
        assert_eq!(
            ok("COPY backend \\\n\tfrontend ./"),
            ["backend", "frontend"]
        );
    }

    #[test]
    fn a_file_name_cut_by_a_continuation_is_still_two_sources() {
        assert_eq!(
            ok("COPY package.json\\\n     bun.lock ./\n"),
            ["package.json", "bun.lock"]
        );
        // The blanks after the backslash end the line as the backslash does.
        assert_eq!(ok("COPY a.json\\ \t\n  b.lock ./\n"), ["a.json", "b.lock"]);
    }

    // The continuation is a backslash at the end of the line that no backslash precedes
    // (`([^\\])\\[ \t]*$|^\\[ \t]*$`): a line ending in `\\` is complete.
    #[test]
    fn a_line_ending_in_two_backslashes_does_not_continue() {
        assert_eq!(
            ok("FROM x\nRUN echo a\\\\\nCOPY frontend ./f\n"),
            ["frontend"]
        );
        // A backslash alone on its line continues, with nothing to add.
        assert_eq!(ok("COPY a \\\n\\\n  b /d\n"), ["a", "b"]);
        assert_eq!(ok("\\\n  COPY a /d\n"), ["a"]);
        assert_eq!(ok("COPY a /d\n\\"), ["a"]);
    }

    #[test]
    fn sources_are_normalized_from_the_context_root() {
        assert_eq!(ok("COPY /abs/./x//y/ ./ a/../b /d"), ["abs/x/y", ".", "b"]);
        assert_eq!(ok("COPY . ."), ["."]);
        assert_eq!(
            ok("COPY *.json app/*/x?.[ab] /d"),
            ["*.json", "app/*/x?.[ab]"]
        );
    }

    #[test]
    fn a_form_the_reader_does_not_parse_leaves_the_dockerfile_unread() {
        for (text, why) in [
            ("COPY $SRC /x", "a source with a variable"),
            ("COPY ${DIR}/app /x", "a source with a variable"),
            ("COPY src$$ /x", "a source with a variable"),
            (
                "RUN --mount=type=bind,source=$S,target=/x make",
                "a source with a variable",
            ),
            ("COPY ../x /y", "a source outside the build context"),
            ("COPY a/../../x /y", "a source outside the build context"),
            ("COPY \"a b\" /y", "a quoted or escaped source"),
            ("COPY a\\ b /y", "a quoted or escaped source"),
            ("RUN --mount=\"type=bind,source=a\" make", "a quoted flag"),
            (
                "COPY --bogus x /y",
                "a COPY or ADD flag this reader does not know",
            ),
            ("COPY --from=\"a\" x /y", "a quoted flag"),
            ("COPY only", "a COPY or ADD with no destination"),
            ("FOO bar", "an instruction this reader does not know"),
            ("# escape=`\nCOPY a /b", "an escape directive"),
            (
                "# syntax=docker/dockerfile:1\n# escape = `\nCOPY a /b",
                "an escape directive",
            ),
        ] {
            assert_eq!(unread(text), why, "{text}");
        }
        // A directive is only a directive before anything else, and `\` is the default escape.
        assert_eq!(ok("# escape=\\\nCOPY a /b"), ["a"]);
        assert_eq!(ok("FROM x\n# escape=`\nCOPY a /b"), ["a"]);
        assert_eq!(ok("\n# escape=`\nCOPY a /b"), ["a"]);
    }

    #[test]
    fn more_sources_than_the_ceiling_leave_the_dockerfile_unread() {
        let many = |n: usize| {
            (0..n)
                .map(|i| format!("COPY f{i} /x\n"))
                .collect::<String>()
        };
        assert_eq!(ok(&many(PATHS_MAX)).len(), PATHS_MAX);
        assert_eq!(
            unread(&many(PATHS_MAX + 1)),
            "more sources than the reader keeps"
        );
    }

    fn matched(pattern: &str, name: &str) -> bool {
        fits(&compile(pattern).unwrap(), name)
    }

    #[test]
    fn a_wildcard_segment_matches_as_filepath_match_does() {
        for (pattern, name, expected) in [
            ("*.py", "a.py", true),
            ("*.py", ".py", true),
            ("*.py", "a.pyc", false),
            ("a*b*c", "aXbYbc", true),
            ("a*b*c", "aXbYb", false),
            ("?", "a", true),
            ("?", "", false),
            ("?", "ab", false),
            ("[a-c]x", "bx", true),
            ("[a-c]x", "dx", false),
            ("[^a-c]x", "dx", true),
            ("[^a-c]x", "bx", false),
            ("[abc]", "b", true),
            ("v[0-9][0-9]", "v42", true),
            ("v[0-9][0-9]", "v4", false),
            ("**", "anything", true),
            ("é?", "éa", true),
        ] {
            assert_eq!(matched(pattern, name), expected, "{pattern} against {name}");
        }
        for malformed in ["[", "[a", "[]", "[]a]", "[-a]", "[a-]", "[a-", "[^]"] {
            assert!(compile(malformed).is_none(), "{malformed}");
        }
    }

    // A name at the 255-byte cap against five stars: this takes about 20 µs in a local debug
    // build, and a matcher that tries every split at every star (C(255, 5) of them) 112 s.
    #[test]
    fn a_wildcard_costs_at_most_the_name_times_the_pattern() {
        let tokens = compile(&("*a".repeat(5) + "b")).unwrap();
        let name = "a".repeat(255);
        let start = std::time::Instant::now();
        assert!(!fits(&tokens, &name));
        assert!(fits(&tokens, &(name + "b")));
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn a_wildcard_source_gives_the_files_it_matches_and_a_star_stops_at_a_slash() {
        let root = repo(
            "wildcard",
            &[
                "Dockerfile",
                "app/a.py",
                "app/sub/b.py",
                "app/c.txt",
                "x1/package.json",
                "x2/package.json",
                "x3/other.json",
            ],
        );
        let run = |dockerfile: &str| {
            let node = node(".", Some(("Dockerfile", false)));
            std::fs::write(root.join("Dockerfile"), dockerfile).unwrap();
            executions(&root, &node, usize::MAX)
        };
        assert_eq!(paths(&run("COPY app/*.py /x")), ["app/a.py"]);
        assert_eq!(
            paths(&run("COPY app/* /x")),
            ["app/a.py", "app/c.txt", "app/sub"]
        );
        assert_eq!(
            paths(&run("COPY x?/package.json ./")),
            ["x1/package.json", "x2/package.json"]
        );
        assert_eq!(
            paths(&run("COPY */package.json ./")),
            ["x1/package.json", "x2/package.json"]
        );
        assert_eq!(paths(&run("COPY app/*.go /x")), Vec::<&str>::new());
        assert_eq!(paths(&run("COPY a*/*/b.py /x")), ["app/sub/b.py"]);
        // The listings are recorded, so a file that arrives is seen by the refresh.
        let (_, seen) = contract::probed(|| run("COPY app/*.py /x"));
        assert!(seen.reads.contains_key("app/"), "{:?}", seen.reads.keys());
        assert!(seen.reads.contains_key("Dockerfile"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_dockerfile_gives_each_path_it_takes_with_the_build_and_the_file_it_read() {
        let root = repo(
            "exact",
            &[
                "svc/Dockerfile.dev",
                "svc/src/a.py",
                "svc/pkg.json",
                "svc/lib/x.py",
            ],
        );
        std::fs::write(
            root.join("svc/Dockerfile.dev"),
            "FROM x\nCOPY src /app/src\nCOPY pkg.json missing.txt /app/\nRUN --mount=type=bind,source=lib,target=/l make\n",
        )
        .unwrap();
        let codes = executions(
            &root,
            &node("svc", Some(("Dockerfile.dev", false))),
            usize::MAX,
        );
        assert_eq!(paths(&codes), ["svc/lib", "svc/pkg.json", "svc/src"]);
        assert!(
            codes
                .iter()
                .all(|c| !c.inferred && c.address == "services.app")
        );
        assert_eq!(
            codes[0].reason,
            "Compose build compose.yaml#services.app.build; Dockerfile svc/Dockerfile.dev"
        );
        // A path the Dockerfile climbs out of its context for stays in the repository.
        let codes = executions(
            &root,
            &node("svc/src", Some(("../Dockerfile.dev", false))),
            usize::MAX,
        );
        assert_eq!(paths(&codes), Vec::<&str>::new());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_inline_dockerfile_is_read_and_a_dockerfile_taking_nothing_runs_no_code() {
        let root = repo("inline", &["src/a.py", "Dockerfile"]);
        let inline = |text: &str| executions(&root, &node(".", Some((text, true))), usize::MAX);
        let codes = inline("FROM x\nCOPY src /app\n");
        assert_eq!(paths(&codes), ["src"]);
        assert!(
            codes[0].reason.ends_with("; Dockerfile dockerfile_inline"),
            "{}",
            codes[0].reason
        );
        assert_eq!(
            paths(&inline("FROM scratch\nCOPY --from=build /a /b\n")),
            Vec::<&str>::new()
        );
        assert_eq!(paths(&inline("FROM scratch\n")), Vec::<&str>::new());
        std::fs::write(root.join("Dockerfile"), "FROM scratch\nRUN make\n").unwrap();
        let codes = executions(&root, &node(".", None), usize::MAX);
        assert_eq!(paths(&codes), Vec::<&str>::new());
        assert!(
            inline("   \n")[0]
                .reason
                .contains("not read: an empty dockerfile_inline")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_dockerfile_not_read_leaves_the_whole_context_inferred_and_says_why() {
        let root = repo(
            "unread",
            &["src/a.py", "svc/Dockerfile", "svc/real/Dockerfile"],
        );
        std::fs::write(root.join("svc/Dockerfile"), "COPY $DIR /x\n").unwrap();
        for (context, dockerfile, why) in [
            ("src", None, "missing"),
            ("svc", None, "a source with a variable"),
            (
                ".",
                Some("/etc/Dockerfile"),
                "a path this reader does not follow",
            ),
            (".", Some("${DF}"), "a path this reader does not follow"),
            (".", Some("../Dockerfile"), "a path outside the repository"),
            (".", Some("svc"), "not a file"),
            (".", Some(""), "a path this reader does not follow"),
        ] {
            let codes = executions(
                &root,
                &node(context, dockerfile.map(|d| (d, false))),
                usize::MAX,
            );
            assert_eq!(paths(&codes), [context], "{context} {dockerfile:?}");
            assert!(codes[0].inferred, "{why}");
            let reason = &codes[0].reason;
            assert!(
                reason.starts_with("Compose build compose.yaml#services.app.build; Dockerfile "),
                "{reason}"
            );
            assert!(reason.ends_with(&format!(" not read: {why}")), "{reason}");
        }
        std::fs::write(root.join("svc/real/Dockerfile"), [0xff, 0xfe]).unwrap();
        let codes = executions(&root, &node("svc/real", None), usize::MAX);
        assert!(
            codes[0]
                .reason
                .ends_with("not read: unreadable, too large or not UTF-8")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_in_the_dockerfile_or_its_sources_is_not_followed() {
        let root = repo("links", &["real/Dockerfile", "real/a.py", "svc/x"]);
        std::os::unix::fs::symlink("../real/Dockerfile", root.join("svc/Dockerfile")).unwrap();
        let codes = executions(&root, &node("svc", None), usize::MAX);
        assert!(
            codes[0].reason.ends_with("not read: a symbolic link"),
            "{}",
            codes[0].reason
        );
        assert_eq!(paths(&codes), ["svc"]);
        // A source behind a link, named or matched by a wildcard.
        std::fs::remove_file(root.join("svc/Dockerfile")).unwrap();
        std::os::unix::fs::symlink("../real", root.join("svc/link")).unwrap();
        for copy in [
            "COPY link /x",
            "COPY link/a.py /x",
            "COPY l*k /x",
            "COPY * /x",
        ] {
            std::fs::write(root.join("svc/Dockerfile"), copy).unwrap();
            let codes = executions(&root, &node("svc", None), usize::MAX);
            assert!(
                codes[0].reason.ends_with("not read: a symbolic link"),
                "{copy}: {}",
                codes[0].reason
            );
        }
        // A context that is a link runs nothing.
        std::os::unix::fs::symlink("real", root.join("ctx")).unwrap();
        assert!(executions(&root, &node("ctx", None), usize::MAX).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_overridable_context_or_dockerfile_keeps_its_paths_inferred() {
        let root = repo("levels", &["src/a.py", "Dockerfile"]);
        std::fs::write(root.join("Dockerfile"), "COPY src /x\n").unwrap();
        let mut service = node(".", None);
        assert!(!executions(&root, &service, usize::MAX)[0].inferred);
        service.build.as_mut().unwrap().overridable = true;
        assert!(executions(&root, &service, usize::MAX)[0].inferred);
        let mut service = node(".", Some(("Dockerfile", false)));
        service.dockerfile.as_mut().unwrap().overridable = true;
        assert!(executions(&root, &service, usize::MAX)[0].inferred);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn paths_past_the_ceiling_or_the_room_leave_the_dockerfile_unread() {
        let many = |n: usize| {
            (0..n)
                .map(|i| format!("COPY f{i} /x\n"))
                .collect::<String>()
        };
        let files: Vec<String> = (0..=PATHS_MAX).map(|i| format!("f{i}")).collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let root = repo("ceiling", &files);
        let run = |n| executions(&root, &node(".", Some((&many(n), true))), usize::MAX);
        assert_eq!(run(PATHS_MAX).len(), PATHS_MAX);
        assert_eq!(paths(&run(PATHS_MAX + 1)), ["."]);
        // A wildcard past the ceiling too.
        let codes = executions(&root, &node(".", Some(("COPY f* /x", true))), usize::MAX);
        assert_eq!(paths(&codes), ["."]);
        assert!(
            codes[0]
                .reason
                .ends_with("not read: more paths than the reader keeps")
        );
        // Room for the whole context only, then for nothing.
        let service = node(".", Some(("COPY f1 f2 f3 /x", true)));
        let whole = executions(&root, &service, 200);
        assert_eq!(paths(&whole), ["."]);
        assert!(
            whole[0]
                .reason
                .ends_with("more stored paths than the summary has room for")
        );
        assert!(executions(&root, &service, 10).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_wildcard_examines_a_bounded_number_of_entries() {
        let dirs = 128;
        let per = WORK_MAX / dirs + 2;
        let files: Vec<String> = (0..dirs)
            .flat_map(|d| (0..per).map(move |f| format!("d{d}/f{f}")))
            .collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let root = repo("work", &files);
        let start = std::time::Instant::now();
        let codes = executions(
            &root,
            &node(".", Some(("COPY */*.none /x", true))),
            usize::MAX,
        );
        assert!(
            codes[0]
                .reason
                .ends_with("not read: more files than the reader examines"),
            "{}",
            codes[0].reason
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_directory_past_the_listing_ceiling_is_not_listed() {
        let files: Vec<String> = (0..=contract::LIST_MAX)
            .map(|i| format!("big/f{i}"))
            .collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let root = repo("list", &files);
        let codes = executions(
            &root,
            &node(".", Some(("COPY big/*.none /x", true))),
            usize::MAX,
        );
        assert!(
            codes[0]
                .reason
                .ends_with("not read: a directory too large to list"),
            "{}",
            codes[0].reason
        );
        std::fs::remove_file(root.join("big/f0")).unwrap();
        let codes = executions(
            &root,
            &node(".", Some(("COPY big/*.none /x", true))),
            usize::MAX,
        );
        assert!(codes.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
