use std::collections::HashMap;

use crate::declared::Types;
use crate::lang::{Call, Part, Step, Ty, Value, property};
use crate::resolve::{Code, Target, scope_of};
use crate::shape::{Budget, Import, Shape, Source, Spent, Want, under};

// ponytail: one budget per handler file; real handlers spend a few hundred, a crafted chain of
// constants or a huge literal would otherwise cost the whole file's time.
const LITERALS_BUDGET: usize = 1 << 16;

// ponytail: real responses nest a handful of levels; the limit keeps a crafted chain of
// constants, or a literal nested thousands of levels, from costing the stack.
const LITERALS_DEPTH: usize = 32;

#[derive(Clone, Copy)]
pub enum Handler {
    Express,
    Next,
}

#[derive(Default)]
pub struct Literals {
    spent: Budget<LITERALS_BUDGET>,
    types: Types,
}

pub struct Responses<'a> {
    pub path: &'a str,
    pub bodies: Vec<Body<'a>>,
    pub returns: Option<(&'a str, &'a Ty)>,
    pub other: bool,
}

pub struct Body<'a> {
    pub scope: &'a str,
    pub value: &'a Value,
    pub cast: Option<&'a Ty>,
}

enum Sends {
    No,
    Body,
    Other,
}

impl Literals {
    pub fn types(&mut self) -> &mut Types {
        &mut self.types
    }

    fn reader<'c, 'a>(&'c mut self, code: &'c Code<'a>, path: &str) -> Reader<'c, 'a> {
        Reader {
            code,
            spent: self.spent.of(path),
            types: &mut self.types,
            seen: Vec::new(),
            shape: Shape::new(Source::Literals(path.to_string())),
        }
    }

    pub fn responses<'a>(
        &mut self,
        code: &Code<'a>,
        path: &str,
        name: &str,
        kind: Handler,
    ) -> Option<Responses<'a>> {
        let (&path, &facts) = code.files().get_key_value(path)?;
        let def = facts.defs.iter().find(|d| d.name == name)?;
        let handler = Target {
            path,
            name: &def.name,
        };
        let nested = format!("{name}.@");
        let from = facts
            .calls
            .partition_point(|c| c.range.start < def.range.start);
        let mut reader = self.reader(code, path);
        let mut body = Vec::new();
        for (i, call) in facts.calls.iter().enumerate().skip(from) {
            if call.range.start >= def.range.end {
                break;
            }
            reader.spent.charge()?;
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
        let mut responses = Responses {
            path,
            bodies: Vec::new(),
            returns: (code.returns(path, name)).map(|ty| (scope_of(&def.name), ty)),
            other: false,
        };
        for &i in &body {
            let call = &facts.calls[i];
            let sends = match kind {
                Handler::Express => express(&mut reader, handler, &facts.calls, &by_start, i)?,
                Handler::Next => next(&mut reader, handler, call, i)?,
            };
            match sends {
                Sends::No => {}
                Sends::Other => responses.other = true,
                Sends::Body => {
                    if let Some(arg) = code.args(path, i)?.first() {
                        responses.bodies.push(Body {
                            scope: &call.scope,
                            value: arg,
                            cast: code.arg_type(path, i, 0),
                        });
                    }
                }
            }
        }
        Some(responses)
    }

    pub fn shape<'a>(&mut self, code: &Code<'a>, responses: &Responses<'a>) -> Option<Shape> {
        if responses.bodies.is_empty() {
            return None;
        }
        let path = responses.path;
        let mut reader = self.reader(code, path);
        for &Body { scope, value, cast } in &responses.bodies {
            reader.read(path, scope, value, "", 0)?;
            if let Some(ty) = cast {
                reader.typed(path, scope, ty, "");
            }
            if let Some((scope, ty)) = responses.returns {
                reader.typed(path, scope, ty, "");
            }
        }
        if responses.other {
            reader.open("");
        }
        Some(reader.shape)
    }

    pub fn exported<'a>(&mut self, code: &Code<'a>, path: &'a str, name: &'a str) -> Option<Shape> {
        let (_, held) = code.constant(path, "", name)?;
        if !matches!(held, Value::Object(_) | Value::List(_)) {
            return None;
        }
        let mut reader = self.reader(code, path);
        reader.read(path, "", held, "", 1)?;
        if let Some(ty) = code.constant_type(path, name) {
            reader.typed(path, "", ty, "");
        }
        Some(reader.shape)
    }
}

struct Reader<'c, 'a> {
    code: &'c Code<'a>,
    spent: Spent<'c, LITERALS_BUDGET>,
    types: &'c mut Types,
    seen: Vec<(&'a str, &'a Ty)>,
    shape: Shape,
}

impl<'a> Reader<'_, 'a> {
    fn resolve(
        &mut self,
        mut path: &'a str,
        mut scope: &'a str,
        mut value: &'a Value,
        mut depth: usize,
    ) -> Option<(&'a str, &'a str, &'a Value, usize)> {
        static UNKNOWN: Value = Value::Unknown;
        while let Value::Ref(name) = value {
            self.spent.charge()?;
            depth += 1;
            let found = match depth <= LITERALS_DEPTH {
                true => (self.code.constant(path, scope, name))
                    .filter(|((_, constant), _)| !constant.contains('.')),
                false => None,
            };
            let Some(((at, constant), held)) = found else {
                let imported =
                    depth <= LITERALS_DEPTH && self.code.imported(path, scope, name).is_some();
                let stop = if imported { value } else { &UNKNOWN };
                return Some((path, scope, stop, depth));
            };
            if let Some(ty) = self.code.constant_type(at, constant) {
                self.seen.push((at, ty));
            }
            path = at;
            scope = "";
            value = held;
        }
        Some((path, scope, value, depth))
    }

    fn typed(&mut self, path: &'a str, scope: &'a str, ty: &'a Ty, at: &str) {
        let Some(read) = self.types.read(self.code, path, scope, ty) else {
            return self.open(at);
        };
        for open in &read.open {
            self.open(&under(at, open));
        }
        for import in read.imports.into_iter().filter(|i| i.want == Want::Type) {
            let at = under(at, &import.at);
            let want = Want::Opens;
            self.shape.imports.insert(Import { at, want, ..import });
        }
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

    fn read(
        &mut self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        at: &str,
        depth: usize,
    ) -> Option<()> {
        self.spent.charge()?;
        let mark = self.seen.len();
        let (path, scope, value, depth) = self.resolve(path, scope, value, depth)?;
        for (file, ty) in self.seen.split_off(mark) {
            self.typed(file, "", ty, at);
        }
        match value {
            Value::Object(properties) => self.object(path, scope, properties, at, depth)?,
            Value::List(items) if depth < LITERALS_DEPTH => {
                self.field(at);
                let element = format!("{at}[]");
                if items.is_empty() {
                    self.open(&element);
                }
                for item in items {
                    self.read(path, scope, item, &element, depth + 1)?;
                }
            }
            Value::Scalar(_) => self.scalar(at),
            Value::Str(parts) if parts.len() != 1 || matches!(parts[0], Part::Text(_)) => {
                self.scalar(at)
            }
            Value::Ref(name) => self.imported(path, scope, name, at),
            _ => self.open(at),
        }
        Some(())
    }

    fn imported(&mut self, path: &'a str, scope: &'a str, name: &'a str, at: &str) {
        let Some(imported) = self.code.imported(path, scope, name) else {
            return self.open(at);
        };
        self.field(at);
        let import = Import::new(at, path, imported, Want::Literals);
        self.shape.imports.insert(import);
    }

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
                let mark = self.seen.len();
                let spread = self.resolve(path, scope, value, depth + 1)?;
                for (file, ty) in self.seen.split_off(mark) {
                    self.typed(file, "", ty, at);
                }
                match spread {
                    (path, scope, Value::Object(inner), depth) => {
                        self.object(path, scope, inner, at, depth)?
                    }
                    (_, _, Value::Undefined, _) => {}
                    (path, scope, Value::Ref(name), _) => self.imported(path, scope, name, at),
                    _ => self.open(at),
                }
            } else if key.is_empty() || key.contains(['.', '[']) {
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

    fn status(&mut self, path: &'a str, scope: &'a str, init: &'a Value) -> Option<Option<u32>> {
        let (path, scope, init, depth) = self.resolve(path, scope, init, 0)?;
        let Value::Object(properties) = init else {
            return Some(None);
        };
        let Ok(Some(status)) = property(properties, "status") else {
            return Some(None);
        };
        self.number(path, scope, status, depth)
    }

    fn number(
        &mut self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        depth: usize,
    ) -> Option<Option<u32>> {
        Some(match self.resolve(path, scope, value, depth)? {
            (_, _, Value::Scalar(number), _) => *number,
            _ => None,
        })
    }
}

fn error(status: Option<u32>) -> bool {
    status.is_some_and(|s| !(200..300).contains(&s))
}

const QUIET: [&str; 14] = [
    "status",
    "set",
    "header",
    "setHeader",
    "type",
    "contentType",
    "cookie",
    "clearCookie",
    "append",
    "vary",
    "location",
    "links",
    "attachment",
    "get",
];

fn express<'a>(
    reader: &mut Reader<'_, 'a>,
    handler: Target<'a>,
    calls: &'a [Call],
    by_start: &HashMap<usize, Vec<usize>>,
    i: usize,
) -> Option<Sends> {
    let (code, path) = (reader.code, handler.path);
    let call = &calls[i];
    let is_res = |c: &'a Call, name: &'a str| code.is_param(path, &c.scope, name, handler, 1);
    let args = code.args(path, i).unwrap_or_default();
    let handed = args
        .iter()
        .any(|a| matches!(a, Value::Ref(name) if is_res(call, name)));
    if handed {
        return Some(Sends::Other);
    }
    if code.is_param(path, &call.scope, &call.callee, handler, 2) {
        let error = match args.first() {
            None | Some(Value::Undefined) => false,
            Some(Value::Str(parts)) => {
                !matches!(parts.as_slice(), [Part::Text(t)] if t == "route" || t == "router")
            }
            Some(_) => true,
        };
        return Some(if error { Sends::No } else { Sends::Other });
    }
    let Some((receiver, method)) = call.callee.rsplit_once('.') else {
        return Some(Sends::No);
    };
    let own = match (method, args.first()) {
        ("sendStatus", Some(arg)) => reader.number(path, &call.scope, arg, 0)?,
        _ => None,
    };
    if receiver != "(call_expression)" {
        return Some(match is_res(call, receiver) {
            true => response(method, own),
            false => Sends::No,
        });
    }
    let mut chain = Vec::new();
    for &j in &by_start[&call.range.start] {
        reader.spent.charge()?;
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
        return Some(Sends::No);
    }
    let set = (chain.iter().rev()).find(|&&j| calls[j].callee.rsplit('.').next() == Some("status"));
    let Some((set, arg)) = set.and_then(|&j| Some((j, code.args(path, j)?.first()?))) else {
        return Some(response(method, own));
    };
    let status = reader.number(path, &calls[set].scope, arg, 0)?;
    Some(response(method, own.or(status)))
}

fn response(method: &str, status: Option<u32>) -> Sends {
    match method {
        _ if error(status) => Sends::No,
        "json" | "send" => Sends::Body,
        m if QUIET.contains(&m) => Sends::No,
        _ => Sends::Other,
    }
}

fn next<'a>(
    reader: &mut Reader<'_, 'a>,
    handler: Target<'a>,
    call: &'a Call,
    i: usize,
) -> Option<Sends> {
    let (code, path) = (reader.code, handler.path);
    let next = |name: &str| {
        matches!(
            code.imported(path, &call.scope, &call.callee),
            Some(("next/server", imported)) if imported == name
        )
    };
    let init = code.args(path, i).and_then(|a| a.get(1));
    let (sends, constructs) = match call.callee.as_str() {
        "NextResponse.json" => (next("NextResponse.json"), false),
        "Response.json" => (code.is_global(path, &call.scope, "Response"), false),
        "NextResponse" => (false, next("NextResponse")),
        "Response" => (false, code.is_global(path, &call.scope, "Response")),
        _ => (false, false),
    };
    if let Some(init) = init.filter(|_| constructs)
        && error(reader.status(path, &call.scope, init)?)
    {
        return Some(Sends::No);
    }
    if !sends {
        let returned = call.scope == handler.name
            && (code.uses(path, i)).is_some_and(|u| u.iter().any(|c| c.contains(&Step::Return)));
        return Some(if returned { Sends::Other } else { Sends::No });
    }
    let Some(init) = init else {
        return Some(Sends::Body);
    };
    Some(match error(reader.status(path, &call.scope, init)?) {
        true => Sends::No,
        false => Sends::Body,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{Handler, LITERALS_BUDGET, LITERALS_DEPTH, Literals};
    use crate::lang::{self, LanguagePack, ts::TsJs};
    use crate::resolve::{Code, Resolver};
    use crate::shape::{Shape, Want};

    fn read_with(reader: &mut Literals, src: &str, name: &str, kind: Handler) -> Option<Shape> {
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        let code = Code::new(&resolver, &files, lang::pack(0));
        let responses = reader.responses(&code, "a.ts", name, kind)?;
        reader.shape(&code, &responses)
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
        assert_eq!(paths(&shape.open), ["", "got", "mapped", "none[]"]);
        let scalars = ["kind", "n", "name", "ok", "rows[].id"];
        assert_eq!(paths(&shape.scalars), scalars);
    }

    #[test]
    fn a_constant_a_package_exports_is_left_for_the_query_unless_a_binding_hides_it() {
        let shape = express(
            "import { BASE, ROW } from '@acme/shared';\n\
             export function h(req, res) {\n\
               res.json({ ...BASE, row: ROW, ok: true });\n\
             }\n",
        )
        .unwrap();
        let at: Vec<(&str, &str)> = (shape.imports.iter())
            .map(|i| (i.at.as_str(), i.name.as_str()))
            .collect();
        assert_eq!(at, [("", "BASE"), ("row", "ROW")]);
        assert!(shape.open.is_empty());
        let shape = express(
            "import { BASE } from '@acme/shared';\n\
             export function h(req, res) {\n\
               try { load(); } catch (BASE) { res.json({ ...BASE }); }\n\
             }\n",
        )
        .unwrap();
        assert!(shape.imports.is_empty());
        assert_eq!(paths(&shape.open), [""]);
    }

    #[test]
    fn a_body_typed_with_a_package_type_takes_its_open_parts_from_the_query() {
        let shape = next(
            "import { NextResponse } from 'next/server';\n\
             import type { Reply, Meta } from '@acme/shared';\n\
             const META: Meta = { a: 'b' };\n\
             export async function h(): Promise<NextResponse<Reply>> {\n\
               return NextResponse.json({ ok: true, meta: META });\n\
             }\n",
        )
        .unwrap();
        let imports: Vec<(&str, &str, Want)> = (shape.imports.iter())
            .map(|i| (i.at.as_str(), i.name.as_str(), i.want))
            .collect();
        assert_eq!(
            imports,
            [("", "Reply", Want::Opens), ("meta", "Meta", Want::Opens)]
        );
        assert!(shape.open.is_empty());
    }

    #[test]
    fn an_index_signature_in_a_type_of_the_returned_value_opens_that_part() {
        let types = "interface Meta { [k: string]: string }\n\
                     interface Reply { [k: string]: unknown; ok: boolean; meta: Meta }\n\
                     interface Plain { ok: boolean; meta: Meta }\n";
        let opens = |handler: &str| {
            let shape = express(&format!("{types}{handler}")).unwrap();
            paths(&shape.open).join(",")
        };
        for (handler, open) in [
            (
                "const body: Reply = { ok: true };\nexport function h(req, res) { res.json(body); }",
                ",meta",
            ),
            (
                "export function h(req, res) { res.json({ ok: true } as Reply); }",
                ",meta",
            ),
            (
                "export function h(req, res): Reply { res.json({ ok: true }); }",
                ",meta",
            ),
            (
                "export function h(req, res) { res.json({ ok: true, meta: {} } as Plain); }",
                "meta",
            ),
            (
                "export function h(req, res) { res.json({ ok: true } as Elsewhere); }",
                "",
            ),
            (
                "export function h(req, res) { res.json({ ok: true }); }",
                "",
            ),
        ] {
            assert_eq!(opens(handler), open, "{handler}");
        }
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
        let hidden =
            "export function h(req, res) {\n  { const res = other(); res.json({ a: 1 }); }\n}\n";
        assert!(express(hidden).is_none());
        assert!(express("export function h(res) {\n  res.json({ a: 1 });\n}\n").is_none());
        let bound =
            "export function h() {\n  const Response = other;\n  Response.json({ a: 1 });\n}\n";
        assert!(next(bound).is_none());
        let other = "import { NextResponse } from 'elsewhere';\n\
                     export function h() {\n  NextResponse.json({ a: 1 });\n}\n";
        assert!(next(other).is_none());
    }

    #[test]
    fn a_next_response_sent_another_way_opens_the_shape() {
        let read = |body: &str| {
            let src = format!(
                "import {{ NextResponse }} from 'next/server';\n\
                 export function h(req) {{\n  if (c) return NextResponse.json({{ cached: true }});\n  \
                 {body}\n}}\n"
            );
            next(&src).unwrap()
        };
        for body in [
            "return ok({ user });",
            "return proxy(req);",
            "return new Response(JSON.stringify(x));",
            "return NextResponse.redirect(url);",
            "const r = helper(); return r;",
        ] {
            let shape = read(body);
            assert_eq!(paths(&shape.fields), ["cached"], "{body}");
            assert_eq!(paths(&shape.open), [""], "{body}");
        }
        for body in [
            "rows.map((r) => { return f(r); }); return NextResponse.json({ a: 1 });",
            "log(x); return NextResponse.json({ a: 1 });",
        ] {
            let shape = read(body);
            assert_eq!(paths(&shape.fields), ["a", "cached"], "{body}");
            assert!(shape.open.is_empty(), "{body}");
        }
    }

    #[test]
    fn an_express_response_sent_another_way_opens_the_shape() {
        let read = |body: &str| {
            let src = format!(
                "export function h(req, res, next) {{\n  res.json({{ a: 1 }});\n  {body}\n}}\n"
            );
            express(&src).unwrap()
        };
        for body in [
            "proxy(req, res);",
            "res.redirect('/x');",
            "res.end();",
            "res.render('v', {});",
            "res.status(200).end();",
            "res.status(200).sendFile(f);",
            "next();",
        ] {
            assert_eq!(paths(&read(body).open), [""], "{body}");
        }
        for body in [
            "res.set('x', 'y');",
            "res.status(201);",
            "log(req);",
            "other.end();",
        ] {
            assert!(read(body).open.is_empty(), "{body}");
        }
    }

    #[test]
    fn any_response_with_a_literal_status_outside_2xx_is_left_out_whatever_its_form() {
        let express = |body: &str| {
            let src = format!(
                "const GONE = 410;\nexport function h(req, res, next) {{\n  res.json({{ a: 1 }});\n  \
                 {body}\n}}\n"
            );
            express(&src).unwrap()
        };
        for body in [
            "res.sendStatus(404);",
            "res.status(404).end();",
            "res.status(GONE).render('missing');",
            "res.set('x', 'y').status(500).sendFile(f);",
            "next(err);",
            "next(new Error('gone'));",
        ] {
            assert!(express(body).open.is_empty(), "{body}");
        }
        for body in [
            "res.sendStatus(200);",
            "res.sendStatus(code);",
            "res.status(code).end();",
            "next();",
            "next('route');",
            "next('router');",
        ] {
            assert_eq!(paths(&express(body).open), [""], "{body}");
        }
        let next = |body: &str| {
            let src = format!(
                "import {{ NextResponse }} from 'next/server';\nconst GONE = 410;\n\
                 export function h(req) {{\n  if (c) {{ {body} }}\n  \
                 return NextResponse.json({{ a: 1 }});\n}}\n"
            );
            next(&src).unwrap()
        };
        for body in [
            "return new Response(null, { status: 404 });",
            "return new NextResponse('no', { status: 401 });",
            "return new Response(null, { status: GONE });",
        ] {
            assert!(next(body).open.is_empty(), "{body}");
        }
        for body in [
            "return new Response(null, { status: 200 });",
            "return new Response(null, init);",
            "return new Response('ok');",
        ] {
            assert_eq!(paths(&next(body).open), [""], "{body}");
        }
    }

    #[test]
    fn a_function_local_constant_is_open() {
        let shape = express(
            "export function h(req, res) {\n  const body = { items };\n  \
             if (more) body.next = cursor;\n  res.json(body);\n}\n",
        )
        .unwrap();
        assert_eq!(paths(&shape.open), [""]);
        let shape =
            express("const BASE = { a: 1 };\nexport function h(req, res) { res.json(BASE); }\n")
                .unwrap();
        assert_eq!(paths(&shape.fields), ["a"]);
        assert!(shape.open.is_empty());
    }

    #[test]
    fn a_name_that_hides_res_is_no_response() {
        for body in [
            "try { f(); } catch (res) { res.json({ a: 1 }); }",
            "const { res } = other; res.json({ a: 1 });",
            "items.forEach((res) => res.json({ a: 1 }));",
        ] {
            let src = format!("export function h(req, res) {{\n  {body}\n}}\n");
            assert!(express(&src).is_none(), "{body}");
        }
    }

    #[test]
    fn a_getter_or_method_in_a_literal_is_present_and_open() {
        let shape = express(
            "const BODY = { get total() { return 1; }, count() { return 2; } };\n\
             export function h(req, res) { res.json({ inner: BODY, a: 1 }); }\n",
        )
        .unwrap();
        assert_eq!(
            paths(&shape.fields),
            ["a", "inner", "inner.count", "inner.total"]
        );
        assert_eq!(paths(&shape.open), ["inner.count", "inner.total"]);
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
        assert_eq!(paths(&shape.open), [path(LITERALS_DEPTH)]);
        assert!(shape.fields.contains(&path(LITERALS_DEPTH)));
        assert!(!shape.fields.contains(&path(LITERALS_DEPTH + 1)));
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
        assert!(read_with(&mut spent, &src, "g", Handler::Express).is_none());
        let mut fresh = Literals::default();
        assert!(read_with(&mut fresh, &src, "g", Handler::Express).is_some());
    }
}
