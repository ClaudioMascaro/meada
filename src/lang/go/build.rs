//! Build constraints, evaluated for one fixed target so that the index never depends on the
//! machine: `linux/amd64`, the `gc` compiler, every `go1.N` release and no cgo. A file is left
//! out when its name ends in another system or architecture (`_windows.go`, `_arm64.go`), when it
//! starts with `_` or `.`, or when its `//go:build` line is false. Any other tag (`cgo`, a custom
//! tag such as `integration`) is false.

/// How deeply a `//go:build` expression may nest (`!`, parentheses); a deeper one is not read,
/// and the file is left out.
// ponytail: real constraints nest two or three levels; the limit keeps a crafted line of
// thousands of parentheses from overflowing the stack.
pub const CONSTRAINT_DEPTH: usize = 64;

const OS: &str = "linux";
const ARCH: &str = "amd64";

/// The systems and architectures a file name suffix may name.
const SYSTEMS: &[&str] = &[
    "aix",
    "android",
    "darwin",
    "dragonfly",
    "freebsd",
    "hurd",
    "illumos",
    "ios",
    "js",
    "linux",
    "nacl",
    "netbsd",
    "openbsd",
    "plan9",
    "solaris",
    "wasip1",
    "windows",
    "zos",
];
const ARCHITECTURES: &[&str] = &[
    "386",
    "amd64",
    "amd64p32",
    "arm",
    "armbe",
    "arm64",
    "arm64be",
    "loong64",
    "mips",
    "mipsle",
    "mips64",
    "mips64le",
    "mips64p32",
    "mips64p32le",
    "ppc",
    "ppc64",
    "ppc64le",
    "riscv",
    "riscv64",
    "s390",
    "s390x",
    "sparc",
    "sparc64",
    "wasm",
];

/// Whether the target builds the file at `path` holding `src`.
pub fn builds(path: &str, src: &[u8]) -> bool {
    named(path)
        && match header(src) {
            Header::None => true,
            Header::One(expression) => holds(expression),
            Header::Unreadable => false,
        }
}

/// The `//go:build` line of a file's header.
enum Header<'s> {
    None,
    One(&'s str),
    /// Two lines, or one that is not UTF-8: the Go tools reject the file.
    Unreadable,
}

/// Whether the file name lets the target build it.
fn named(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    if file.starts_with('_') || file.starts_with('.') {
        return false;
    }
    let stem = file.strip_suffix(".go").unwrap_or(file);
    // What precedes the first `_` is the name itself.
    let Some((_, tail)) = stem.split_once('_') else {
        return true;
    };
    let mut parts: Vec<&str> = tail.split('_').collect();
    if parts.last() == Some(&"test") {
        parts.pop();
    }
    match parts[..] {
        [.., os, arch] if SYSTEMS.contains(&os) && ARCHITECTURES.contains(&arch) => {
            os == OS && arch == ARCH
        }
        [.., last] if SYSTEMS.contains(&last) || ARCHITECTURES.contains(&last) => {
            last == OS || last == ARCH
        }
        _ => true,
    }
}

/// The `//go:build` line of the file's header, read as the Go tools read it: the lines before
/// the first that holds code (blank lines, `//` comments, `/* */` comments), a directive inside a
/// block comment not counting.
fn header(src: &[u8]) -> Header<'_> {
    let mut found = Header::None;
    let mut in_block = false;
    'lines: for line in src.split(|b| *b == b'\n') {
        let mut line = line.trim_ascii();
        if !in_block && let Some(expression) = directive(line) {
            if !matches!(found, Header::None) {
                return Header::Unreadable;
            }
            found = match std::str::from_utf8(expression) {
                Ok(expression) => Header::One(expression.trim()),
                Err(_) => return Header::Unreadable,
            };
        }
        while !line.is_empty() {
            if in_block {
                let Some(end) = line.windows(2).position(|w| w == b"*/") else {
                    continue 'lines;
                };
                in_block = false;
                line = line[end + 2..].trim_ascii();
            } else if line.starts_with(b"//") {
                continue 'lines;
            } else if let Some(rest) = line.strip_prefix(b"/*") {
                in_block = true;
                line = rest.trim_ascii();
            } else {
                break 'lines;
            }
        }
    }
    found
}

/// The expression of a `//go:build` line.
fn directive(line: &[u8]) -> Option<&[u8]> {
    let expression = line.strip_prefix(b"//go:build")?;
    (expression.is_empty() || expression.starts_with(b" ") || expression.starts_with(b"\t"))
        .then_some(expression)
}

/// Whether a constraint expression holds for the target. One that cannot be read does not.
fn holds(expression: &str) -> bool {
    let tokens = tokens(expression);
    let mut at = 0;
    match or(&tokens, &mut at, 0) {
        Some(value) if at == tokens.len() => value,
        _ => false,
    }
}

fn tokens(expression: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = expression.trim_start();
    while !rest.is_empty() {
        let len = match rest.as_bytes()[0] {
            b'(' | b')' | b'!' => 1,
            b'&' | b'|' => 2,
            _ => rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                .unwrap_or(rest.len())
                .max(1),
        };
        let len = len.min(rest.len());
        let Some(token) = rest.get(..len) else {
            return vec!["?"];
        };
        out.push(token);
        rest = rest[len..].trim_start();
    }
    out
}

fn or(tokens: &[&str], at: &mut usize, depth: usize) -> Option<bool> {
    let mut value = and(tokens, at, depth)?;
    while tokens.get(*at) == Some(&"||") {
        *at += 1;
        value |= and(tokens, at, depth)?;
    }
    Some(value)
}

fn and(tokens: &[&str], at: &mut usize, depth: usize) -> Option<bool> {
    let mut value = not(tokens, at, depth)?;
    while tokens.get(*at) == Some(&"&&") {
        *at += 1;
        value &= not(tokens, at, depth)?;
    }
    Some(value)
}

fn not(tokens: &[&str], at: &mut usize, depth: usize) -> Option<bool> {
    if depth >= CONSTRAINT_DEPTH {
        return None;
    }
    let token = *tokens.get(*at)?;
    *at += 1;
    match token {
        "!" => not(tokens, at, depth + 1).map(|v| !v),
        "(" => {
            let value = or(tokens, at, depth + 1)?;
            (tokens.get(*at) == Some(&")")).then(|| *at += 1)?;
            Some(value)
        }
        tag if tag.starts_with(|c: char| c.is_alphanumeric() || c == '_') => Some(tag_holds(tag)),
        _ => None,
    }
}

fn tag_holds(tag: &str) -> bool {
    let release = tag
        .strip_prefix("go1.")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    matches!(tag, OS | ARCH | "unix" | "gc") || release
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_name_suffix_names_the_target_or_another() {
        let cases = [
            ("store/store.go", true),
            ("store/store_test.go", true),
            ("store/disk_linux.go", true),
            ("store/disk_amd64.go", true),
            ("store/disk_linux_amd64_test.go", true),
            ("store/disk_windows.go", false),
            ("store/disk_arm64.go", false),
            ("store/disk_linux_arm64.go", false),
            ("store/disk_android.go", false),
            ("store/disk_windows_test.go", false),
            // The name before the first `_` is no suffix.
            ("store/windows.go", true),
            ("store/_draft.go", false),
            ("store/.hidden.go", false),
        ];
        for (path, built) in cases {
            assert_eq!(named(path), built, "{path}");
        }
    }

    #[test]
    fn a_go_build_line_is_evaluated_for_the_target() {
        let cases = [
            ("linux", true),
            ("linux && amd64", true),
            ("linux && (amd64 || arm64) && !cgo", true),
            ("unix && gc && go1.21", true),
            ("!windows", true),
            ("ignore", false),
            ("cgo", false),
            ("integration", false),
            ("darwin || windows", false),
            ("!linux", false),
            ("go1.x", false),
            // A line that cannot be read holds for nothing.
            ("linux &&", false),
            ("(linux", false),
            ("linux)", false),
            ("linux & amd64", false),
            ("", false),
        ];
        for (expression, built) in cases {
            assert_eq!(holds(expression), built, "{expression}");
        }
    }

    #[test]
    fn the_go_build_line_counts_in_the_header_alone() {
        let built = |src: &str| builds("x.go", src.as_bytes());
        assert!(!built("//go:build ignore\n\npackage x\n"));
        assert!(!built(
            "// Copyright\n\n//go:build windows\n\n// Package x.\npackage x\n"
        ));
        assert!(built("//go:build linux\n\npackage x\n"));
        assert!(built("package x\n"));
        // Right above the package clause, it still counts.
        assert!(!built("//go:build ignore\npackage x\n"));
        // After a block comment it counts; inside one it does not.
        assert!(!built("/* License */\n//go:build windows\npackage x\n"));
        assert!(!built(
            "/*\n * License\n */\n\n//go:build windows\n\npackage x\n"
        ));
        assert!(built("/*\n//go:build windows\n*/\npackage x\n"));
        assert!(!built("/* a */ /* b */\n//go:build ignore\npackage x\n"));
        // After the package clause, it is any comment.
        assert!(built("package x\n\n//go:build ignore\n\n"));
        assert!(built("package x /* a */\n//go:build ignore\n"));
        // Two lines: the Go tools reject the file.
        assert!(!built("//go:build linux\n//go:build amd64\n\npackage x\n"));
        assert!(built("//go:buildx windows\npackage x\n"));
    }

    #[test]
    fn a_constraint_nested_deeper_than_the_limit_is_not_read() {
        let nested = |n: usize| format!("{}linux{}", "(".repeat(n), ")".repeat(n));
        assert!(holds(&nested(CONSTRAINT_DEPTH - 1)));
        assert!(!holds(&nested(CONSTRAINT_DEPTH)));
        assert!(!holds(&format!("{}linux", "!".repeat(100_000))));
    }
}
