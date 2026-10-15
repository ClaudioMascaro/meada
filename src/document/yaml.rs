//! YAML read into the JSON value model, so the document readers see one tree whatever the
//! syntax. The first document of the stream only. A mapping key is the scalar text as written
//! (`200:` is the key `"200"`); a plain scalar is typed by the YAML 1.2 core schema, a quoted or
//! block one is a string; tags are not read. An alias repeats the node its anchor holds, and
//! every node built, aliases included, is charged to a budget proportional to the source, so a
//! small file cannot expand into a huge tree ("billion laughs"). Nothing recurses per level of
//! nesting, and a document nested deeper than `MAX_NESTING` is unreadable.

use std::collections::HashMap;

use serde_json::{Map, Number, Value};
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

/// What a node costs, besides the bytes of its text: the budget is `EXPANSION` times the
/// source length plus `SLACK`, far above what a document without aliases uses.
const EXPANSION: usize = 4;
const SLACK: usize = 1 << 16;

/// How many collections may be open inside each other, the same limit `serde_json` puts on a
/// JSON document. Deeper, the document is unreadable: the tree a reader walks, and drops,
/// recursively stays shallow.
// ponytail: real documents nest a few dozen levels; 128 matches JSON's limit.
const MAX_NESTING: usize = 128;

/// The first document of `src`, or the line and the reason it cannot be read. The parser's
/// events are taken one at a time (its own `load` recurses once per level of nesting), and the
/// collections being read are an explicit stack.
pub fn parse(src: &str) -> Result<Value, (usize, String)> {
    let mut builder = Builder {
        stack: Vec::new(),
        anchors: HashMap::new(),
        budget: src.len().saturating_mul(EXPANSION).saturating_add(SLACK),
        root: None,
        error: None,
    };
    let mut parser = Parser::new_from_str(src);
    loop {
        let (event, mark) = parser
            .next_token()
            .map_err(|e| (e.marker().line(), e.info().to_string()))?;
        // The first document only.
        let end = matches!(event, Event::DocumentEnd | Event::StreamEnd);
        builder.on_event(event, mark);
        if let Some(error) = builder.error {
            return Err(error);
        }
        if end {
            return Ok(builder.root.unwrap_or(Value::Null));
        }
    }
}

/// A collection being read, with its anchor and the cost of what it holds so far.
struct Open {
    node: Node,
    anchor: usize,
    cost: usize,
}

enum Node {
    Sequence(Vec<Value>),
    /// The entries so far, and the key waiting for its value.
    Mapping(Map<String, Value>, Option<String>),
}

struct Builder {
    stack: Vec<Open>,
    /// Each anchored node with its cost, charged again at every alias.
    anchors: HashMap<usize, (Value, usize)>,
    budget: usize,
    root: Option<Value>,
    error: Option<(usize, String)>,
}

impl Builder {
    fn charge(&mut self, cost: usize, mark: Marker) -> bool {
        match self.budget.checked_sub(cost) {
            Some(left) => {
                self.budget = left;
                true
            }
            None => {
                self.fail(mark, "aliases expand beyond the size limit");
                false
            }
        }
    }

    fn fail(&mut self, mark: Marker, reason: &str) {
        self.error.get_or_insert((mark.line(), reason.to_string()));
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

    /// Places a finished node in the collection that holds it, or as the root.
    fn place(&mut self, value: Value, cost: usize, mark: Marker) {
        let Some(open) = self.stack.last_mut() else {
            self.root = Some(value);
            return;
        };
        open.cost = open.cost.saturating_add(cost);
        match &mut open.node {
            Node::Sequence(items) => items.push(value),
            Node::Mapping(map, key @ Some(_)) => {
                map.insert(key.take().unwrap_or_default(), value);
            }
            Node::Mapping(_, key @ None) => match value {
                Value::String(text) => *key = Some(text),
                _ => self.fail(mark, "a mapping key that is not a scalar"),
            },
        }
    }

    fn remember(&mut self, anchor: usize, value: &Value, cost: usize) {
        if anchor != 0 {
            self.anchors.insert(anchor, (value.clone(), cost));
        }
    }
}

impl MarkedEventReceiver for Builder {
    fn on_event(&mut self, event: Event, mark: Marker) {
        if self.error.is_some() {
            return;
        }
        match event {
            Event::Scalar(text, style, anchor, _) => {
                let cost = 1 + text.len();
                if !self.charge(cost, mark) {
                    return;
                }
                let value = match style {
                    TScalarStyle::Plain if !self.at_key() => plain(text),
                    _ => Value::String(text),
                };
                self.remember(anchor, &value, cost);
                self.place(value, cost, mark);
            }
            Event::Alias(anchor) => {
                let Some((value, cost)) = self.anchors.get(&anchor).cloned() else {
                    return self.fail(mark, "an alias to no anchor");
                };
                if self.charge(cost, mark) {
                    self.place(value, cost, mark);
                }
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                if self.stack.len() == MAX_NESTING {
                    let reason = format!("nested deeper than {MAX_NESTING} levels");
                    return self.fail(mark, &reason);
                }
                if !self.charge(1, mark) {
                    return;
                }
                let node = match event {
                    Event::SequenceStart(..) => Node::Sequence(Vec::new()),
                    _ => Node::Mapping(Map::new(), None),
                };
                self.stack.push(Open {
                    node,
                    anchor,
                    cost: 1,
                });
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let Some(open) = self.stack.pop() else {
                    return;
                };
                let value = match open.node {
                    Node::Sequence(items) => Value::Array(items),
                    Node::Mapping(map, _) => Value::Object(map),
                };
                self.remember(open.anchor, &value, open.cost);
                self.place(value, open.cost, mark);
            }
            _ => {}
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
    use serde_json::json;

    use super::parse;

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
            assert_eq!(parse(src), Ok(expected), "{src}");
        }
    }

    #[test]
    fn unreadable_yaml_gives_its_line_and_reason() {
        let (line, _) = parse("a: 1\nb: [unclosed\n").unwrap_err();
        assert_eq!(line, 3);
        let (line, reason) = parse("? [a]\n: 1\n").unwrap_err();
        assert_eq!(
            (line, reason.as_str()),
            (1, "a mapping key that is not a scalar")
        );
    }

    #[test]
    fn nesting_past_the_limit_is_unreadable_not_a_crash() {
        // 200,000 block sequences inside each other in 400 KB: a recursive reader overflows
        // the stack long before the end.
        let src = format!("{}x\n", "- ".repeat(200_000));
        let (line, reason) = parse(&src).unwrap_err();
        assert_eq!(
            (line, reason.as_str()),
            (1, "nested deeper than 128 levels")
        );
        // A block mapping on its own lines gives the line the limit is reached on.
        let mut src = String::new();
        for depth in 0..200 {
            src += &format!("{}k:\n", " ".repeat(depth));
        }
        let (line, _) = parse(&src).unwrap_err();
        assert_eq!(line, 129);
        // At the limit it still reads.
        let src = format!("{}x\n", "- ".repeat(128));
        assert!(parse(&src).is_ok());
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
        let (_, reason) = parse(&src).unwrap_err();
        assert_eq!(reason, "aliases expand beyond the size limit");
    }
}
