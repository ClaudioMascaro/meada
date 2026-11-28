use std::collections::HashMap;

use serde_json::{Map, Number, Value};
use yaml_rust2::parser::{Event, Parser, Tag};
use yaml_rust2::scanner::TScalarStyle;

const EXPANSION: usize = 4;
const SLACK: usize = 1 << 16;

// ponytail: real documents nest a few dozen levels; 128 matches JSON's limit.
const MAX_NESTING: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    Yaml,
    Json,
}

impl Syntax {
    pub fn of(path: &str) -> Syntax {
        if path.ends_with(".json") {
            Syntax::Json
        } else {
            Syntax::Yaml
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Key(String),
    Index(usize),
}

#[derive(Debug, PartialEq)]
pub struct Scalar<'a> {
    pub document: usize,
    pub path: &'a [Step],
    pub value: &'a Value,
    pub line: usize,
}

pub type Error = (usize, String);

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

#[derive(Debug, PartialEq)]
pub struct KeyLine {
    pub section: String,
    pub key: String,
    pub line: usize,
}

pub fn read_keyed(
    syntax: Syntax,
    src: &[u8],
    tags: bool,
    mut each: impl FnMut(Scalar<'_>),
) -> Result<(Vec<Value>, Vec<KeyLine>), Error> {
    let mut reader = Reader::new(syntax, src, &mut each)?;
    reader.builder.tags = tags;
    reader.builder.keyed = true;
    let mut documents = Vec::new();
    while let Some(document) = reader.document()? {
        documents.push(document);
    }
    Ok((documents, std::mem::take(&mut reader.builder.keys)))
}

pub(crate) struct Collection<'a> {
    pub document: usize,
    pub path: &'a [Step],
    pub line: usize,
    pub end_line: usize,
}

pub(crate) fn json_spanned(
    src: &[u8],
    each: impl FnMut(Collection<'_>, &Value),
) -> Result<Value, Error> {
    Ok(read_located(Syntax::Json, src, |_| {}, each)?
        .into_iter()
        .next()
        .unwrap_or(Value::Null))
}

pub(crate) fn read_located(
    syntax: Syntax,
    src: &[u8],
    mut scalar: impl FnMut(Scalar<'_>),
    mut collection: impl FnMut(Collection<'_>, &Value),
) -> Result<Vec<Value>, Error> {
    let mut reader = Reader::new(syntax, src, &mut scalar)?;
    reader.builder.collections = Some(&mut collection);
    let mut documents = Vec::new();
    while let Some(document) = reader.document()? {
        documents.push(document);
    }
    Ok(documents)
}

pub fn first(syntax: Syntax, src: &[u8]) -> Result<Value, Error> {
    let mut ignore = |_: Scalar<'_>| {};
    let mut reader = Reader::new(syntax, src, &mut ignore)?;
    Ok(reader.document()?.unwrap_or(Value::Null))
}

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
        let src = src.strip_prefix("\u{feff}".as_bytes()).unwrap_or(src);
        let source = match syntax {
            Syntax::Yaml => Source::Yaml(Box::new(Parser::new_from_str(text(src)?))),
            Syntax::Json => {
                serde_json::from_slice::<Value>(src).map_err(|e| {
                    let error = e.to_string();
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
            tags: false,
            keyed: false,
            keys: Vec::new(),
            collections: None,
        };
        Ok(Reader { source, builder })
    }

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
            Event::Scalar(mut text, style, anchor, tag) => {
                if matches!(style, TScalarStyle::Plain) && text.starts_with('\u{feff}') {
                    text.remove(0);
                }
                let cost = 1 + text.len();
                let value = match style {
                    TScalarStyle::Plain if !b.at_key() => plain(text),
                    _ => Value::String(text),
                };
                let wrap = b.wrapper(&tag);
                b.scalar(value, cost, anchor, line, wrap);
            }
            Event::Alias(anchor) => b.alias(anchor, line),
            Event::SequenceStart(anchor, tag) => {
                let wrap = b.wrapper(&tag);
                b.open(true, anchor, line, wrap);
            }
            Event::MappingStart(anchor, tag) => {
                let wrap = b.wrapper(&tag);
                b.open(false, anchor, line, wrap);
            }
            Event::SequenceEnd | Event::MappingEnd => b.close(line),
            _ => {}
        }
        if let Some(error) = b.error.take() {
            return Err(error);
        }
    }
}

fn json(text: &str, b: &mut Builder) -> Result<bool, Error> {
    let bytes = text.as_bytes();
    let (mut at, mut line) = (0, 1);
    while let Some(&c) = bytes.get(at) {
        let start = at;
        at += 1;
        match c {
            b'\n' => line += 1,
            b' ' | b'\t' | b'\r' | b',' | b':' => {}
            b'[' | b'{' => b.open(c == b'[', 0, line, None),
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
                    Some(Ok(value)) => b.scalar(value, 1 + at - start, 0, line, None),
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

struct Open {
    node: Node,
    anchor: usize,
    cost: usize,
    depth: usize,
    in_key: bool,
    line: usize,
    key_line: Option<usize>,
    wrap: Option<String>,
}

enum Node {
    Sequence(Vec<Value>),
    Mapping(Map<String, Value>, Option<String>),
}

#[derive(Clone)]
struct Anchored {
    value: Value,
    cost: usize,
    depth: usize,
}

type CollectionCallback<'f> = &'f mut dyn FnMut(Collection<'_>, &Value);

struct Builder<'f> {
    stack: Vec<Open>,
    path: Vec<Step>,
    anchors: HashMap<usize, Anchored>,
    budget: usize,
    document: usize,
    root: Option<Value>,
    error: Option<Error>,
    each: &'f mut dyn FnMut(Scalar<'_>),
    tags: bool,
    keyed: bool,
    keys: Vec<KeyLine>,
    collections: Option<CollectionCallback<'f>>,
}

impl Builder<'_> {
    fn charge(&mut self, cost: usize, line: usize) -> bool {
        match self.budget.checked_sub(cost) {
            Some(left) => {
                self.budget = left;
                true
            }
            None => {
                self.fail(line, "anchors and aliases expand beyond the size limit");
                false
            }
        }
    }

    fn fail(&mut self, line: usize, reason: &str) {
        self.error.get_or_insert((line, reason.to_string()));
    }

    fn too_deep(&mut self, line: usize) {
        self.fail(line, &format!("nested deeper than {MAX_NESTING} levels"));
    }

    fn at_key(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(Open {
                node: Node::Mapping(_, None),
                ..
            })
        )
    }

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

    fn wrapper(&self, tag: &Option<Tag>) -> Option<String> {
        let tag = tag.as_ref().filter(|t| self.tags && t.handle == "!")?;
        if tag.suffix.is_empty() || self.at_key() || self.stack.last().is_some_and(|o| o.in_key) {
            return None;
        }
        Some(match tag.suffix.as_str() {
            "Ref" | "Condition" => tag.suffix.clone(),
            name => format!("Fn::{name}"),
        })
    }

    fn wrapped(
        &mut self,
        value: Value,
        wrap: Option<String>,
        cost: usize,
        depth: usize,
        line: usize,
    ) -> (Value, usize, usize) {
        match wrap {
            Some(key) if self.charge(1, line) => {
                let mut map = Map::new();
                map.insert(key, value);
                (Value::Object(map), cost + 1, depth + 1)
            }
            _ => (value, cost, depth),
        }
    }

    fn scalar(
        &mut self,
        value: Value,
        cost: usize,
        anchor: usize,
        line: usize,
        wrap: Option<String>,
    ) {
        if !self.charge(cost, line) {
            return;
        }
        self.hand_out(&value, line);
        let (value, cost, depth) = self.wrapped(value, wrap, cost, 0, line);
        self.remember(anchor, &value, cost, depth, line);
        self.place(value, cost, depth, line);
    }

    fn alias(&mut self, anchor: usize, line: usize) {
        let Some(&Anchored { cost, depth, .. }) = self.anchors.get(&anchor) else {
            return self.fail(line, "an alias to no anchor");
        };
        if self.stack.len() + depth > MAX_NESTING {
            return self.too_deep(line);
        }
        if !self.charge(cost, line) {
            return;
        }
        let value = self.anchors[&anchor].value.clone();
        self.hand_out(&value, line);
        self.place(value, cost, depth, line);
    }

    fn open(&mut self, sequence: bool, anchor: usize, line: usize, wrap: Option<String>) {
        if self.stack.len() == MAX_NESTING {
            return self.too_deep(line);
        }
        if !self.charge(1, line) {
            return;
        }
        self.begin();
        let start = self.stack.last().and_then(|o| o.key_line).unwrap_or(line);
        let in_key = self.at_key() || self.stack.last().is_some_and(|open| open.in_key);
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
            in_key,
            line: start,
            key_line: None,
            wrap,
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
        if !open.in_key
            && let Some(each) = &mut self.collections
        {
            each(
                Collection {
                    document: self.document,
                    path: &self.path,
                    line: open.line,
                    end_line: line,
                },
                &value,
            );
        }
        let (value, cost, depth) = self.wrapped(value, open.wrap, open.cost, open.depth, line);
        self.remember(open.anchor, &value, cost, depth, line);
        self.place(value, cost, depth, line);
    }

    fn hand_out(&mut self, value: &Value, line: usize) {
        if self.at_key() || self.stack.last().is_some_and(|open| open.in_key) {
            return;
        }
        self.begin();
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
                    open.key_line = Some(line);
                    if let Some(step) = self.path.last_mut() {
                        *step = Step::Key(text.clone());
                    }
                    *key = Some(text.clone());
                    if let (true, 0, [Step::Key(section), Step::Key(_)]) =
                        (self.keyed, self.document, &self.path[..])
                    {
                        let section = section.clone();
                        self.keys.push(KeyLine {
                            section,
                            key: text,
                            line,
                        });
                    }
                }
                _ => self.fail(line, "a mapping key that is not a scalar"),
            },
        }
    }

    fn remember(&mut self, anchor: usize, value: &Value, cost: usize, depth: usize, line: usize) {
        if anchor != 0 && self.charge(cost, line) {
            let value = value.clone();
            self.anchors.insert(anchor, Anchored { value, cost, depth });
        }
    }
}

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

    type Row = (usize, String, Value, usize);

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
            (
                "200: ok\n'n': 1\nf: 1.5\nb: true\nz: ~\ns: '3.0'\nv: 3.1.0\n",
                json!({"200": "ok", "n": 1, "f": 1.5, "b": true, "z": null, "s": "3.0", "v": "3.1.0"}),
            ),
            ("true: |\n  a\n", json!({"true": "a\n"})),
            (
                "a: &x {k: [1, 2]}\nb: *x\n",
                json!({"a": {"k": [1, 2]}, "b": {"k": [1, 2]}}),
            ),
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
                row(2, "", json!("just text"), 5),
                row(3, "", json!(null), 7),
            ]
        );
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
        let mut handed = Vec::new();
        let result = read(Syntax::Yaml, b"? [a, {b: c}]\n: 1\n", |s| {
            handed.push(s.value.clone())
        });
        assert_eq!(
            result,
            Err((1, "a mapping key that is not a scalar".into()))
        );
        assert_eq!(handed, Vec::<Value>::new());
        assert_eq!(
            first(Syntax::Yaml, b"a: 1\nb: \xff\n"),
            Err((2, "not UTF-8".into()))
        );
    }

    #[test]
    fn nesting_past_the_limit_is_unreadable_not_a_crash() {
        let src = format!("{}x\n", "- ".repeat(200_000));
        let (line, reason) = first(Syntax::Yaml, src.as_bytes()).unwrap_err();
        assert_eq!(
            (line, reason.as_str()),
            (1, "nested deeper than 128 levels")
        );
        let mut src = String::new();
        for depth in 0..200 {
            src += &format!("{}k:\n", " ".repeat(depth));
        }
        let (line, _) = first(Syntax::Yaml, src.as_bytes()).unwrap_err();
        assert_eq!(line, 129);
        let src = format!("{}x\n", "- ".repeat(128));
        let (_, scalars) = scalars(Syntax::Yaml, &src);
        assert_eq!(scalars.len(), 1);
        assert_eq!(scalars[0].1, vec!["[0]"; 128].join("."));
        let src = format!("{}1{}", "[".repeat(200), "]".repeat(200));
        let (_, reason) = first(Syntax::Json, src.as_bytes()).unwrap_err();
        assert_eq!(reason, "recursion limit exceeded");
    }

    #[test]
    fn an_alias_counts_its_nodes_depth_where_it_lands() {
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
        let mut src = String::from("l0: &l0 [x, x, x, x, x, x, x, x, x, x]\n");
        for n in 1..=9 {
            let p = n - 1;
            src += &format!(
                "l{n}: &l{n} [*l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}]\n"
            );
        }
        let mut handed = 0;
        let (_, reason) = read(Syntax::Yaml, src.as_bytes(), |_| handed += 1).unwrap_err();
        assert_eq!(reason, "anchors and aliases expand beyond the size limit");
        assert!(
            handed < src.len() * super::EXPANSION + super::SLACK,
            "{handed}"
        );
    }

    fn tagged(syntax: Syntax, src: &[u8]) -> Result<Vec<Value>, super::Error> {
        super::read_keyed(syntax, src, true, |_| {}).map(|r| r.0)
    }

    #[test]
    fn the_second_level_keys_of_the_first_document_come_with_their_lines() {
        let yaml = "a:\n  x: 1\n  y:\n    x: 2\nb:\n  - z: 1\n---\na:\n  late: 1\n";
        let (_, keys) = super::read_keyed(Syntax::Yaml, yaml.as_bytes(), false, |_| {}).unwrap();
        let shown: Vec<_> = keys
            .iter()
            .map(|k| (k.section.as_str(), k.key.as_str(), k.line))
            .collect();
        assert_eq!(shown, [("a", "x", 2), ("a", "y", 3)]);
        let json = "{\"a\": {\"k\"\n  : 1,\n \"m\": 2}}";
        let (_, keys) = super::read_keyed(Syntax::Json, json.as_bytes(), false, |_| {}).unwrap();
        let shown: Vec<_> = keys.iter().map(|k| (k.key.as_str(), k.line)).collect();
        assert_eq!(shown, [("k", 1), ("m", 3)]);
    }

    #[test]
    fn tagged_reading_wraps_a_tagged_node_in_its_long_form_and_plain_reading_does_not() {
        let src = "a: !Ref X\nb: !Sub [\"${A}\", {A: !GetAtt [R, Arn]}]\nc: !Condition Y\nd: !If [c, !Ref Z, ~]\nf: &n !Join ['', [a]]\ng: *n\n";
        let mut seen = Vec::new();
        let (documents, _) = super::read_keyed(Syntax::Yaml, src.as_bytes(), true, |s| {
            seen.push((s.path.len(), s.value.clone(), s.line));
        })
        .unwrap();
        assert_eq!(
            documents[0],
            json!({
                "a": {"Ref": "X"},
                "b": {"Fn::Sub": ["${A}", {"A": {"Fn::GetAtt": ["R", "Arn"]}}]},
                "c": {"Condition": "Y"},
                "d": {"Fn::If": ["c", {"Ref": "Z"}, null]},
                "f": {"Fn::Join": ["", ["a"]]},
                "g": {"Fn::Join": ["", ["a"]]},
            })
        );
        assert_eq!(seen[0], (1, json!("X"), 1));
        let (plain, _) = scalars(Syntax::Yaml, "a: !Ref X\nb: !Sub [x]\n");
        assert_eq!(plain[0], json!({"a": "X", "b": ["x"]}));
        let read = tagged(Syntax::Yaml, b"!Ref k: v\n").unwrap();
        assert_eq!(read[0], json!({"k": "v"}));
        let json = tagged(Syntax::Json, br#"{"Ref": "X"}"#).unwrap();
        assert_eq!(json[0], json!({"Ref": "X"}));
    }

    #[test]
    fn tagged_nodes_count_against_the_budget_and_the_nesting_like_any_other() {
        let nest = |n: usize| "!Join [".repeat(n) + "x" + &"]".repeat(n);
        assert!(tagged(Syntax::Yaml, nest(100).as_bytes()).is_ok());
        let (_, reason) = tagged(Syntax::Yaml, nest(200).as_bytes()).unwrap_err();
        assert_eq!(reason, "nested deeper than 128 levels");
        let mut src = String::from("l0: &l0 !Ref [x, x, x, x, x, x, x, x, x, x]\n");
        for n in 1..=9 {
            let p = n - 1;
            src += &format!(
                "l{n}: &l{n} !Ref [*l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}, *l{p}]\n"
            );
        }
        let (_, reason) = tagged(Syntax::Yaml, src.as_bytes()).unwrap_err();
        assert_eq!(reason, "anchors and aliases expand beyond the size limit");
    }

    #[test]
    fn anchors_nested_around_a_large_node_cannot_copy_it_past_the_budget() {
        let list = vec!["1"; 100_000].join(",");
        let nest = |n: usize| {
            let open: String = (0..n).map(|i| format!("&a{i} [")).collect();
            format!("{open}{list}{}", "]".repeat(n))
        };
        let (_, reason) = read(Syntax::Yaml, nest(100).as_bytes(), |_| {}).unwrap_err();
        assert_eq!(reason, "anchors and aliases expand beyond the size limit");
        assert!(read(Syntax::Yaml, nest(1).as_bytes(), |_| {}).is_ok());
    }
}
