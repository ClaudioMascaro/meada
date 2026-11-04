//! The event reader: YAML and JSON read one event at a time into the JSON value model, so every
//! reader sees one tree whatever the syntax, and a caller that asks is handed every scalar with
//! its 1-based line and its position in its document. `read` reads every document of a stream,
//! `first` the first one and nothing past it.
//!
//! YAML: `yaml-rust2`'s event parser. A mapping key is the scalar text as written (`200:` is the
//! key `"200"`); a plain scalar is typed by the YAML 1.2 core schema, a quoted or block one is a
//! string; tags are not read. An alias repeats the node its anchor holds in the same document,
//! its scalars handed out again at the alias's line.
//!
//! JSON: `serde_json` reads the text first, so a JSON file reads, or fails with the same line and
//! reason, as `serde_json` has it; a lexer then walks the text it accepted, each string and
//! literal decoded by `serde_json`. One document.
//!
//! Every node built, aliases included, and every document is charged to a budget proportional to
//! the source, so a small file cannot expand into a huge tree ("billion laughs"). Nothing recurses
//! per level of nesting: the collections being read are an explicit stack, and a document nested
//! deeper than `MAX_NESTING`, an alias's node counted at the depth it lands, is unreadable.

use std::collections::HashMap;

use serde_json::{Map, Number, Value};
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::TScalarStyle;

/// What a node costs, besides the bytes of its text: the budget is `EXPANSION` times the
/// source length plus `SLACK`, far above what a document without aliases uses.
const EXPANSION: usize = 4;
const SLACK: usize = 1 << 16;

/// How many collections may be open inside each other, the same limit `serde_json` puts on a
/// JSON document. Deeper, the document is unreadable: the tree a reader walks, and drops,
/// recursively stays shallow.
// ponytail: real documents nest a few dozen levels; 128 matches JSON's limit.
const MAX_NESTING: usize = 128;

/// The syntax a text is read with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    Yaml,
    Json,
}

impl Syntax {
    /// JSON by the extension of `path`, YAML otherwise.
    pub fn of(path: &str) -> Syntax {
        if path.ends_with(".json") {
            Syntax::Json
        } else {
            Syntax::Yaml
        }
    }
}

/// A step from a collection to the node it holds: a mapping's key, or a sequence's 0-based
/// index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Key(String),
    Index(usize),
}

/// A scalar as the reader meets it.
#[derive(Debug, PartialEq)]
pub struct Scalar<'a> {
    /// The 0-based index of its document in the stream.
    pub document: usize,
    /// Its position in its document, from the root; empty when the document is the scalar.
    pub path: &'a [Step],
    /// A string, a number, a boolean or null.
    pub value: &'a Value,
    /// 1-based; for a scalar an alias repeats, the alias's.
    pub line: usize,
}

/// The line and the reason a text cannot be read.
pub type Error = (usize, String);

/// Every document of `src`, handing each scalar to `each` as it is read, in the order of the
/// text (an aliased mapping's scalars in key order). A document that cannot be read fails the
/// stream.
// Read by the tests and the hostile probes until config values (#639) store what it yields.
#[cfg_attr(not(test), expect(dead_code))]
pub fn read(
    syntax: Syntax,
    src: &[u8],
    mut each: impl FnMut(Scalar<'_>),
) -> Result<Vec<Value>, Error> {
    let mut reader = Reader::new(syntax, src, &mut each)?;
    let mut documents = Vec::new();
    while let Some(document) = reader.document()? {
        documents.push(document);
    }
    Ok(documents)
}

/// The first document of `src`, null when the stream has none. Nothing past it is read, so a
/// later document that cannot be read changes nothing.
pub fn first(syntax: Syntax, src: &[u8]) -> Result<Value, Error> {
    let mut ignore = |_: Scalar<'_>| {};
    let mut reader = Reader::new(syntax, src, &mut ignore)?;
    Ok(reader.document()?.unwrap_or(Value::Null))
}

/// `src` as text, or the line of its first byte that is not UTF-8.
fn text(src: &[u8]) -> Result<&str, Error> {
    std::str::from_utf8(src).map_err(|e| {
        let line = src[..e.valid_up_to()]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        (line, "not UTF-8".to_string())
    })
}

enum Source<'s> {
    Yaml(Box<Parser<std::str::Chars<'s>>>),
    /// The text `serde_json` accepted, until its one document is read.
    Json(Option<&'s str>),
}

struct Reader<'s, 'f> {
    source: Source<'s>,
    builder: Builder<'f>,
}

impl<'s, 'f> Reader<'s, 'f> {
    fn new(
        syntax: Syntax,
        src: &'s [u8],
        each: &'f mut dyn FnMut(Scalar<'_>),
    ) -> Result<Self, Error> {
        let source = match syntax {
            Syntax::Yaml => Source::Yaml(Box::new(Parser::new_from_str(text(src)?))),
            Syntax::Json => {
                serde_json::from_slice::<Value>(src).map_err(|e| {
                    let error = e.to_string();
                    // The position is the line; the column is noise in a status line.
                    let error = error.split(" at line ").next().unwrap_or(&error);
                    (e.line(), error.to_string())
                })?;
                Source::Json(Some(text(src)?))
            }
        };
        let builder = Builder {
            stack: Vec::new(),
            path: Vec::new(),
            anchors: HashMap::new(),
            budget: src.len().saturating_mul(EXPANSION).saturating_add(SLACK),
            document: 0,
            root: None,
            error: None,
            each,
        };
        Ok(Reader { source, builder })
    }

    /// The next document, or `None` past the last.
    fn document(&mut self) -> Result<Option<Value>, Error> {
        let b = &mut self.builder;
        let done = match &mut self.source {
            Source::Yaml(parser) => yaml(parser, b)?,
            Source::Json(text) => match text.take() {
                Some(text) => json(text, b)?,
                None => return Ok(None),
            },
        };
        if done {
            return Ok(None);
        }
        b.document += 1;
        b.anchors.clear();
        Ok(Some(b.root.take().unwrap_or(Value::Null)))
    }
}

/// Reads the events of one document into `b`; true when the stream ended instead. The parser's
/// events are taken one at a time: its own `load` recurses once per level of nesting.
fn yaml(parser: &mut Parser<std::str::Chars<'_>>, b: &mut Builder) -> Result<bool, Error> {
    loop {
        let (event, mark) = parser
            .next_token()
            .map_err(|e| (e.marker().line(), e.info().to_string()))?;
        let line = mark.line();
        match event {
            Event::StreamEnd => return Ok(true),
            Event::DocumentEnd => return Ok(false),
            Event::DocumentStart => {
                b.charge(1, line);
            }
            Event::Scalar(text, style, anchor, _) => {
                let cost = 1 + text.len();
                let value = match style {
                    TScalarStyle::Plain if !b.at_key() => plain(text),
                    _ => Value::String(text),
                };
                b.scalar(value, cost, anchor, line);
            }
            Event::Alias(anchor) => b.alias(anchor, line),
            Event::SequenceStart(anchor, _) => b.open(true, anchor, line),
            Event::MappingStart(anchor, _) => b.open(false, anchor, line),
            Event::SequenceEnd | Event::MappingEnd => b.close(line),
            _ => {}
        }
        if let Some(error) = b.error.take() {
            return Err(error);
        }
    }
}

/// Reads the one document of `text`, which `serde_json` accepted, into `b`: brackets open and
/// close, each string or literal is decoded by `serde_json`, and a mapping's keys and values
/// alternate, so `,` and `:` are separators.
fn json(text: &str, b: &mut Builder) -> Result<bool, Error> {
    let bytes = text.as_bytes();
    let (mut at, mut line) = (0, 1);
    while let Some(&c) = bytes.get(at) {
        let start = at;
        at += 1;
        match c {
            b'\n' => line += 1,
            b' ' | b'\t' | b'\r' | b',' | b':' => {}
            b'[' | b'{' => b.open(c == b'[', 0, line),
            b']' | b'}' => b.close(line),
            _ => {
                if c == b'"' {
                    while let Some(&s) = bytes.get(at).filter(|&&s| s != b'"') {
                        at += if s == b'\\' { 2 } else { 1 };
                    }
                    at += 1;
                } else {
                    let delimiter = |s: &u8| b" \t\r\n,:]}".contains(s);
                    while bytes.get(at).is_some_and(|s| !delimiter(s)) {
                        at += 1;
                    }
                }
                match text.get(start..at).map(serde_json::from_str::<Value>) {
                    Some(Ok(value)) => b.scalar(value, 1 + at - start, 0, line),
                    _ => b.fail(
                        line,
                        "a token serde_json accepted and the lexer cannot read",
                    ),
                }
            }
        }
        if let Some(error) = b.error.take() {
            return Err(error);
        }
    }
    Ok(false)
}

/// A collection being read, with its anchor, the cost of what it holds so far and how many
/// collections deep it is, itself included.
struct Open {
    node: Node,
    anchor: usize,
    cost: usize,
    depth: usize,
}

enum Node {
    Sequence(Vec<Value>),
    /// The entries so far, and the key waiting for its value.
    Mapping(Map<String, Value>, Option<String>),
}

/// An anchored node, with its cost and depth, charged again at every alias.
#[derive(Clone)]
struct Anchored {
    value: Value,
    cost: usize,
    depth: usize,
}

struct Builder<'f> {
    stack: Vec<Open>,
    /// The position of the node being read: for each open collection, the step to the node
    /// being read in it.
    path: Vec<Step>,
    anchors: HashMap<usize, Anchored>,
    budget: usize,
    /// The index of the document being read.
    document: usize,
    root: Option<Value>,
    error: Option<Error>,
    each: &'f mut dyn FnMut(Scalar<'_>),
}

impl Builder<'_> {
    fn charge(&mut self, cost: usize, line: usize) -> bool {
        match self.budget.checked_sub(cost) {
            Some(left) => {
                self.budget = left;
                true
            }
            None => {
                self.fail(line, "aliases expand beyond the size limit");
                false
            }
        }
    }

    fn fail(&mut self, line: usize, reason: &str) {
        self.error.get_or_insert((line, reason.to_string()));
    }

    /// Whether the next node is a mapping key.
    fn at_key(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(Open {
                node: Node::Mapping(_, None),
                ..
            })
        )
    }

    /// Points the path at the node about to be read: in a sequence, the next index. In a
    /// mapping the key did, when it was placed.
    fn begin(&mut self) {
        if let (
            Some(Open {
                node: Node::Sequence(items),
                ..
            }),
            Some(step),
        ) = (self.stack.last(), self.path.last_mut())
        {
            *step = Step::Index(items.len());
        }
    }

    fn scalar(&mut self, value: Value, cost: usize, anchor: usize, line: usize) {
        if !self.charge(cost, line) {
            return;
        }
        if !self.at_key() {
            self.begin();
            self.hand_out(&value, line);
        }
        self.remember(anchor, &value, cost, 0);
        self.place(value, cost, 0, line);
    }

    fn alias(&mut self, anchor: usize, line: usize) {
        let Some(anchored) = self.anchors.get(&anchor).cloned() else {
            return self.fail(line, "an alias to no anchor");
        };
        if self.stack.len() + anchored.depth > MAX_NESTING {
            let reason = format!("nested deeper than {MAX_NESTING} levels");
            return self.fail(line, &reason);
        }
        if !self.charge(anchored.cost, line) {
            return;
        }
        if !self.at_key() {
            self.begin();
            self.hand_out(&anchored.value, line);
        }
        self.place(anchored.value, anchored.cost, anchored.depth, line);
    }

    fn open(&mut self, sequence: bool, anchor: usize, line: usize) {
        if self.stack.len() == MAX_NESTING {
            let reason = format!("nested deeper than {MAX_NESTING} levels");
            return self.fail(line, &reason);
        }
        if !self.charge(1, line) {
            return;
        }
        self.begin();
        let node = if sequence {
            Node::Sequence(Vec::new())
        } else {
            Node::Mapping(Map::new(), None)
        };
        self.stack.push(Open {
            node,
            anchor,
            cost: 1,
            depth: 1,
        });
        self.path.push(Step::Index(0));
    }

    fn close(&mut self, line: usize) {
        let Some(open) = self.stack.pop() else {
            return;
        };
        self.path.pop();
        let value = match open.node {
            Node::Sequence(items) => Value::Array(items),
            Node::Mapping(map, _) => Value::Object(map),
        };
        self.remember(open.anchor, &value, open.cost, open.depth);
        self.place(value, open.cost, open.depth, line);
    }

    /// Hands every scalar of `value`, read at the path, to `each`. A walk over an explicit
    /// stack: an aliased node is as deep as `MAX_NESTING` allows.
    fn hand_out(&mut self, value: &Value, line: usize) {
        let base = self.path.len();
        let mut pending = vec![(0, None, value)];
        while let Some((below, step, value)) = pending.pop() {
            self.path.truncate(base + below);
            self.path.extend(step);
            let below = self.path.len() - base;
            match value {
                Value::Array(items) => {
                    let steps = items.iter().enumerate().rev();
                    pending.extend(steps.map(|(i, v)| (below, Some(Step::Index(i)), v)));
                }
                Value::Object(map) => {
                    let steps = map.iter().rev();
                    pending.extend(steps.map(|(k, v)| (below, Some(Step::Key(k.clone())), v)));
                }
                scalar => (self.each)(Scalar {
                    document: self.document,
                    path: &self.path,
                    value: scalar,
                    line,
                }),
            }
        }
        self.path.truncate(base);
    }

    /// Places a finished node in the collection that holds it, or as the root.
    fn place(&mut self, value: Value, cost: usize, depth: usize, line: usize) {
        let Some(open) = self.stack.last_mut() else {
            self.root = Some(value);
            return;
        };
        open.cost = open.cost.saturating_add(cost);
        open.depth = open.depth.max(depth + 1);
        match &mut open.node {
            Node::Sequence(items) => items.push(value),
            Node::Mapping(map, key @ Some(_)) => {
                map.insert(key.take().unwrap_or_default(), value);
            }
            Node::Mapping(_, key @ None) => match value {
                Value::String(text) => {
                    if let Some(step) = self.path.last_mut() {
                        *step = Step::Key(text.clone());
                    }
                    *key = Some(text);
                }
                _ => self.fail(line, "a mapping key that is not a scalar"),
            },
        }
    }

    fn remember(&mut self, anchor: usize, value: &Value, cost: usize, depth: usize) {
        if anchor != 0 {
            let value = value.clone();
            self.anchors.insert(anchor, Anchored { value, cost, depth });
        }
    }
}

/// A plain scalar by the YAML 1.2 core schema: null, a boolean, a number JSON can hold, or
/// else the text.
fn plain(text: String) -> Value {
    match text.as_str() {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        _ => {}
    }
    let numeric = text.bytes().any(|b| b.is_ascii_digit())
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E'));
    if numeric {
        if let Ok(n) = text.parse::<i64>() {
            return Value::Number(n.into());
        }
        if let Some(n) = text.parse::<f64>().ok().and_then(Number::from_f64) {
            return Value::Number(n);
        }
    }
    Value::String(text)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{Step, Syntax, first, read};

    /// A scalar handed out, as `(document, path, value, line)` with the path written `a.[0].b`.
    type Row = (usize, String, Value, usize);

    /// The documents of `src` and every scalar handed out.
    fn scalars(syntax: Syntax, src: &str) -> (Vec<Value>, Vec<Row>) {
        let mut out = Vec::new();
        let documents = read(syntax, src.as_bytes(), |s| {
            let path = s.path.iter().map(|step| match step {
                Step::Key(k) => k.clone(),
                Step::Index(i) => format!("[{i}]"),
            });
            out.push((
                s.document,
                path.collect::<Vec<_>>().join("."),
                s.value.clone(),
                s.line,
            ));
        })
        .unwrap();
        (documents, out)
    }

    fn row(
        document: usize,
        path: &str,
        value: Value,
        line: usize,
    ) -> (usize, String, Value, usize) {
        (document, path.to_string(), value, line)
    }

    #[test]
    fn yaml_reads_as_the_json_value_model() {
        for (src, expected) in [
            // Keys as written, plain scalars typed, quoted ones strings.
            (
                "200: ok\n'n': 1\nf: 1.5\nb: true\nz: ~\ns: '3.0'\nv: 3.1.0\n",
                json!({"200": "ok", "n": 1, "f": 1.5, "b": true, "z": null, "s": "3.0", "v": "3.1.0"}),
            ),
            // `true:` is the key "true"; a block scalar is text.
            ("true: |\n  a\n", json!({"true": "a\n"})),
            // An alias repeats its anchor's node.
            (
                "a: &x {k: [1, 2]}\nb: *x\n",
                json!({"a": {"k": [1, 2]}, "b": {"k": [1, 2]}}),
            ),
            // The first document only.
            ("a: 1\n---\nb: 2\n", json!({"a": 1})),
            ("", json!(null)),
        ] {
            assert_eq!(first(Syntax::Yaml, src.as_bytes()), Ok(expected), "{src}");
        }
    }

    #[test]
    fn block_and_flow_styles_give_every_scalar_with_its_line_and_position() {
        let src = "name: api\nports:\n  - 80\n  - {host: 8080, tls: false}\nnote: |\n  two\n  lines\nempty: ~\ntags: [a, 'b']\n";
        let (documents, scalars) = scalars(Syntax::Yaml, src);
        assert_eq!(
            documents,
            [json!({
                "name": "api",
                "ports": [80, {"host": 8080, "tls": false}],
                "note": "two\nlines\n",
                "empty": null,
                "tags": ["a", "b"],
            })]
        );
        assert_eq!(
            scalars,
            [
                row(0, "name", json!("api"), 1),
                row(0, "ports.[0]", json!(80), 3),
                row(0, "ports.[1].host", json!(8080), 4),
                row(0, "ports.[1].tls", json!(false), 4),
                // A block scalar's line is its text's first.
                row(0, "note", json!("two\nlines\n"), 6),
                row(0, "empty", json!(null), 8),
                row(0, "tags.[0]", json!("a"), 9),
                row(0, "tags.[1]", json!("b"), 9),
            ]
        );
    }

    #[test]
    fn every_document_of_a_stream_is_read() {
        let src = "a: 1\n---\n- x\n- y\n--- just text\n---\n...\n";
        let (documents, scalars) = scalars(Syntax::Yaml, src);
        assert_eq!(
            documents,
            [
                json!({"a": 1}),
                json!(["x", "y"]),
                json!("just text"),
                json!(null)
            ]
        );
        assert_eq!(
            scalars,
            [
                row(0, "a", json!(1), 1),
                row(1, "[0]", json!("x"), 3),
                row(1, "[1]", json!("y"), 4),
                // A document that is one scalar has no path.
                row(2, "", json!("just text"), 5),
                row(3, "", json!(null), 7),
            ]
        );
        // `first` stops at the first document: a broken second one is never read.
        assert_eq!(
            first(Syntax::Yaml, b"a: 1\n---\nb: [unclosed\n"),
            Ok(json!({"a": 1}))
        );
        let broken = read(Syntax::Yaml, b"a: 1\n---\nb: [unclosed\n", |_| {});
        assert_eq!(broken.map_err(|(line, _)| line), Err(4));
    }

    #[test]
    fn an_alias_hands_out_its_anchors_scalars_again_at_its_own_line() {
        let src = "base: &b\n  image: app\n  ports: [80]\nweb: *b\nname: &n api\nalso: *n\n";
        let (_, scalars) = scalars(Syntax::Yaml, src);
        assert_eq!(
            scalars,
            [
                row(0, "base.image", json!("app"), 2),
                row(0, "base.ports.[0]", json!(80), 3),
                row(0, "web.image", json!("app"), 4),
                row(0, "web.ports.[0]", json!(80), 4),
                row(0, "name", json!("api"), 5),
                row(0, "also", json!("api"), 6),
            ]
        );
        // Anchors belong to their document.
        let other = read(Syntax::Yaml, b"a: &x 1\n---\nb: *x\n", |_| {});
        assert_eq!(other, Err((3, "an alias to no anchor".into())));
    }

    #[test]
    fn json_reads_nested_arrays_with_lines_as_serde_json_does() {
        let src = "{\n\t\"a\": [[1, [2.5]], []],\n  \"b\\u00e9\": {\"c\": \"\\ud83d\\ude00\", \"n\": null},\n  \"big\": 18446744073709551615, \"t\": true\n}\n";
        let (documents, scalars) = scalars(Syntax::Json, src);
        let tree: Value = serde_json::from_str(src).unwrap();
        assert_eq!(documents, [tree]);
        assert_eq!(
            scalars,
            [
                row(0, "a.[0].[0]", json!(1), 2),
                row(0, "a.[0].[1].[0]", json!(2.5), 2),
                row(0, "bé.c", json!("😀"), 3),
                row(0, "bé.n", json!(null), 3),
                row(0, "big", json!(18_446_744_073_709_551_615_u64), 4),
                row(0, "t", json!(true), 4),
            ]
        );
        // What `serde_json` refuses fails with its line and reason, YAML-only syntax included.
        for (src, line, reason) in [
            ("{\"a\": 1,}\n", 1, "trailing comma"),
            ("{a: 1}", 1, "key must be a string"),
            ("{\"a\":\n [1e400]}", 2, "number out of range"),
        ] {
            assert_eq!(
                first(Syntax::Json, src.as_bytes()),
                Err((line, reason.to_string())),
                "{src}"
            );
        }
    }

    #[test]
    fn unreadable_yaml_gives_its_line_and_reason() {
        let (line, _) = first(Syntax::Yaml, b"a: 1\nb: [unclosed\n").unwrap_err();
        assert_eq!(line, 3);
        let (line, reason) = first(Syntax::Yaml, b"? [a]\n: 1\n").unwrap_err();
        assert_eq!(
            (line, reason.as_str()),
            (1, "a mapping key that is not a scalar")
        );
        assert_eq!(
            first(Syntax::Yaml, b"a: 1\nb: \xff\n"),
            Err((2, "not UTF-8".into()))
        );
    }

    #[test]
    fn nesting_past_the_limit_is_unreadable_not_a_crash() {
        // 200,000 block sequences inside each other in 400 KB: a recursive reader overflows
        // the stack long before the end.
        let src = format!("{}x\n", "- ".repeat(200_000));
        let (line, reason) = first(Syntax::Yaml, src.as_bytes()).unwrap_err();
        assert_eq!(
            (line, reason.as_str()),
            (1, "nested deeper than 128 levels")
        );
        // A block mapping on its own lines gives the line the limit is reached on.
        let mut src = String::new();
        for depth in 0..200 {
            src += &format!("{}k:\n", " ".repeat(depth));
        }
        let (line, _) = first(Syntax::Yaml, src.as_bytes()).unwrap_err();
        assert_eq!(line, 129);
        // At the limit it still reads, and hands out its scalar at the full depth.
        let src = format!("{}x\n", "- ".repeat(128));
        let (_, scalars) = scalars(Syntax::Yaml, &src);
        assert_eq!(scalars.len(), 1);
        assert_eq!(scalars[0].1, vec!["[0]"; 128].join("."));
        // JSON stops at `serde_json`'s own limit.
        let src = format!("{}1{}", "[".repeat(200), "]".repeat(200));
        let (_, reason) = first(Syntax::Json, src.as_bytes()).unwrap_err();
        assert_eq!(reason, "recursion limit exceeded");
    }

    #[test]
    fn an_alias_counts_its_nodes_depth_where_it_lands() {
        // Each anchor is 100 deep and holds the one before: 100 more levels at every alias.
        let deep = |n: usize| {
            let mut src = String::new();
            for i in 0..n {
                let inner = if i == 0 {
                    "x".to_string()
                } else {
                    format!("*a{}", i - 1)
                };
                src += &format!(
                    "a{i}: &a{i} {}{inner}{}\n",
                    "[".repeat(100),
                    "]".repeat(100)
                );
            }
            src
        };
        assert!(first(Syntax::Yaml, deep(1).as_bytes()).is_ok());
        let (line, reason) = first(Syntax::Yaml, deep(2).as_bytes()).unwrap_err();
        assert_eq!(
            (line, reason.as_str()),
            (2, "nested deeper than 128 levels")
        );
    }

    #[test]
    fn aliases_cannot_expand_past_the_budget() {
        // Nine levels of ten aliases each: 10^9 nodes from a few hundred bytes.
        let mut src = String::from("l0: &l0 [x, x, x, x, x, x, x, x, x, x]\n");
        for n in 1..=9 {
            let p = n - 1;
            src += &format!(
                "l{n}: &l{n} [*l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}]\n"
            );
        }
        let mut handed = 0;
        let (_, reason) = read(Syntax::Yaml, src.as_bytes(), |_| handed += 1).unwrap_err();
        assert_eq!(reason, "aliases expand beyond the size limit");
        // What was handed out before the budget ran out stays within it.
        assert!(
            handed < src.len() * super::EXPANSION + super::SLACK,
            "{handed}"
        );
    }
}
