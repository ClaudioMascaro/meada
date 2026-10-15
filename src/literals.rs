//! The shape of a route read from the object literals its handler returns (spec #23, ADR 0009):
//! the last of a provider's sources, after a document. Properties give fields, array literals
//! give `[]` elements, literal strings, numbers and booleans are scalar leaves, and anything
//! Meada cannot read (a call, a parameter, a spread of an unknown value) is present and open.

use std::collections::HashMap;

use crate::lang::{Call, Part, Value, property};
use crate::resolve::{Code, Target};
use crate::shape::{Shape, Source};

/// What reading the literals of one file's handlers may cost: every call examined and every
/// value read or constant followed, over all the handlers of the file. Past it, the file's
/// remaining handlers give no shape, so their routes get no drift.
// ponytail: one budget per handler file; real handlers spend a few hundred, a crafted chain of
// constants or a huge literal would otherwise cost the whole file's time.
const LITERALS_BUDGET: usize = 1 << 16;

/// How deep reading follows nested literals and constants: the nesting of objects and arrays
/// plus the constants followed on the way. A part deeper than that is present and open.
// ponytail: real responses nest a handful of levels; the limit keeps a crafted chain of
// constants, or a literal nested thousands of levels, from costing the stack.
const LITERALS_DEPTH: usize = 32;

/// Whose responses a handler sends, and so which calls send them.
#[derive(Clone, Copy)]
pub enum Handler {
    /// An Express handler: `res.json(body)` and `res.send(body)`, `res` being its second
    /// parameter, also after `res.status(code)`.
    Express,
    /// A Next.js route handler: `NextResponse.json(body, init)` and `Response.json(body, init)`.
    Next,
}

/// Reads handlers, keeping the budget of each file.
#[derive(Default)]
pub struct Literals {
    spent: HashMap<String, usize>,
}

impl Literals {
    /// The shape the 2xx responses of the handler `name` of `path` give, as the union of their
    /// body literals; `None` when it sends none Meada reads, or past the budget. A response
    /// with a literal status outside 2xx is left out; one whose status cannot be read counts.
    pub fn returned<'a>(
        &mut self,
        code: &Code<'a>,
        path: &str,
        name: &str,
        kind: Handler,
    ) -> Option<Shape> {
        let (&path, &facts) = code.files().get_key_value(path)?;
        let def = facts.defs.iter().find(|d| d.name == name)?;
        let handler = Target {
            path,
            name: &def.name,
        };
        let nested = format!("{name}.@");
        // The calls in the handler's byte range, which source order keeps contiguous.
        let from = facts
            .calls
            .partition_point(|c| c.range.start < def.range.start);
        let mut reader = Reader {
            code,
            spent: self.spent.entry(path.to_string()).or_default(),
            shape: Shape::new(Source::Literals(path.to_string())),
        };
        let mut body = Vec::new();
        for (i, call) in facts.calls.iter().enumerate().skip(from) {
            if call.range.start >= def.range.end {
                break;
            }
            reader.charge()?;
            if call.scope == name || call.scope.starts_with(&nested) {
                body.push(i);
            }
        }
        let mut by_start: HashMap<usize, Vec<usize>> = HashMap::new();
        for &i in &body {
            by_start
                .entry(facts.calls[i].range.start)
                .or_default()
                .push(i);
        }
        let mut sent = false;
        for &i in &body {
            let call = &facts.calls[i];
            let status = match kind {
                Handler::Express => express(&mut reader, handler, &facts.calls, &by_start, i),
                Handler::Next => next(&mut reader, path, call, i),
            };
            let Some(status) = status? else { continue };
            if status.is_some_and(|s| !(200..300).contains(&s)) {
                continue;
            }
            let Some(arg) = code.args(path, i)?.first() else {
                continue;
            };
            reader.read(path, &call.scope, arg, "", 0)?;
            sent = true;
        }
        sent.then_some(reader.shape)
    }
}

/// Reads one handler's values into a shape.
struct Reader<'c, 'a> {
    code: &'c Code<'a>,
    spent: &'c mut usize,
    shape: Shape,
}

impl<'a> Reader<'_, 'a> {
    /// Charges one unit of the file's budget; `None` once it is spent.
    fn charge(&mut self) -> Option<()> {
        *self.spent += 1;
        (*self.spent <= LITERALS_BUDGET).then_some(())
    }

    /// `value`, written in `path` within `scope`, with the constants it names followed: the
    /// value that stands for it, where that is written, and the depth reached. A name that is
    /// no constant (a parameter, a definition) or past the depth limit is `Unknown`.
    fn resolve(
        &mut self,
        mut path: &'a str,
        mut scope: &'a str,
        mut value: &'a Value,
        mut depth: usize,
    ) -> Option<(&'a str, &'a str, &'a Value, usize)> {
        static UNKNOWN: Value = Value::Unknown;
        while let Value::Ref(name) = value {
            self.charge()?;
            depth += 1;
            let found = match depth <= LITERALS_DEPTH {
                true => self.code.constant(path, scope, name),
                false => None,
            };
            let Some(((at, constant), held)) = found else {
                return Some((path, scope, &UNKNOWN, depth));
            };
            path = at;
            scope = constant.rsplit_once('.').map_or("", |(s, _)| s);
            value = held;
        }
        Some((path, scope, value, depth))
    }

    fn field(&mut self, at: &str) {
        if !at.is_empty() {
            self.shape.fields.insert(at.to_string());
        }
    }

    fn open(&mut self, at: &str) {
        self.field(at);
        self.shape.open.insert(at.to_string());
    }

    fn scalar(&mut self, at: &str) {
        self.field(at);
        self.shape.scalars.insert(at.to_string());
    }

    /// Adds the paths `value` gives below `at` to the shape; `None` past the budget.
    fn read(
        &mut self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        at: &str,
        depth: usize,
    ) -> Option<()> {
        self.charge()?;
        let (path, scope, value, depth) = self.resolve(path, scope, value, depth)?;
        match value {
            Value::Object(properties) => self.object(path, scope, properties, at, depth)?,
            Value::List(items) if depth < LITERALS_DEPTH => {
                self.field(at);
                let element = format!("{at}[]");
                // An empty array, or one with a spread, has no element Meada can read.
                if items.is_empty() {
                    self.open(&element);
                }
                for item in items {
                    self.read(path, scope, item, &element, depth + 1)?;
                }
            }
            Value::Scalar(_) => self.scalar(at),
            // A single part may be a name or a hole: anything, not a string.
            Value::Str(parts) if parts.len() != 1 || matches!(parts[0], Part::Text(_)) => {
                self.scalar(at)
            }
            _ => self.open(at),
        }
        Some(())
    }

    /// The properties of an object literal as fields below `at`; a spread of a known object
    /// merges its fields, one of anything else makes the object open.
    fn object(
        &mut self,
        path: &'a str,
        scope: &'a str,
        properties: &'a [(String, Value)],
        at: &str,
        depth: usize,
    ) -> Option<()> {
        if depth >= LITERALS_DEPTH {
            self.open(at);
            return Some(());
        }
        self.field(at);
        for (key, value) in properties {
            if key == "..." {
                match self.resolve(path, scope, value, depth + 1)? {
                    (path, scope, Value::Object(inner), depth) => {
                        self.object(path, scope, inner, at, depth)?
                    }
                    (_, _, Value::Undefined, _) => {}
                    _ => self.open(at),
                }
            } else if key.is_empty() || key.contains(['.', '[']) {
                // A key with no spelling in a field path may be any field.
                self.open(at);
            } else {
                let child = match at.is_empty() {
                    true => key.clone(),
                    false => format!("{at}.{key}"),
                };
                self.read(path, scope, value, &child, depth + 1)?;
            }
        }
        Some(())
    }

    /// The literal status `init`, the options of a `NextResponse.json` or `Response.json`, sets.
    fn status(&mut self, path: &'a str, scope: &'a str, init: &'a Value) -> Option<Option<u32>> {
        let (path, scope, init, depth) = self.resolve(path, scope, init, 0)?;
        let Value::Object(properties) = init else {
            return Some(None);
        };
        let Ok(Some(status)) = property(properties, "status") else {
            return Some(None);
        };
        Some(match self.resolve(path, scope, status, depth)? {
            (_, _, Value::Scalar(status), _) => *status,
            _ => None,
        })
    }
}

/// Whether the call `i` of an Express handler sends a response, and its literal status: the
/// outer `None` is a call that sends none, the inner one a status that cannot be read.
/// `res.json(body)` has none; `res.status(code).json(body)` has the `code` written in its chain.
/// `None` overall is a spent budget.
fn express<'a>(
    reader: &mut Reader<'_, 'a>,
    handler: Target<'a>,
    calls: &'a [Call],
    by_start: &HashMap<usize, Vec<usize>>,
    i: usize,
) -> Option<Option<Option<u32>>> {
    let (code, path) = (reader.code, handler.path);
    let call = &calls[i];
    let Some((receiver, method)) = call.callee.rsplit_once('.') else {
        return Some(None);
    };
    if !matches!(method, "json" | "send") {
        return Some(None);
    }
    let is_res = |c: &'a Call, name: &'a str| code.is_param(path, &c.scope, name, handler, 1);
    if receiver != "(call_expression)" {
        return Some(is_res(call, receiver).then_some(None));
    }
    // `res.status(code).json(body)`: the calls of the chain start where it does and end before
    // this one; the first is on `res`, the last `status` call sets the status.
    let mut chain = Vec::new();
    for &j in &by_start[&call.range.start] {
        reader.charge()?;
        if calls[j].range.end < call.range.end {
            chain.push(j);
        }
    }
    chain.sort_by_key(|&j| calls[j].range.end);
    let on_res = chain.first().is_some_and(|&j| {
        let base = &calls[j];
        (base.callee.split_once('.')).is_some_and(|(name, _)| is_res(base, name))
    });
    if !on_res {
        return Some(None);
    }
    let set = (chain.iter().rev()).find(|&&j| calls[j].callee.rsplit('.').next() == Some("status"));
    let Some(&set) = set else {
        return Some(Some(None));
    };
    let Some(arg) = code.args(path, set).and_then(|a| a.first()) else {
        return Some(Some(None));
    };
    let status = match reader.resolve(path, &calls[set].scope, arg, 0)? {
        (_, _, Value::Scalar(status), _) => *status,
        _ => None,
    };
    Some(Some(status))
}

/// The same for the call `i` of a Next.js handler: `NextResponse.json` (imported from
/// `next/server`) or the global `Response.json`, with the status of its options.
fn next<'a>(
    reader: &mut Reader<'_, 'a>,
    path: &'a str,
    call: &'a Call,
    i: usize,
) -> Option<Option<Option<u32>>> {
    let code = reader.code;
    let sends = match call.callee.as_str() {
        "NextResponse.json" => matches!(
            code.imported(path, &call.scope, &call.callee),
            Some(("next/server", name)) if name == "NextResponse.json"
        ),
        "Response.json" => code.is_global(path, &call.scope, "Response"),
        _ => false,
    };
    if !sends {
        return Some(None);
    }
    let Some(init) = code.args(path, i).and_then(|a| a.get(1)) else {
        return Some(Some(None));
    };
    Some(Some(reader.status(path, &call.scope, init)?))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{Handler, LITERALS_BUDGET, LITERALS_DEPTH, Literals};
    use crate::lang::{self, LanguagePack, ts::TsJs};
    use crate::resolve::{Code, Resolver};
    use crate::shape::Shape;

    /// The shape of the handler `name` of one file, read with `reader` (to share a budget).
    fn read_with(reader: &mut Literals, src: &str, name: &str, kind: Handler) -> Option<Shape> {
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let resolver = Resolver::new(&files, TsJs.modules(&[]).unwrap());
        let code = Code::new(&resolver, &files, lang::pack(0));
        reader.returned(&code, "a.ts", name, kind)
    }

    fn express(src: &str) -> Option<Shape> {
        read_with(&mut Literals::default(), src, "h", Handler::Express)
    }

    fn next(src: &str) -> Option<Shape> {
        read_with(&mut Literals::default(), src, "h", Handler::Next)
    }

    fn paths(set: &BTreeSet<String>) -> Vec<&str> {
        set.iter().map(String::as_str).collect()
    }

    #[test]
    fn literals_give_fields_elements_scalars_and_open_parts() {
        let shape = express(
            "const BASE = { kind: 'a', n: 1 };\n\
             export function h(req, res) {\n\
               res.json({ ...BASE, ok: true, rows: [{ id: 1 }], name: `x${req.q}`, got: req.q,\n\
                          mapped: ROWS.map((r) => ({ id: r })), none: [], ...req.body });\n\
             }\n",
        )
        .unwrap();
        let fields = [
            "got",
            "kind",
            "mapped",
            "n",
            "name",
            "none",
            "none[]",
            "ok",
            "rows",
            "rows[]",
            "rows[].id",
        ];
        assert_eq!(paths(&shape.fields), fields);
        // The spread of the request body opens the whole object.
        assert_eq!(paths(&shape.open), ["", "got", "mapped", "none[]"]);
        let scalars = ["kind", "n", "name", "ok", "rows[].id"];
        assert_eq!(paths(&shape.scalars), scalars);
    }

    #[test]
    fn a_literal_status_outside_2xx_leaves_the_response_out() {
        let express = |send: &str| {
            let src = format!(
                "const GONE = 410;\nexport function h(req, res) {{\n  res.json({{ ok: 1 }});\n  \
                 {send}\n}}\n"
            );
            express(&src).map(|s| paths(&s.fields).join(","))
        };
        for (send, fields) in [
            ("res.status(404).json({ a: 1 });", "ok"),
            ("res.status(GONE).send({ a: 1 });", "ok"),
            ("res.set('x', 'y').status(500).json({ a: 1 });", "ok"),
            ("res.status(201).json({ a: 1 });", "a,ok"),
            ("res.status(code).json({ a: 1 });", "a,ok"),
            ("res.json({ a: 1 });", "a,ok"),
            // Not a response of `res`.
            ("other.status(404).json({ a: 1 });", "ok"),
            ("other.json({ a: 1 });", "ok"),
        ] {
            assert_eq!(express(send).as_deref(), Some(fields), "{send}");
        }
        let next = |init: &str| {
            let src = format!(
                "import {{ NextResponse }} from 'next/server';\n\
                 export function h() {{\n  NextResponse.json({{ ok: 1 }});\n  \
                 Response.json({{ a: 1 }}{init});\n}}\n"
            );
            next(&src).map(|s| paths(&s.fields).join(","))
        };
        assert_eq!(next(", { status: 404 }").as_deref(), Some("ok"));
        assert_eq!(next(", { status: 200 }").as_deref(), Some("a,ok"));
        assert_eq!(next(", init").as_deref(), Some("a,ok"));
        assert_eq!(next("").as_deref(), Some("a,ok"));
    }

    #[test]
    fn a_rebound_name_does_not_send() {
        // `res` is the handler's second parameter only until a binding hides it.
        let hidden =
            "export function h(req, res) {\n  { const res = other(); res.json({ a: 1 }); }\n}\n";
        assert!(express(hidden).is_none());
        assert!(express("export function h(res) {\n  res.json({ a: 1 });\n}\n").is_none());
        // `Response` is the global one only while nothing binds the name.
        let bound =
            "export function h() {\n  const Response = other;\n  Response.json({ a: 1 });\n}\n";
        assert!(next(bound).is_none());
        // `NextResponse` must come from `next/server`.
        let other = "import { NextResponse } from 'elsewhere';\n\
                     export function h() {\n  NextResponse.json({ a: 1 });\n}\n";
        assert!(next(other).is_none());
    }

    #[test]
    fn nesting_past_the_depth_limit_is_open() {
        let deep = |levels: usize| {
            let literal = "{ a: ".repeat(levels) + "1" + &" }".repeat(levels);
            let src = format!("export function h(req, res) {{ res.json({literal}); }}\n");
            express(&src).unwrap()
        };
        let path = |n: usize| vec!["a"; n].join(".");
        let shape = deep(LITERALS_DEPTH + 8);
        // The object at depth `LITERALS_DEPTH` is present and open, with nothing read below it.
        assert_eq!(paths(&shape.open), [path(LITERALS_DEPTH)]);
        assert!(shape.fields.contains(&path(LITERALS_DEPTH)));
        assert!(!shape.fields.contains(&path(LITERALS_DEPTH + 1)));
        // Within the limit nothing is open.
        assert!(deep(LITERALS_DEPTH - 1).open.is_empty());
    }

    #[test]
    fn constants_followed_past_the_depth_limit_are_unknown() {
        let chain = |n: usize| {
            let mut src = String::from("const c0 = { a: 1 };\n");
            for i in 1..=n {
                src += &format!("const c{i} = c{};\n", i - 1);
            }
            src + &format!("export function h(req, res) {{ res.json(c{n}); }}\n")
        };
        let known = express(&chain(LITERALS_DEPTH - 2)).unwrap();
        assert_eq!(paths(&known.fields), ["a"]);
        assert!(known.open.is_empty());
        let unknown = express(&chain(LITERALS_DEPTH + 8)).unwrap();
        assert_eq!(paths(&unknown.open), [""]);
        // Constants that name each other are no value either.
        let cycle = "const a = b;\nconst b = a;\nexport function h(req, res) { res.json(a); }\n";
        assert_eq!(paths(&express(cycle).unwrap().open), [""]);
    }

    #[test]
    fn a_file_past_the_budget_gives_no_shape_to_any_of_its_handlers() {
        let elements = "1,".repeat(LITERALS_BUDGET);
        let src = format!(
            "export function h(req, res) {{ res.json([{elements}]); }}\n\
             export function g(req, res) {{ res.json({{ a: 1 }}); }}\n"
        );
        let mut spent = Literals::default();
        assert!(read_with(&mut spent, &src, "h", Handler::Express).is_none());
        // The file's budget stays spent: the next handler of the file reads nothing either.
        assert!(read_with(&mut spent, &src, "g", Handler::Express).is_none());
        let mut fresh = Literals::default();
        assert!(read_with(&mut fresh, &src, "g", Handler::Express).is_some());
    }
}
