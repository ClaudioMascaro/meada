//! The pure functions static evaluation registers with `hcl-rs`'s evaluator (ADR 0025): string
//! functions, collection functions and conversions, and `jsonencode`, each over the values it is
//! given and nothing else (no file, clock, randomness or network). A call to any other function
//! is undefined, so the expression holding it stays unresolved. Each follows Terraform's function
//! of the same name; a case it does not cover (a `replace` pattern, a `format` verb other than
//! `%s`, `%d`, `%v`, `%q` and `%%`) is an error, which leaves the call unresolved too.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use hcl::eval::{Context, FuncArgs, FuncDef, ParamType};
use hcl::{Map, Number, Value};

type Out = Result<Value, String>;

/// The widest a `format` verb may pad (`%256s`); past it the call is unresolved.
// ponytail: a name pads a number to two or three digits; 256 keeps `iac::cost` a tight bound.
pub(super) const WIDTH_MAX: usize = 256;

/// `ctx` with every function declared.
pub(super) fn declare(ctx: &mut Context) {
    use ParamType::{Any, String as Str};
    let fixed = |params: Vec<ParamType>, f: fn(FuncArgs) -> Out| FuncDef::new(f, params);
    let variadic = |params: Vec<ParamType>, rest: ParamType, f: fn(FuncArgs) -> Out| {
        FuncDef::builder()
            .params(params)
            .variadic_param(rest)
            .build(f)
    };
    let list = || ParamType::array_of(Any);
    let map = || ParamType::object_of(Any);
    let funcs: [(&str, FuncDef); 28] = [
        ("format", variadic(vec![Str], Any, format)),
        ("formatlist", variadic(vec![Str], Any, formatlist)),
        ("join", variadic(vec![Str], list(), join)),
        ("split", fixed(vec![Str, Str], split)),
        ("lower", fixed(vec![Str], |a| text(a, str::to_lowercase))),
        ("upper", fixed(vec![Str], |a| text(a, str::to_uppercase))),
        ("title", fixed(vec![Str], |a| text(a, title))),
        ("trim", fixed(vec![Str, Str], trim)),
        ("trimprefix", fixed(vec![Str, Str], trimprefix)),
        ("trimsuffix", fixed(vec![Str, Str], trimsuffix)),
        (
            "trimspace",
            fixed(vec![Str], |a| text(a, |s| s.trim().into())),
        ),
        ("chomp", fixed(vec![Str], |a| text(a, chomp))),
        ("replace", fixed(vec![Str, Str, Str], replace)),
        (
            "substr",
            fixed(vec![Str, ParamType::Number, ParamType::Number], substr),
        ),
        ("concat", variadic(vec![], list(), concat)),
        ("coalesce", variadic(vec![], Any, coalesce)),
        ("element", fixed(vec![list(), ParamType::Number], element)),
        ("keys", fixed(vec![map()], keys)),
        ("values", fixed(vec![map()], values)),
        ("lookup", variadic(vec![map(), Str], Any, lookup)),
        ("merge", variadic(vec![], ParamType::nullable(map()), merge)),
        ("length", fixed(vec![Any], length)),
        ("toset", fixed(vec![list()], toset)),
        ("tolist", fixed(vec![list()], tolist)),
        ("tomap", fixed(vec![map()], tomap)),
        ("tostring", fixed(vec![Any], tostring)),
        ("tonumber", fixed(vec![Any], tonumber)),
        ("jsonencode", fixed(vec![Any], jsonencode)),
    ];
    for (name, def) in funcs {
        ctx.declare_func(name, def);
    }
    declare_exact(ctx);
}

/// `ctx` with `meada_exact` alone, the call `Exact` puts in: a var file calls no function.
pub(super) fn declare_exact(ctx: &mut Context) {
    let def = FuncDef::builder()
        .params(vec![ParamType::String])
        .variadic_param(ParamType::Any)
        .build(super::exact_call);
    ctx.declare_func("meada_exact", def);
}

fn string(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

fn text(args: FuncArgs, f: fn(&str) -> String) -> Out {
    Ok(Value::String(f(string(&args[0]))))
}

/// A scalar as a template shows it; `None` for a null, a list or an object.
pub(super) fn shown(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Terraform's `format`: `%s`, `%d`, `%v`, `%q` and `%%`, with an optional `-` or `0` flag and
/// a width.
fn formatted(spec: &str, args: &[Value]) -> Result<String, String> {
    let mut out = String::new();
    let mut args = args.iter();
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let (mut left, mut zero, mut width) = (false, false, 0usize);
        while let Some(&f) = chars.peek().filter(|f| matches!(f, '-' | '0')) {
            (left, zero) = (left || f == '-', zero || f == '0');
            chars.next();
        }
        while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
            width = width.saturating_mul(10).saturating_add(d as usize);
            chars.next();
        }
        let verb = chars.next().ok_or("a verb is missing")?;
        if verb == '%' {
            out.push('%');
            continue;
        }
        let arg = args.next().ok_or("too few arguments")?;
        let piece = match verb {
            's' => shown(arg).ok_or("%s of a collection")?,
            'd' => match arg.as_number().and_then(Number::as_i64) {
                Some(n) => n.to_string(),
                None => return Err("%d of a non-integer".into()),
            },
            'v' => shown(arg).map_or_else(|| encoded(arg), Ok)?,
            'q' => serde_json::to_string(arg.as_str().ok_or("%q of a non-string")?)
                .map_err(|e| e.to_string())?,
            _ => return Err(format!("the verb %{verb}")),
        };
        if width > WIDTH_MAX {
            return Err("a width past its ceiling".into());
        }
        let pad = width.saturating_sub(piece.chars().count());
        let fill = if zero && !left && verb == 'd' {
            '0'
        } else {
            ' '
        };
        let padding: String = std::iter::repeat_n(fill, pad).collect();
        match (left, fill, piece.strip_prefix('-')) {
            (true, _, _) => out.extend([piece.as_str(), &padding]),
            (false, '0', Some(digits)) => out.extend(["-", &padding, digits]),
            _ => out.extend([padding.as_str(), &piece]),
        }
    }
    Ok(out)
}

fn format(args: FuncArgs) -> Out {
    Ok(Value::String(formatted(string(&args[0]), &args[1..])?))
}

/// Terraform's `formatlist`: every list argument has the same length, a scalar repeats.
fn formatlist(args: FuncArgs) -> Out {
    let rest = &args[1..];
    let lengths: Vec<usize> = rest
        .iter()
        .filter_map(|a| a.as_array().map(Vec::len))
        .collect();
    let n = lengths.first().copied().unwrap_or(1);
    if lengths.iter().any(|&l| l != n) {
        return Err("lists of different lengths".into());
    }
    (0..n)
        .map(|i| {
            let row: Vec<Value> = (rest.iter())
                .map(|a| a.as_array().map_or(a, |l| &l[i]).clone())
                .collect();
            formatted(string(&args[0]), &row).map(Value::String)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

fn join(args: FuncArgs) -> Out {
    let mut parts = Vec::new();
    for list in &args[1..] {
        for item in list.as_array().into_iter().flatten() {
            parts.push(shown(item).ok_or("join of a collection or null")?);
        }
    }
    Ok(Value::String(parts.join(string(&args[0]))))
}

fn split(args: FuncArgs) -> Out {
    let (sep, s) = (string(&args[0]), string(&args[1]));
    if sep.is_empty() {
        return Err("an empty separator".into());
    }
    Ok(Value::Array(s.split(sep).map(Value::from).collect()))
}

/// Go's `strings.Title`, as Terraform's `title`: a letter after a separator is upper case.
fn title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut after = true;
    for c in s.chars() {
        match after {
            true => out.extend(c.to_uppercase()),
            false => out.push(c),
        }
        after = match c.is_ascii() {
            true => !(c.is_ascii_alphanumeric() || c == '_'),
            false => c.is_whitespace(),
        };
    }
    out
}

fn trim(args: FuncArgs) -> Out {
    let cut: HashSet<char> = string(&args[1]).chars().collect();
    Ok(Value::from(
        string(&args[0]).trim_matches(|c| cut.contains(&c)),
    ))
}

fn trimprefix(args: FuncArgs) -> Out {
    let s = string(&args[0]);
    Ok(Value::from(s.strip_prefix(string(&args[1])).unwrap_or(s)))
}

fn trimsuffix(args: FuncArgs) -> Out {
    let s = string(&args[0]);
    Ok(Value::from(s.strip_suffix(string(&args[1])).unwrap_or(s)))
}

fn chomp(s: &str) -> String {
    s.trim_end_matches(['\n', '\r']).into()
}

/// A literal substring only: a `/.../` pattern is a regular expression, left unresolved.
fn replace(args: FuncArgs) -> Out {
    let (s, from, to) = (string(&args[0]), string(&args[1]), string(&args[2]));
    if from.is_empty() || (from.len() > 1 && from.starts_with('/') && from.ends_with('/')) {
        return Err("a pattern or an empty substring".into());
    }
    Ok(Value::String(s.replace(from, to)))
}

/// Characters from `offset` (from the end when negative), `length` of them or, at -1, the rest.
fn substr(args: FuncArgs) -> Out {
    let chars: Vec<char> = string(&args[0]).chars().collect();
    let int = |v: &Value| v.as_i64().ok_or("a non-integer");
    let (offset, length) = (int(&args[1])?, int(&args[2])?);
    let len = chars.len() as i64;
    let start = if offset < 0 { len + offset } else { offset };
    if start < 0 || start > len {
        return Err("an offset out of the string".into());
    }
    let end = match length {
        -1 => len,
        l if l < 0 => return Err("a negative length".into()),
        l => start + l.min(len - start),
    };
    Ok(Value::String(
        chars[start as usize..end as usize].iter().collect(),
    ))
}

fn concat(args: FuncArgs) -> Out {
    let items = args
        .iter()
        .flat_map(|l| l.as_array().cloned().unwrap_or_default());
    Ok(Value::Array(items.collect()))
}

fn coalesce(args: FuncArgs) -> Out {
    (args.iter())
        .find(|v| !v.is_null() && v.as_str() != Some(""))
        .cloned()
        .ok_or_else(|| "no non-empty argument".into())
}

fn element(args: FuncArgs) -> Out {
    let list = args[0].as_array().ok_or("not a list")?;
    let at = args[1].as_u64().ok_or("a negative or fractional index")?;
    match list.len() {
        0 => Err("an empty list".into()),
        n => Ok(list[(at % n as u64) as usize].clone()),
    }
}

fn sorted(v: &Value) -> Vec<(&String, &Value)> {
    let mut entries: Vec<_> = v.as_object().into_iter().flatten().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
}

fn keys(args: FuncArgs) -> Out {
    let keys = sorted(&args[0])
        .into_iter()
        .map(|(k, _)| Value::from(k.as_str()));
    Ok(Value::Array(keys.collect()))
}

fn values(args: FuncArgs) -> Out {
    let values = sorted(&args[0]).into_iter().map(|(_, v)| v.clone());
    Ok(Value::Array(values.collect()))
}

fn lookup(args: FuncArgs) -> Out {
    let found = args[0].as_object().and_then(|m| m.get(string(&args[1])));
    match (found, args.get(2)) {
        (Some(v), _) | (None, Some(v)) => Ok(v.clone()),
        (None, None) => Err("no such key and no default".into()),
    }
}

fn merge(args: FuncArgs) -> Out {
    let mut out = Map::new();
    for m in args.iter().filter_map(Value::as_object) {
        out.extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    out.sort_keys();
    Ok(Value::Object(out))
}

fn length(args: FuncArgs) -> Out {
    let n = match &args[0] {
        Value::String(s) => s.chars().count(),
        Value::Array(l) => l.len(),
        Value::Object(m) => m.len(),
        _ => return Err("the length of a scalar".into()),
    };
    Ok(Value::from(n as u64))
}

/// The distinct elements of a list in the order of cty's sets: strings bytewise, numbers by value,
/// `false` before `true`, a null last, after `unified`. A set of collections is in hash order:
/// unresolved.
fn toset(args: FuncArgs) -> Out {
    let list = args[0].as_array().map_or(&[][..], Vec::as_slice);
    distinct(unified(list)?).map(Value::Array)
}

/// `items`, of one type, as a set orders them (`toset`).
fn distinct(items: Vec<Value>) -> Result<Vec<Value>, String> {
    let (nulls, mut items): (Vec<_>, Vec<_>) = items.into_iter().partition(Value::is_null);
    if items.iter().any(|v| v.is_array() || v.is_object()) {
        return Err("the order of a set of collections".into());
    }
    items.sort_unstable_by(order);
    items.dedup_by(|a, b| order(a, b).is_eq());
    items.extend(nulls.into_iter().take(1));
    Ok(items)
}

/// The element type of a variable's declared `set(...)` or `list(...)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Elem {
    String,
    Number,
    Bool,
    Any,
    /// A collection or an object: Meada makes no conversion to it.
    Other,
}

/// `v`, a list, tuple or set, as the set of `elem` that Terraform makes of it: the elements
/// converted to `elem`, then sorted and deduplicated as cty does. `Err` where Meada cannot make
/// the conversion with certainty, and for a value that is no list.
pub(super) fn to_set(v: &Value, elem: Elem) -> Result<Value, String> {
    collection(v, elem, distinct)
}

/// `v` as the list of `elem` that Terraform makes of it: the elements converted, in their order.
pub(super) fn to_list(v: &Value, elem: Elem) -> Result<Value, String> {
    collection(v, elem, Ok)
}

impl Elem {
    /// The element type written `name` in a `set(...)` or `list(...)` constraint.
    pub(super) fn named(name: &str) -> Elem {
        match name.trim() {
            "string" => Elem::String,
            "number" => Elem::Number,
            "bool" => Elem::Bool,
            "any" => Elem::Any,
            _ => Elem::Other,
        }
    }

    /// Whether this is a primitive type: `string`, `number` or `bool`.
    pub(super) fn primitive(self) -> bool {
        matches!(self, Elem::String | Elem::Number | Elem::Bool)
    }
}

fn collection(
    v: &Value,
    elem: Elem,
    shape: fn(Vec<Value>) -> Result<Vec<Value>, String>,
) -> Result<Value, String> {
    match v {
        Value::Null => Ok(Value::Null),
        Value::Array(items) => {
            let items = match elem {
                Elem::Any => unified(items)?,
                Elem::Other => return Err("an element type Meada does not convert".into()),
                _ => (items.iter())
                    .map(|v| scalar_as(v, elem))
                    .collect::<Result<_, _>>()?,
            };
            shape(items).map(Value::Array)
        }
        _ => Err("not a list".into()),
    }
}

/// A scalar converted to the scalar type `elem`, as cty's `convert` does; a null stays null.
fn scalar_as(v: &Value, elem: Elem) -> Result<Value, String> {
    match (elem, v) {
        (_, Value::Null) => Ok(Value::Null),
        (Elem::String, Value::String(_))
        | (Elem::Number, Value::Number(_))
        | (Elem::Bool, Value::Bool(_)) => Ok(v.clone()),
        (Elem::String, _) => shown(v)
            .map(Value::String)
            .ok_or_else(|| "a collection as a string".into()),
        (Elem::Number, Value::String(s)) => number(s).map(Value::Number),
        (Elem::Bool, Value::String(s)) if s == "true" || s == "false" => {
            Ok(Value::Bool(s == "true"))
        }
        _ => Err("an element that does not convert".into()),
    }
}

fn tolist(args: FuncArgs) -> Out {
    unified(args[0].as_array().map_or(&[][..], Vec::as_slice)).map(Value::Array)
}

fn tomap(args: FuncArgs) -> Out {
    let Some(map) = args[0].as_object() else {
        return Ok(args[0].clone());
    };
    let values: Vec<Value> = map.values().cloned().collect();
    // A map, unlike a list, takes a null beside values of one type, which stay as they are.
    let mut given = values.iter().filter(|v| !v.is_null()).map(type_of);
    let first = given.next();
    let values = match unified(&values) {
        Err(_) if given.all(|t| Some(t) == first) => values,
        unified => unified?,
    };
    Ok(Value::Object(map.keys().cloned().zip(values).collect()))
}

/// Elements converted to their one type, as cty's `convert` does for a list or a set.
fn unified(items: &[Value]) -> Result<Vec<Value>, String> {
    let ty = unify(&items.iter().map(type_of).collect::<Vec<_>>())?;
    Ok(items.iter().map(|v| converted(v, &ty)).collect())
}

/// A value's type as cty reads a literal: a list is a tuple, a null has any type (`Dynamic`).
/// Unification also gives lists and maps.
#[derive(Clone, PartialEq)]
enum Type {
    Dynamic,
    Bool,
    Number,
    String,
    List(Box<Type>),
    Map(Box<Type>),
    Tuple(Vec<Type>),
    Object(BTreeMap<String, Type>),
}

fn type_of(v: &Value) -> Type {
    match v {
        Value::Null => Type::Dynamic,
        Value::Bool(_) => Type::Bool,
        Value::Number(_) => Type::Number,
        Value::String(_) => Type::String,
        Value::Array(items) => Type::Tuple(items.iter().map(type_of).collect()),
        Value::Object(map) => {
            Type::Object(map.iter().map(|(k, v)| (k.clone(), type_of(v))).collect())
        }
    }
}

/// The type cty's `unify` gives values of these types: objects of one set of attributes unify
/// attribute by attribute, others to a map; tuples of one length element by element, others to a
/// list; a string takes any scalar. A null beside a collection unifies to cty's dynamic type, which
/// a list refuses and nested values seldom pass: unresolved.
fn unify(types: &[Type]) -> Result<Type, String> {
    let count = |kind: fn(&Type) -> bool| types.iter().filter(|t| kind(t)).count();
    let nulls = count(|t| *t == Type::Dynamic);
    let objects = count(|t| matches!(t, Type::Object(_)));
    let tuples = count(|t| matches!(t, Type::Tuple(_)));
    if (objects > 0 || tuples > 0) && nulls > 0 {
        return Err("a null beside a collection".into());
    }
    if objects == types.len() && objects > 0 {
        let attrs: Vec<_> = (types.iter())
            .filter_map(|t| {
                if let Type::Object(a) = t {
                    Some(a)
                } else {
                    None
                }
            })
            .collect();
        if attrs.iter().any(|a| !a.keys().eq(attrs[0].keys())) {
            let all: Vec<Type> = attrs.iter().flat_map(|a| a.values().cloned()).collect();
            return Ok(Type::Map(Box::new(unify(&all)?)));
        }
        let across = |k: &String| unify(&attrs.iter().map(|a| a[k].clone()).collect::<Vec<_>>());
        return (attrs[0].keys())
            .map(|k| Ok((k.clone(), across(k)?)))
            .collect::<Result<_, String>>()
            .map(Type::Object);
    }
    if tuples == types.len() && tuples > 0 {
        let elems: Vec<_> = (types.iter())
            .filter_map(|t| {
                if let Type::Tuple(e) = t {
                    Some(e)
                } else {
                    None
                }
            })
            .collect();
        if elems.iter().any(|e| e.len() != elems[0].len()) {
            let all: Vec<Type> = elems.iter().flat_map(|e| e.iter().cloned()).collect();
            return Ok(Type::List(Box::new(unify(&all)?)));
        }
        let across = |i: usize| unify(&elems.iter().map(|e| e[i].clone()).collect::<Vec<_>>());
        return (0..elems[0].len())
            .map(across)
            .collect::<Result<_, String>>()
            .map(Type::Tuple);
    }
    if objects > 0 || tuples > 0 {
        return Err("a collection beside a scalar or another kind of collection".into());
    }
    if types.contains(&Type::String) {
        return Ok(Type::String);
    }
    let mut given = types.iter().filter(|t| **t != Type::Dynamic);
    let first = given.next().cloned().unwrap_or(Type::Dynamic);
    match given.all(|t| *t == first) {
        true => Ok(first),
        false => Err("a number with a bool".into()),
    }
}

/// `v` converted to `ty`, a type `unify` gave it: only a scalar becoming a string changes.
fn converted(v: &Value, ty: &Type) -> Value {
    match (v, ty) {
        (Value::Array(items), Type::List(t)) => {
            Value::Array(items.iter().map(|i| converted(i, t)).collect())
        }
        (Value::Array(items), Type::Tuple(ts)) => {
            Value::Array(items.iter().zip(ts).map(|(i, t)| converted(i, t)).collect())
        }
        (Value::Object(map), Type::Map(t)) => Value::Object(
            map.iter()
                .map(|(k, x)| (k.clone(), converted(x, t)))
                .collect(),
        ),
        (Value::Object(map), Type::Object(ts)) => Value::Object(
            map.iter()
                .map(|(k, x)| (k.clone(), converted(x, &ts[k])))
                .collect(),
        ),
        (_, Type::String) => shown(v).map_or(Value::Null, Value::String),
        _ => v.clone(),
    }
}

/// Two scalars of one type as cty orders them in a set.
fn order(a: &Value, b: &Value) -> Ordering {
    let int = |n: &Number| (n.as_i64().map(i128::from)).or_else(|| n.as_u64().map(i128::from));
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => match (int(a), int(b)) {
            (Some(a), Some(b)) => a.cmp(&b),
            _ => (a.as_f64().partial_cmp(&b.as_f64())).unwrap_or(Ordering::Equal),
        },
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        _ => string(a).cmp(string(b)),
    }
}

fn tostring(args: FuncArgs) -> Out {
    match &args[0] {
        Value::Null => Ok(Value::Null),
        v => shown(v)
            .map(Value::String)
            .ok_or_else(|| "a collection".into()),
    }
}

fn tonumber(args: FuncArgs) -> Out {
    match &args[0] {
        Value::Null => Ok(Value::Null),
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::String(s) => number(s).map(Value::Number),
        _ => Err("not a number".into()),
    }
}

/// The number a string is, if Terraform and Meada print it as it is written.
fn number(s: &str) -> Result<Number, String> {
    (s.parse::<i64>().ok().map(Number::from))
        .or_else(|| s.parse::<u64>().ok().map(Number::from))
        .or_else(|| s.parse::<f64>().ok().and_then(Number::from_f64))
        .filter(|n| super::printed_as_written(n, s))
        .ok_or_else(|| format!("{s:?} is no number"))
}

/// JSON as Terraform's `jsonencode` writes it: compact, keys in lexical order, `<`, `>` and `&`
/// escaped as Go does.
fn encoded(v: &Value) -> Result<String, String> {
    let json = serde_json::to_string(&super::lexical(v.clone())).map_err(|e| e.to_string())?;
    Ok(json
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026"))
}

fn jsonencode(args: FuncArgs) -> Out {
    encoded(&args[0]).map(Value::String)
}
