//! Hostile input (`CODING_STANDARDS.md`): a crafted file ends as a status line or an open part,
//! and indexing carries on. Each limit has a unit test of its own; this subject is the net for
//! the one nobody wrote. Per language pack, pathological repositories sized past the limits of
//! "Known ceilings" are indexed on a thread with a small stack, joined with a wall-clock
//! budget: a recursion with no limit overflows the stack (the process aborts: with
//! `--nocapture`, the case named with no time after it is the one), a loop with none misses
//! the budget.
//!
//! tree-sitter parses without recursion, so its depth is no limit here; the walks over its trees
//! are Meada's. Real code nests about 30 deep.

use crate::common;

use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use meada::engine::Engine;

/// The stack of the indexing thread, a test thread's: the fixtures of every pack index in
/// 128 KiB in debug, and the deepest case here, at the limits, in half of it.
const STACK: usize = 2 << 20;

/// The wall-clock budget of one case in debug. A case alone takes 3 s at most, but beside every
/// other subject, on a loaded machine under `bounded`'s memory cap, one of 100 ms took over
/// 10 s: a loop with no limit misses any budget all the same.
const BUDGET: Duration = Duration::from_secs(60);

/// Nesting and chains within one file, past every depth limit (`PATH_DEPTH`, `FOLD_DEPTH`,
/// `EVAL_DEPTH` and `PROCESS_DEPTH` are 256, the others less) by enough that a recursion with no
/// limit at all needs more than `STACK`.
const DEEP: usize = 1000;
/// Python nesting by indentation, past `DEFINITION_DEPTH` (128): its file grows with the square
/// of the depth.
const INDENTED: usize = 300;
/// Past `FLUENT_CHAIN` (1024).
const LONG: usize = 1100;
/// Past `USE_BINDINGS` and `FEATURES` (4096).
const WIDE: usize = 5000;
/// Files in a chain, past `LINK_DEPTH` and `INCLUDE_DEPTH` (64) and `FAMILY_DEPTH` (128).
const LINKS: usize = 130;

/// A case: its name and the files of the repository, relative to its root.
type Case = (&'static str, Vec<(String, String)>);

fn one(path: &str, body: String) -> Vec<(String, String)> {
    vec![(path.into(), body)]
}

/// `open` `n` times, `core`, `close` `n` times.
fn nest(open: &str, core: &str, close: &str, n: usize) -> String {
    format!("{}{core}{}", open.repeat(n), close.repeat(n))
}

/// `f(0)`, `f(1)`, ... `f(n - 1)`, one per line.
fn lines(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(f).collect::<Vec<_>>().join("\n")
}

/// The same files as `lines`, one file each: `path(i)` holds `body(i)`.
fn chain(
    n: usize,
    path: impl Fn(usize) -> String,
    body: impl Fn(usize) -> String,
) -> Vec<(String, String)> {
    (0..n).map(|i| (path(i), body(i))).collect()
}

/// Indexes `files` as a repository and exports it, on a thread with `STACK` bytes, then removes
/// the repository (a failed case's stays to be read).
fn index(pack: &str, name: &str, files: &[(String, String)]) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("hostile-{pack}-{name}"));
    let _ = fs::remove_dir_all(&dir);
    for (path, body) in files {
        let at = dir.join(path);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, body).unwrap();
    }
    common::git(&dir, &["-c", "init.defaultBranch=main", "init", "-q"]);
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    eprintln!("hostile {pack}/{name}");
    let repo = dir.clone();
    let worker = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            let _ = tx.send(
                Engine::open(&repo)
                    .and_then(|e| e.export())
                    .map(|e| e.len())
                    .map_err(|e| e.to_string()),
            );
        })
        .unwrap();
    let done = rx
        .recv_timeout(BUDGET)
        .unwrap_or_else(|_| panic!("{pack}/{name}: no end within {BUDGET:?}"));
    done.unwrap_or_else(|e| panic!("{pack}/{name}: {e}"));
    worker
        .join()
        .unwrap_or_else(|_| panic!("{pack}/{name}: panicked"));
    eprintln!("hostile {pack}/{name}: {:?}", started.elapsed());
    fs::remove_dir_all(&dir).unwrap();
}

/// Each pack's cases are split into `PARTS` tests, a case in every `PARTS`th, so the harness
/// indexes them in parallel with the other subjects instead of after them.
const PARTS: usize = 4;

/// The cases of `pack` that fall to `part`.
fn run(pack: &str, cases: Vec<Case>, part: usize) {
    for (name, files) in cases.into_iter().skip(part).step_by(PARTS) {
        index(pack, name, &files);
    }
}

fn ts() -> Vec<Case> {
    let t = |name, body: String| (name, one("src/a.ts", body));
    let x = |body: String| format!("function f(a) {{ return a; }}\nexport const x = {body};\n");
    let express = "import express from 'express';\nconst h = (req, res) => res.send('a');\n";
    vec![
        t("parens", x(nest("(", "1", ")", DEEP))),
        t("arrays", x(nest("[", "1", "]", DEEP))),
        t("objects", x(nest("{a:", "1", "}", DEEP))),
        t("calls", x(nest("f(", "1", ")", DEEP))),
        t("arrows", x(nest("() => ", "1", "", DEEP))),
        t("ternaries", x(nest("a ? 1 : ", "1", "", DEEP))),
        t("templates", x(nest("`${", "1", "}`", DEEP))),
        t("binary", x(vec!["'a'"; 5 * LONG].join(" + "))),
        t("members", x(format!("a{}", ".b".repeat(5 * LONG)))),
        t("method-chain", x(format!("a{}", ".m(1)".repeat(5 * LONG)))),
        t(
            "blocks",
            format!("export function f() {{ {} }}\n", nest("{", "", "}", DEEP)),
        ),
        t(
            "functions",
            format!("export {}", nest("function f() { ", "", " }", DEEP)),
        ),
        t(
            "classes",
            format!(
                "export {}",
                nest("class C { m() { return ", "1", "; } }", DEEP)
            ),
        ),
        t(
            "types",
            format!("export type T = {};\n", nest("Array<", "number", ">", DEEP)),
        ),
        t(
            "fn-types",
            format!(
                "export type T = {};\n",
                nest("(a: ", "number", ") => void", DEEP)
            ),
        ),
        t(
            "aliases",
            format!(
                "const a0 = 1;\n{}\nexport const y = a{LONG};\n",
                lines(LONG, |i| format!("const a{} = a{i};", i + 1))
            ),
        ),
        t(
            "args",
            format!(
                "function f() {{}}\nf({});\n",
                vec!["1"; 4 * WIDE].join(", ")
            ),
        ),
        t(
            "template-holes",
            format!(
                "const a = 1;\nexport const s = `{}`;\n",
                "${a}".repeat(WIDE)
            ),
        ),
        t(
            "decorators",
            format!(
                "export class C {{\n{}\n}}\n",
                lines(LONG, |i| format!("@d() m{i}() {{}}"))
            ),
        ),
        t(
            "decorator-stack",
            format!("export class C {{\n{} m() {{}}\n}}\n", "@d() ".repeat(WIDE)),
        ),
        t(
            "bindings",
            format!(
                "export function f() {{\n{}\n}}\n",
                lines(WIDE, |_| "{ let x = f(); x.m(); }".into())
            ),
        ),
        t(
            "redeclared",
            format!(
                "var x = f();\n{}\nexport default x;\n",
                lines(WIDE, |_| "var x = x + 1;".into())
            ),
        ),
        t(
            "doubling",
            format!(
                "const s0 = 'a';\n{}\nfetch(s40);\n",
                lines(40, |i| format!("const s{} = s{i} + s{i};", i + 1))
            ),
        ),
        t(
            "doubling-templates",
            format!(
                "const s0 = '/a';\n{}\nfetch(s40);\n",
                lines(40, |i| format!("const s{} = `${{s{i}}}${{s{i}}}`;", i + 1))
            ),
        ),
        t(
            "const-chain",
            format!(
                "const a0 = '/a';\n{}\nfetch(a{DEEP});\n",
                lines(DEEP, |i| format!("const a{} = a{i};", i + 1))
            ),
        ),
        t(
            "const-nesting",
            format!(
                "const c0 = '/a';\n{}\nfetch(c{DEEP});\n",
                lines(DEEP, |i| format!(
                    "const c{} = {};",
                    i + 1,
                    nest("[", &format!("c{i}"), "]", 60)
                ))
            ),
        ),
        t(
            "wrapper-chain",
            format!(
                "function w0(u) {{ return fetch(u); }}\n{}\nw{DEEP}('/a');\n",
                lines(DEEP, |i| format!(
                    "function w{}(u) {{ return w{i}(u); }}",
                    i + 1
                ))
            ),
        ),
        t(
            "wrapper-lattice",
            format!(
                "function w0(u) {{ return fetch(u); }}\n{}\nw40('/a');\n",
                lines(40, |i| format!(
                    "function w{}(u) {{ w{i}(u); return w{i}(u); }}",
                    i + 1
                ))
            ),
        ),
        t(
            "default-cycle",
            "function f(a = b, b = a) { return fetch(a); }\nf();\n".into(),
        ),
        t(
            "returns-chain",
            format!(
                "import {{ Kafka }} from 'kafkajs';\nfunction k0() {{ return new Kafka({{}}).producer(); }}\n{}\nconst p = k{DEEP}();\np.send({{ topic: 'a' }});\n",
                lines(DEEP, |i| format!(
                    "function k{}() {{ return k{i}(); }}",
                    i + 1
                ))
            ),
        ),
        t(
            "express-chain",
            format!(
                "{express}const r = express.Router();\nr{}\n",
                ".get('/a', h)".repeat(LONG)
            ),
        ),
        t(
            "express-mounts",
            format!(
                "{express}{}\nexport default r0;\n",
                lines(DEEP, |i| format!(
                    "const r{i} = express.Router();\nr{i}.get('/a', h);\n{}",
                    if i > 0 {
                        format!("r{}.use('/p', r{i});", i - 1)
                    } else {
                        String::new()
                    }
                ))
            ),
        ),
        t(
            "fetch-chain",
            format!("fetch('/a'){}\n", ".then(r => r)".repeat(LONG)),
        ),
        (
            "reexport-chain",
            chain(
                LINKS,
                |i| format!("src/p{i}.ts"),
                |i| match i + 1 == LINKS {
                    true => "export function x() {}\n".into(),
                    false => format!(
                        "export * from './p{}';\nexport {{ x as y{i} }} from './p{}';\n",
                        i + 1,
                        i + 1
                    ),
                },
            ),
        ),
        (
            "import-chain",
            chain(
                LINKS,
                |i| format!("src/p{i}.ts"),
                |i| match i + 1 == LINKS {
                    true => "export function x() {}\n".into(),
                    false => format!(
                        "import {{ x }} from './p{}';\nexport {{ x }};\nexport const v = x();\n",
                        i + 1
                    ),
                },
            ),
        ),
        (
            "manifest-wide",
            one(
                "package.json",
                format!(
                    "{{\"name\":\"a\",\"dependencies\":{{{}}}}}",
                    (0..WIDE)
                        .map(|i| format!("\"d{i}\":\"^1.0.0\""))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
        ),
        (
            "manifest-large",
            one(
                "package.json",
                format!(
                    "{{\"name\":\"a\",\"description\":\"{}\"}}",
                    "x".repeat(1 << 20)
                ),
            ),
        ),
        (
            "manifest-deep",
            one(
                "package.json",
                format!(
                    "{{\"name\":\"a\",\"exports\":{}}}",
                    nest("{\"import\":", "\"./a.js\"", "}", WIDE)
                ),
            ),
        ),
        (
            "tsconfig-paths",
            one(
                "tsconfig.json",
                format!(
                    "{{\"compilerOptions\":{{\"paths\":{{{}}}}}}}",
                    (0..WIDE)
                        .map(|i| format!("\"@p{i}/*\":[\"src/p{i}/*\"]"))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
        ),
    ]
}

fn python() -> Vec<Case> {
    let p = |name, body: String| (name, one("a.py", body));
    let x = |body: String| format!("def f(a):\n    return a\n\nx = {body}\n");
    let flask = "from flask import Flask, Blueprint\napp = Flask(__name__)\n";
    let indented = |line: &str| {
        (0..INDENTED)
            .map(|i| format!("{}{line}\n", " ".repeat(i)))
            .collect::<String>()
            + &format!("{}pass\n", " ".repeat(INDENTED))
    };
    vec![
        p("parens", x(nest("(", "1", ")", DEEP))),
        p("lists", x(nest("[", "1", "]", DEEP))),
        p("dicts", x(nest("{'a': ", "1", "}", DEEP))),
        p("calls", x(nest("f(", "1", ")", DEEP))),
        p("lambdas", x(nest("lambda: ", "1", "", DEEP))),
        p("ternaries", x(nest("1 if a else ", "1", "", DEEP))),
        p("fstrings", x(nest("f'{", "1", "}'", DEEP / 10))),
        p("binary", x(vec!["'a'"; 5 * LONG].join(" + "))),
        p("attributes", x(format!("a{}", ".b".repeat(5 * LONG)))),
        p("method-chain", x(format!("a{}", ".m(1)".repeat(5 * LONG)))),
        p("subscripts", x(format!("a{}", "[0]".repeat(5 * LONG)))),
        p("blocks", indented("if a:")),
        p("defs", indented("def f():")),
        p("classes", indented("class C:")),
        p(
            "annotations",
            format!("x: {} = None\n", nest("List[", "int", "]", DEEP)),
        ),
        p(
            "family",
            format!(
                "class C0:\n    def m(self): pass\n{}\nC{DEEP}().m()\n",
                lines(DEEP, |i| format!("class C{}(C{i}): pass", i + 1))
            ),
        ),
        p(
            "receivers",
            format!(
                "class C:\n    def m(self): return self\n{}\n",
                lines(DEEP, |i| format!(
                    "def f{i}(a{i}: C):\n    b = a{i}.m()\n    return b.m().m()"
                ))
            ),
        ),
        p(
            "aliases",
            format!(
                "a0 = 1\n{}\ny = a{WIDE}\n",
                lines(WIDE, |i| format!("a{} = a{i}", i + 1))
            ),
        ),
        p(
            "args",
            format!("def f(*a): pass\nf({})\n", vec!["1"; 4 * WIDE].join(", ")),
        ),
        p(
            "keywords",
            format!(
                "def f(**k): pass\nf({})\n",
                (0..WIDE)
                    .map(|i| format!("k{i}=1"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        p(
            "fstring-holes",
            format!("a = 1\ns = f'{}'\n", "{a}".repeat(WIDE)),
        ),
        p(
            "decorators",
            lines(WIDE, |i| format!("@d\ndef f{i}(): pass")),
        ),
        p(
            "decorator-stack",
            format!("{}def f(): pass\n", "@d\n".repeat(WIDE)),
        ),
        p(
            "bindings",
            format!(
                "def f():\n{}\n",
                lines(WIDE, |_| "    x = f()\n    x.m()".into())
            ),
        ),
        p(
            "doubling",
            format!(
                "s0 = 'a'\n{}\nimport requests\nrequests.get(s40)\n",
                lines(40, |i| format!("s{} = s{i} + s{i}", i + 1))
            ),
        ),
        p(
            "doubling-fstrings",
            format!(
                "s0 = '/a'\n{}\nimport requests\nrequests.get(s40)\n",
                lines(40, |i| format!("s{} = f'{{s{i}}}{{s{i}}}'", i + 1))
            ),
        ),
        p(
            "const-chain",
            format!(
                "import requests\na0 = '/a'\n{}\nrequests.get(a{DEEP})\n",
                lines(DEEP, |i| format!("a{} = a{i}", i + 1))
            ),
        ),
        p(
            "const-nesting",
            format!(
                "import requests\nc0 = '/a'\n{}\nrequests.get(c{DEEP})\n",
                lines(DEEP, |i| format!(
                    "c{} = {}",
                    i + 1,
                    nest("[", &format!("c{i}"), "]", 60)
                ))
            ),
        ),
        p(
            "wrapper-chain",
            format!(
                "import requests\ndef w0(u):\n    return requests.get(u)\n{}\nw{DEEP}('/a')\n",
                lines(DEEP, |i| format!("def w{}(u):\n    return w{i}(u)", i + 1))
            ),
        ),
        p(
            "wrapper-lattice",
            format!(
                "import requests\ndef w0(u):\n    return requests.get(u)\n{}\nw40('/a')\n",
                lines(40, |i| format!(
                    "def w{}(u):\n    w{i}(u)\n    return w{i}(u)",
                    i + 1
                ))
            ),
        ),
        p(
            "returns-chain",
            format!(
                "from kafka import KafkaProducer\ndef k0():\n    return KafkaProducer()\n{}\np = k{DEEP}()\np.send('a', b'x')\n",
                lines(DEEP, |i| format!("def k{}():\n    return k{i}()", i + 1))
            ),
        ),
        p(
            "returns-lattice",
            format!(
                "from kafka import KafkaProducer\ndef k0():\n    return KafkaProducer()\n{}\np = k40()\np.send('a', b'x')\n",
                lines(40, |i| format!(
                    "def k{}():\n    if c:\n        return k{i}()\n    return k{i}()",
                    i + 1
                ))
            ),
        ),
        p(
            "param-lattice",
            format!(
                "import requests\ndef p0(s):\n    s.get('/a')\n{}\np40(requests.Session())\n",
                lines(40, |i| format!(
                    "def p{}(s):\n    p{i}(s)\n    p{i}(s)",
                    i + 1
                ))
            ),
        ),
        p(
            "param-chain",
            format!(
                "import requests\ndef p0(s):\n    s.get('/a')\n{}\np{DEEP}(requests.Session())\n",
                lines(DEEP, |i| format!("def p{}(s):\n    p{i}(s)", i + 1))
            ),
        ),
        p(
            "flask-chain",
            format!(
                "{flask}@app.route('/a')\ndef h(): pass\napp.add_url_rule('/a', view_func=h){}\n",
                ".x".repeat(LONG)
            ),
        ),
        p(
            "flask-blueprints",
            format!(
                "{flask}{}\napp.register_blueprint(b0)\n",
                lines(DEEP, |i| format!(
                    "b{i} = Blueprint('b{i}', __name__)\n@b{i}.route('/a')\ndef h{i}(): pass\n{}",
                    if i > 0 {
                        format!("b{}.register_blueprint(b{i}, url_prefix='/p')", i - 1)
                    } else {
                        String::new()
                    }
                ))
            ),
        ),
        (
            "reexport-chain",
            chain(
                DEEP,
                |i| format!("p{i}.py"),
                |i| match i + 1 == DEEP {
                    true => "def x(): pass\n".into(),
                    false => format!(
                        "from p{} import *\nfrom p{} import x as y{i}\n",
                        i + 1,
                        i + 1
                    ),
                },
            ),
        ),
        (
            "import-chain",
            chain(
                LINKS,
                |i| format!("p{i}.py"),
                |i| match i + 1 == LINKS {
                    true => "def x(): pass\n".into(),
                    false => format!("from p{} import x\nv = x()\n", i + 1),
                },
            ),
        ),
        (
            "urlconf-chain",
            chain(
                LINKS,
                |i| format!("p{i}/urls.py"),
                |i| {
                    format!(
                        "from django.urls import path, include\nfrom . import views\nurlpatterns = [path('a/', views.h), path('p/', include('p{}.urls'))]\n",
                        i + 1
                    )
                },
            ),
        ),
        (
            "pyproject-wide",
            one(
                "pyproject.toml",
                format!(
                    "[project]\nname = \"a\"\ndependencies = [{}]\n",
                    (0..WIDE)
                        .map(|i| format!("\"d{i}>=1\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
        ),
        (
            "pyproject-large",
            one(
                "pyproject.toml",
                format!("[project]\nname = \"a\"\n#{}", "x".repeat(1 << 20)),
            ),
        ),
        (
            "pyproject-deep",
            one(
                "pyproject.toml",
                format!(
                    "[project]\nname = \"a\"\n[tool]\na = {}\n",
                    nest("{a = ", "1", "}", WIDE)
                ),
            ),
        ),
        (
            "requirements-wide",
            one("requirements.txt", lines(4 * WIDE, |i| format!("d{i}>=1"))),
        ),
        (
            "requirements-includes",
            chain(
                LINKS,
                |i| {
                    format!(
                        "requirements{}.txt",
                        if i == 0 {
                            String::new()
                        } else {
                            format!("-{i}")
                        }
                    )
                },
                |i| format!("-r requirements-{}.txt\nd{i}\n", i + 1),
            ),
        ),
    ]
}

/// A crate `h`, its `src/lib.rs` the `body`, with `more` files beside it.
fn krate(body: String, more: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut files = vec![
        (
            "Cargo.toml".to_string(),
            "[package]\nname = \"h\"\nversion = \"0.1.0\"\n".to_string(),
        ),
        ("src/lib.rs".into(), body),
    ];
    files.extend(more);
    files
}

fn rust() -> Vec<Case> {
    let r = |name, body: String| (name, krate(body, vec![]));
    let x = |body: String| {
        format!("pub fn f<T>(a: T) -> T {{ a }}\npub fn g() {{ let x = {body}; }}\n")
    };
    let g = |body: String| format!("pub fn g() {{ {body} }}\n");
    let strukt = "pub struct S;\nimpl S { pub fn m(&self) -> S { S } }\n";
    let axum = "use axum::{Router, routing::get};\nasync fn h() {}\n";
    let (client, new) = ("reqwest::Client", "reqwest::Client::new()");
    let package = |name: &str| format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
    let manifest = |name, toml: String| {
        let files = vec![
            ("Cargo.toml".to_string(), package("h") + &toml),
            ("src/lib.rs".into(), String::new()),
        ];
        (name, files)
    };
    vec![
        r("parens", x(nest("(", "1", ")", DEEP))),
        r("arrays", x(nest("[", "1", "]", DEEP))),
        r("tuples", x(nest("(", "1,", ",)", DEEP))),
        r("calls", x(nest("f(", "1", ")", DEEP))),
        r("closures", x(nest("|| ", "1", "", DEEP))),
        r("refs", x(nest("&", "1", "", DEEP))),
        r("blocks", x(nest("{ ", "1", " }", DEEP))),
        r("ifs", x(nest("if true { ", "1", " } else { 2 }", DEEP))),
        r("matches", x(nest("match 1 { _ => ", "1", " }", DEEP))),
        r("vecs", x(nest("vec![", "1", "]", DEEP))),
        r("formats", x(nest("format!(\"{}\", ", "1", ")", DEEP))),
        r("binary", x(vec!["\"a\""; 5 * LONG].join(" + "))),
        r(
            "methods",
            format!(
                "{strukt}pub fn g() {{ let x = S{}; }}\n",
                ".m()".repeat(LONG)
            ),
        ),
        r("fields", x(format!("a{}", ".b".repeat(5 * LONG)))),
        r(
            "functions",
            format!("pub {}", nest("fn f() { ", "", " }", DEEP)),
        ),
        r(
            "modules",
            format!("pub {}", nest("mod m { ", "pub fn f() {}", " }", DEEP)),
        ),
        r(
            "impls",
            format!(
                "pub {}",
                nest(
                    "fn f() { struct S; impl S { fn m(&self) { ",
                    "",
                    " } } }",
                    DEEP
                )
            ),
        ),
        r(
            "types",
            format!("pub type T = {};\n", nest("Vec<", "u8", ">", DEEP)),
        ),
        r(
            "tuple-types",
            format!("pub type T = {};\n", nest("(", "u8", ",)", DEEP)),
        ),
        r(
            "fn-types",
            format!("pub type T = {};\n", nest("Box<dyn Fn(", "u8", ")>", DEEP)),
        ),
        r(
            "path-call",
            format!("pub fn g() {{ {}f(); }}\n", "a::".repeat(DEEP)),
        ),
        r(
            "path-use",
            format!("use {}f;\npub fn g() {{ f(); }}\n", "a::".repeat(DEEP)),
        ),
        r(
            "use-tree",
            format!("use {};\n", nest("a::{", "f", "}", DEEP)),
        ),
        r(
            "use-wide",
            format!(
                "use a::{{{}}};\n",
                (0..WIDE)
                    .map(|i| format!("f{i}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        r(
            "trait-bases",
            format!(
                "{}\npub struct S;\nimpl T0 for S {{}}\n",
                lines(DEEP, |i| format!(
                    "pub trait T{i}: T{} {{ fn m(&self) {{}} }}",
                    i + 1
                )) + &format!("\npub trait T{DEEP} {{}}")
            ),
        ),
        r(
            "impl-blocks",
            format!(
                "pub struct S;\n{}\n",
                lines(WIDE, |i| format!(
                    "impl From<[u8; {i}]> for S {{ fn from(_: [u8; {i}]) -> S {{ S }} }}"
                ))
            ),
        ),
        r(
            "receivers",
            format!(
                "{strukt}pub fn g(a: &S) {{\n{}\n}}\n",
                lines(DEEP, |i| format!(
                    "let b{i} = {}.m();",
                    if i == 0 {
                        "a".into()
                    } else {
                        format!("b{}", i - 1)
                    }
                ))
            ),
        ),
        r(
            "returns",
            format!(
                "{strukt}{}\npub fn g() {{ r0().m(); }}\n",
                lines(DEEP, |i| format!("fn r{i}() -> S {{ r{}() }}", i + 1))
                    + &format!("\nfn r{DEEP}() -> S {{ S }}")
            ),
        ),
        r(
            "attributes",
            format!("{}\npub fn f() {{}}\n", "#[a]\n".repeat(WIDE)),
        ),
        r(
            "derives",
            format!(
                "#[derive({})]\npub struct S;\n",
                (0..WIDE)
                    .map(|i| format!("D{i}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        r(
            "cfg-nesting",
            format!(
                "#[cfg({})]\npub fn f() {{}}\n",
                nest("all(", "unix", ")", DEEP)
            ),
        ),
        r(
            "cfg-features",
            format!(
                "{}\n",
                lines(WIDE, |i| format!(
                    "#[cfg(feature = \"f{i}\")]\npub fn f{i}() {{}}"
                ))
            ),
        ),
        r(
            "routes-attributes",
            format!("{}\npub fn h() {{}}\n", "#[get(\"/a\")]\n".repeat(WIDE)),
        ),
        r(
            "route-methods",
            format!(
                "#[route(\"/a\", {})]\npub fn h() {{}}\n",
                vec!["method = \"GET\""; WIDE].join(", ")
            ),
        ),
        r(
            "routes-macro",
            format!(
                "pub fn g() {{ rocket::build().mount(\"/\", routes![{}]); }}\n",
                vec!["h"; WIDE].join(", ")
            ),
        ),
        r(
            "axum-chain",
            format!(
                "{axum}pub fn g() {{ Router::new(){}; }}\n",
                ".route(\"/a\", get(h))".repeat(LONG)
            ),
        ),
        r(
            "axum-method-chain",
            format!(
                "{axum}pub fn g() {{ Router::new().route(\"/a\", get(h){}); }}\n",
                ".post(h)".repeat(LONG)
            ),
        ),
        r(
            "axum-nest",
            format!(
                "{axum}pub fn g() {{ let r = {}; }}\n",
                nest(
                    "Router::new().route(\"/a\", get(h)).nest(\"/p\", ",
                    "Router::new()",
                    ")",
                    LINKS
                )
            ),
        ),
        r(
            "axum-merge",
            format!(
                "{axum}pub fn g() {{ let r = {}; }}\n",
                nest("Router::new().merge(", "Router::new()", ")", LINKS)
            ),
        ),
        r(
            "format-holes",
            format!(
                "pub fn g() {{ let a = 1; let s = format!(\"{}\"); }}\n",
                "{a}".repeat(WIDE)
            ),
        ),
        r(
            "format-args",
            format!(
                "pub fn g() {{ let s = format!(\"{}\", {}); }}\n",
                "{}".repeat(WIDE),
                vec!["1"; WIDE].join(", ")
            ),
        ),
        r(
            "format-literal",
            format!(
                "pub fn g() {{ let s = format!(\"{}\"); }}\n",
                "x".repeat(1 << 20)
            ),
        ),
        r(
            "string-builders",
            format!(
                "pub fn g() {{ let mut s = String::new();\n{}\n}}\n",
                lines(WIDE, |_| "s.push_str(\"a\"); s += \"b\";".into())
            ),
        ),
        r(
            "string-adds",
            format!(
                "pub fn g() {{ let s = String::new() {}; }}\n",
                "+ \"a\"".repeat(WIDE)
            ),
        ),
        // Values a reqwest call reads, through what each case chains or nests.
        r(
            "doubling",
            g(format!(
                "let s0 = \"a\";\n{}\n{new}.get(s40);",
                lines(40, |i| format!(
                    "let s{} = format!(\"{{}}{{}}\", s{i}, s{i});",
                    i + 1
                ))
            )),
        ),
        r(
            "doubling-captures",
            g(format!(
                "let s0 = \"/a\";\n{}\n{new}.get(s40);",
                lines(40, |i| format!(
                    "let s{} = format!(\"{{s{i}}}{{s{i}}}\");",
                    i + 1
                ))
            )),
        ),
        r(
            "const-chain",
            g(format!(
                "let a0 = \"/a\";\n{}\n{new}.get(a{DEEP});",
                lines(DEEP, |i| format!("let a{} = a{i};", i + 1))
            )),
        ),
        r(
            "const-nesting",
            g(format!(
                "let c0 = \"/a\";\n{}\n{new}.get(c{DEEP});",
                lines(DEEP, |i| format!(
                    "let c{} = {};",
                    i + 1,
                    nest("[", &format!("c{i}"), "]", 60)
                ))
            )),
        ),
        r(
            "wrapper-chain",
            format!(
                "fn w0(u: &str) {{ {new}.get(u); }}\n{}\n{}",
                lines(DEEP, |i| format!("fn w{}(u: &str) {{ w{i}(u) }}", i + 1)),
                g(format!("w{DEEP}(\"/a\");"))
            ),
        ),
        r(
            "wrapper-lattice",
            format!(
                "fn w0(u: &str) {{ {new}.get(u); }}\n{}\n{}",
                lines(40, |i| format!(
                    "fn w{}(u: &str) {{ w{i}(u); w{i}(u) }}",
                    i + 1
                )),
                g("w40(\"/a\");".into())
            ),
        ),
        r(
            "returns-chain",
            format!(
                "fn c0() -> {client} {{ {new} }}\n{}\n{}",
                lines(DEEP, |i| format!(
                    "fn c{}() -> {client} {{ c{i}() }}",
                    i + 1
                )),
                g(format!("let c = c{DEEP}(); c.get(\"/a\");"))
            ),
        ),
        r(
            "returns-lattice",
            format!(
                "fn c0() -> {client} {{ {new} }}\n{}\n{}",
                lines(40, |i| format!(
                    "fn c{}() -> {client} {{ if true {{ return c{i}(); }} c{i}() }}",
                    i + 1
                )),
                g("let c = c40(); c.get(\"/a\");".into())
            ),
        ),
        r(
            "param-lattice",
            format!(
                "fn p0(c: &{client}) {{ c.get(\"/a\"); }}\n{}\n{}",
                lines(40, |i| format!(
                    "fn p{}(c: &{client}) {{ p{i}(c); p{i}(c) }}",
                    i + 1
                )),
                g(format!("p40(&{new});"))
            ),
        ),
        r(
            "param-chain",
            format!(
                "fn p0(c: &{client}) {{ c.get(\"/a\"); }}\n{}\n{}",
                lines(DEEP, |i| format!(
                    "fn p{}(c: &{client}) {{ p{i}(c) }}",
                    i + 1
                )),
                g(format!("p{DEEP}(&{new});"))
            ),
        ),
        r(
            "bindings",
            format!(
                "pub fn g() {{\n{}\n}}\n",
                lines(WIDE, |_| "let x = g(); x.m();".into())
            ),
        ),
        r(
            "mutables",
            format!(
                "pub fn g() {{\n{}\n}}\n",
                lines(LONG, |i| format!(
                    "let mut v{i} = Vec::new(); v{i}.push(1); h(&mut v{i});"
                ))
            ),
        ),
        r(
            "shadowing",
            format!(
                "pub fn g() {{ let x = 1;\n{}\n}}\n",
                lines(WIDE, |_| "let x = x + 1;".into())
            ),
        ),
        (
            "pub-use-chain",
            krate(
                format!(
                    "{}\npub fn g() {{ p0::x(); }}\n",
                    lines(LINKS, |i| format!("pub mod p{i};"))
                ),
                chain(
                    LINKS,
                    |i| format!("src/p{i}.rs"),
                    |i| match i + 1 == LINKS {
                        true => "pub fn x() {}\n".into(),
                        false => format!(
                            "pub use crate::p{}::x;\npub use crate::p{}::*;\n",
                            i + 1,
                            i + 1
                        ),
                    },
                ),
            ),
        ),
        (
            "alias-chain",
            krate(
                format!(
                    "pub fn x() {{}}\n{}\npub fn g() {{ a{DEEP}(); }}\n",
                    lines(DEEP, |i| format!(
                        "use {} as a{};",
                        if i == 0 {
                            "crate::x".into()
                        } else {
                            format!("crate::a{i}")
                        },
                        i + 1
                    ))
                ),
                vec![],
            ),
        ),
        (
            "mod-chain",
            krate(
                "#[path = \"m0.rs\"] mod m0;\npub fn g() { m0::x(); }\n".into(),
                chain(
                    LINKS,
                    |i| format!("src/m{i}.rs"),
                    |i| match i + 1 == LINKS {
                        true => "pub fn x() {}\n".into(),
                        false => format!(
                            "#[path = \"m{}.rs\"] pub mod next;\npub fn x() {{ next::x() }}\n",
                            i + 1
                        ),
                    },
                ),
            ),
        ),
        manifest(
            "deps",
            format!(
                "[dependencies]\n{}\n",
                lines(WIDE, |i| format!("d{i} = \"1\""))
            ),
        ),
        manifest(
            "dev-deps",
            format!(
                "[dependencies]\n{}\n[dev-dependencies]\n{}\n",
                lines(WIDE, |i| format!("d{i} = \"1\"")),
                lines(WIDE, |i| format!(
                    "d{i} = {{ version = \"1\", features = [\"a\"] }}"
                ))
            ),
        ),
        manifest(
            "targets",
            lines(WIDE, |i| {
                format!("[target.'cfg(t{i})'.dependencies]\nd = \"1\"")
            }),
        ),
        manifest(
            "features",
            format!(
                "[features]\n{}\n",
                lines(WIDE, |i| format!("f{i} = [\"f{}\"]", i + 1))
            ),
        ),
        manifest(
            "inline-tables",
            format!("[dependencies]\nd = {}\n", nest("{ d = ", "1", " }", WIDE)),
        ),
        manifest(
            "members",
            format!(
                "[workspace]\nmembers = [{}]\n",
                (0..WIDE)
                    .map(|i| format!("\"m{i}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        manifest(
            "bins",
            lines(WIDE, |i| {
                format!("[[bin]]\nname = \"b{i}\"\npath = \"src/b{i}.rs\"")
            }),
        ),
        manifest("large", format!("#{}", "x".repeat(1 << 20))),
        (
            "member-chain",
            chain(
                LINKS,
                |i| format!("m{i}/Cargo.toml"),
                |i| {
                    package(&format!("m{i}"))
                        + &format!(
                            "[dependencies]\nm{} = {{ path = \"../m{}\" }}\n",
                            i + 1,
                            i + 1
                        )
                },
            )
            .into_iter()
            .chain(chain(
                LINKS,
                |i| format!("m{i}/src/lib.rs"),
                |i| format!("pub use m{}::x;\n", i + 1),
            ))
            .collect(),
        ),
    ]
}

/// A Go module `example.com/h`, its `h.go` the `body`, with `more` files beside it.
fn module(body: String, more: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut files = vec![
        ("go.mod".to_string(), "module example.com/h\n".to_string()),
        ("h.go".into(), body),
    ];
    files.extend(more);
    files
}

fn go() -> Vec<Case> {
    let m = |name, body: String| (name, module(body, vec![]));
    let x =
        |body: String| format!("package h\n\nfunc f(a int) int {{ return a }}\n\nvar x = {body}\n");
    let g = |body: String| format!("package h\n\nfunc g() {{ {body} }}\n");
    let manifest = |name, go_mod: String| {
        let files = vec![
            ("go.mod".to_string(), go_mod),
            ("h.go".into(), "package h\n".into()),
        ];
        (name, files)
    };
    let gin = |body: String| {
        let head = "package h\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/gin-gonic/gin\"\n)\n";
        format!("{head}\nfunc f(c *gin.Context) {{}}\n\n{body}\n")
    };
    let chi = |body: String| {
        let head = "package h\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/go-chi/chi/v5\"\n)\n";
        format!("{head}\nfunc f(w http.ResponseWriter, r *http.Request) {{}}\n\n{body}\n")
    };
    let mux = |body: String| {
        let head = "package h\n\nimport (\n\t\"net/http\"\n\n\t\"github.com/gorilla/mux\"\n)\n";
        format!("{head}\nfunc f(w http.ResponseWriter, r *http.Request) {{}}\n\n{body}\n")
    };
    vec![
        // Routers: groups made on groups past `PREFIX_DEPTH`, a registration function reaching
        // the next one with a group, and a mux inside calls nested past the value depth.
        m(
            "groups",
            gin(format!(
                "func g() {{\n\tg0 := gin.Default()\n{}\n\tg{LINKS}.GET(\"/x\", f)\n}}",
                lines(LINKS, |i| format!("\tg{} := g{i}.Group(\"/a\")", i + 1))
            )),
        ),
        m(
            "registrations",
            gin(format!(
                "func g() {{ r0(gin.Default().Group(\"/a\")) }}\n{}\nfunc r{LINKS}(g *gin.RouterGroup) {{ g.GET(\"/x\", f) }}",
                lines(LINKS, |i| {
                    format!(
                        "func r{i}(g *gin.RouterGroup) {{ r{}(g.Group(\"/a\")) }}",
                        i + 1
                    )
                })
            )),
        ),
        // chi and gorilla/mux: `Route` literals nested past the definition depth, a named
        // function reaching the next with its subrouter, `Mount` and `PathPrefix` chains past
        // `PREFIX_DEPTH`, and `Methods` chained past the value depth.
        m(
            "chi route literals",
            chi(format!(
                "func g() {{\n\tr := chi.NewRouter()\n\t{}\n}}",
                nest(
                    "r.Route(\"/a\", func(r chi.Router) { ",
                    "r.Get(\"/x\", f)",
                    " })",
                    DEEP
                )
            )),
        ),
        m(
            "chi route functions",
            chi(format!(
                "func g() {{ chi.NewRouter().Route(\"/a\", c0) }}\n{}\nfunc c{LINKS}(r chi.Router) {{ r.Get(\"/x\", f) }}",
                lines(LINKS, |i| format!(
                    "func c{i}(r chi.Router) {{ r.Route(\"/a\", c{}) }}",
                    i + 1
                ))
            )),
        ),
        m(
            "chi mounts",
            chi(format!(
                "func g() {{\n\tm0 := chi.NewRouter()\n{}\n\tm{LINKS}.Get(\"/x\", f)\n}}",
                lines(LINKS, |i| format!(
                    "\tm{} := chi.NewRouter()\n\tm{i}.Mount(\"/a\", m{})",
                    i + 1,
                    i + 1
                ))
            )),
        ),
        m(
            "gorilla subrouters",
            mux(format!(
                "func g() {{\n\ts0 := mux.NewRouter()\n{}\n\ts{LINKS}.HandleFunc(\"/x\", f)\n}}",
                lines(LINKS, |i| format!(
                    "\ts{} := s{i}.PathPrefix(\"/a\").Subrouter()",
                    i + 1
                ))
            )),
        ),
        m(
            "gorilla methods",
            mux(format!(
                "func g() {{ mux.NewRouter().HandleFunc(\"/x\", f){} }}",
                ".Methods(\"GET\")".repeat(DEEP)
            )),
        ),
        m(
            "mux-chain",
            gin(format!(
                "func g() {{\n\tm0 := http.NewServeMux()\n{}\n\tm{LINKS}.HandleFunc(\"/a/x\", f)\n}}",
                lines(LINKS, |i| format!(
                    "\tm{} := http.NewServeMux()\n\tm{i}.Handle(\"/a/\", m{})",
                    i + 1,
                    i + 1
                ))
            )),
        ),
        m(
            "wrapped-muxes",
            gin(format!(
                "func g() {{\n\tmux := http.NewServeMux()\n\thttp.Handle(\"/x\", {})\n}}",
                nest("w(", "mux", ")", DEEP)
            )),
        ),
        m("parens", x(nest("(", "1", ")", DEEP))),
        m("calls", x(nest("f(", "1", ")", DEEP))),
        m("composites", x(nest("[]any{", "1", "}", DEEP))),
        m("binary", x(vec!["\"a\""; 5 * LONG].join(" + "))),
        m("literals", g(nest("func() { ", "f(1)", " }()", DEEP))),
        m("blocks", g(nest("{ ", "f(1)", " }", DEEP))),
        m("ifs", g(nest("if true { ", "f(1)", " }", DEEP))),
        m("selectors", g(format!("a{}()", ".b".repeat(DEEP)))),
        m(
            "receivers",
            format!(
                "package h\n\ntype S struct{{}}\n\nfunc (s {}S) M() {{}}\n",
                "*".repeat(DEEP)
            ),
        ),
        // Receivers: a result passed from variable to variable, a field chain, a type wrapped
        // in pointers, and one name bound over and over.
        m(
            "result chain",
            format!(
                "package h\n\ntype S struct{{}}\n\nfunc (s S) M() S {{ return s }}\n\n\
                 func g() {{\n\ta0 := S{{}}\n{}\n\ta0.M()\n}}\n",
                lines(DEEP, |i| format!("\ta{} := a{i}.M()", i + 1))
            ),
        ),
        m(
            "field chain",
            format!(
                "package h\n\ntype S struct{{ n *S }}\n\nfunc (s *S) M() {{}}\n\n\
                 func g(a *S) {{ a{}.M() }}\n",
                ".n".repeat(DEEP)
            ),
        ),
        m(
            "pointers",
            format!(
                "package h\n\ntype S struct{{}}\n\nfunc (s S) M() {{}}\n\n\
                 func g(a {}S) {{ a.M() }}\n",
                "*".repeat(DEEP)
            ),
        ),
        m(
            "bindings",
            format!(
                "package h\n\ntype S struct{{}}\n\nfunc (s S) M() {{}}\n\n{}\n",
                lines(WIDE, |i| format!("func f{i}() {{ a := S{{}}; a.M() }}"))
            ),
        ),
        m(
            "nested bindings",
            format!(
                "package h\n\ntype S struct{{}}\n\nfunc (s S) M() {{}}\n\n\
                 func g() {{ a := S{{}}; {} }}\n",
                nest("func() { a := S{}; a.M(); ", "a.M()", " }()", DEEP)
            ),
        ),
        m(
            "rebound",
            format!(
                "package h\n\ntype S struct{{}}\n\nfunc (s S) M() {{}}\n\n\
                 func g() {{\n{}\n\ta.M()\n}}\n",
                lines(WIDE, |_| "\ta := S{}".into())
            ),
        ),
        m(
            "constraint",
            format!(
                "//go:build {}\n\npackage h\n",
                nest("(", "linux", ")", DEEP)
            ),
        ),
        m(
            "negations",
            format!("//go:build {}linux\n\npackage h\n", "!".repeat(DEEP)),
        ),
        m(
            "imports",
            format!(
                "package h\n\nimport (\n{}\n)\n",
                lines(WIDE, |i| format!("\tp{i} \"example.com/h/p{i}\""))
            ),
        ),
        m(
            "names",
            format!(
                "package h\n\n{}\n",
                lines(WIDE, |i| format!("func f{i}() {{ f{}() }}", i + 1))
            ),
        ),
        (
            "package",
            module(
                "package h\n".into(),
                chain(
                    LINKS,
                    |i| format!("p{i}.go"),
                    |i| format!("package h\n\nfunc f{i}() {{ f{}() }}\n", i + 1),
                ),
            ),
        ),
        manifest(
            "large",
            format!("module example.com/h\n// {}\n", "x".repeat(1 << 20)),
        ),
        manifest(
            "replaces",
            format!(
                "module example.com/h\n\nreplace (\n{}\n)\n",
                lines(WIDE, |i| format!("\texample.com/r{i} => ./r{i}"))
            ),
        ),
        (
            "module-chain",
            chain(
                LINKS,
                |i| format!("m{i}/go.mod"),
                |i| {
                    format!(
                        "module example.com/m{i}\n\nreplace example.com/m{} => ../m{}\n",
                        i + 1,
                        i + 1
                    )
                },
            )
            .into_iter()
            .chain(chain(
                LINKS,
                |i| format!("m{i}/m.go"),
                |i| {
                    format!(
                        "package m{i}\n\nimport \"example.com/m{}\"\n\nfunc F() {{ m{}.F() }}\n",
                        i + 1,
                        i + 1
                    )
                },
            ))
            .collect(),
        ),
    ]
}

/// A test for each part of each pack: `<pack>_survives_hostile_input_<part>`.
macro_rules! parts {
    ($($test:ident = $pack:literal, $cases:ident, $part:literal;)*) => {
        $(#[test]
        fn $test() {
            run($pack, $cases(), $part);
        })*
    };
}

parts! {
    typescript_survives_hostile_input_0 = "ts", ts, 0;
    typescript_survives_hostile_input_1 = "ts", ts, 1;
    typescript_survives_hostile_input_2 = "ts", ts, 2;
    typescript_survives_hostile_input_3 = "ts", ts, 3;
    python_survives_hostile_input_0 = "py", python, 0;
    python_survives_hostile_input_1 = "py", python, 1;
    python_survives_hostile_input_2 = "py", python, 2;
    python_survives_hostile_input_3 = "py", python, 3;
    rust_survives_hostile_input_0 = "rs", rust, 0;
    rust_survives_hostile_input_1 = "rs", rust, 1;
    rust_survives_hostile_input_2 = "rs", rust, 2;
    rust_survives_hostile_input_3 = "rs", rust, 3;
    go_survives_hostile_input_0 = "go", go, 0;
    go_survives_hostile_input_1 = "go", go, 1;
    go_survives_hostile_input_2 = "go", go, 2;
    go_survives_hostile_input_3 = "go", go, 3;
}
