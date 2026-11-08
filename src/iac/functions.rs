//! The pure functions static evaluation registers with `hcl-rs`'s evaluator (ADR 0025): string
//! functions, collection functions and conversions, and `jsonencode`, each over the values it is
//! given and nothing else (no file, clock, randomness or network). A call to any other function
//! is undefined, so the expression holding it stays unresolved. Each follows Terraform's function
//! of the same name; a case it does not cover (a `replace` pattern, a `format` verb other than
//! `%s`, `%d`, `%v`, `%q` and `%%`) is an error, which leaves the call unresolved too.

use std::collections::HashSet;

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
        ("tolist", fixed(vec![list()], |a| Ok(a[0].clone()))),
        ("tomap", fixed(vec![map()], |a| Ok(a[0].clone()))),
        ("tostring", fixed(vec![Any], tostring)),
        ("tonumber", fixed(vec![Any], tonumber)),
        ("jsonencode", fixed(vec![Any], jsonencode)),
    ];
    for (name, def) in funcs {
        ctx.declare_func(name, def);
    }
}

fn string(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

fn text(args: FuncArgs, f: fn(&str) -> String) -> Out {
    Ok(Value::String(f(string(&args[0]))))
}

/// A scalar as a template shows it; `None` for a null, a list or an object.
fn shown(v: &Value) -> Option<String> {
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
    if s.is_empty() {
        return Ok(Value::Array(Vec::new()));
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

/// Terraform's sets of strings are ordered lexically; any other set keeps its first order.
/// The distinct elements of a list: strings sorted, as Terraform orders a set of strings, and
/// any other values in their first order, each kept by `canonical` once.
fn toset(args: FuncArgs) -> Out {
    let list = args[0].as_array().map_or(&[][..], Vec::as_slice);
    if list.iter().all(Value::is_string) {
        let mut items = list.to_vec();
        items.sort_unstable_by(|a, b| string(a).cmp(string(b)));
        items.dedup();
        return Ok(Value::Array(items));
    }
    let mut seen = HashSet::new();
    let items = list.iter().filter(|v| seen.insert(canonical(v)));
    Ok(Value::Array(items.cloned().collect()))
}

/// A text two values share exactly when they are equal: an object's whatever the order of its
/// keys, as `hcl-rs` compares them, and a number's in its exact digits.
fn canonical(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("{s:?}"),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", items.join(","))
        }
        Value::Object(map) => {
            let mut entries: Vec<String> = (map.iter())
                .map(|(k, v)| format!("{k:?}:{}", canonical(v)))
                .collect();
            entries.sort_unstable();
            format!("{{{}}}", entries.join(","))
        }
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
        Value::String(s) => (s.parse::<i64>().ok().map(Number::from))
            .or_else(|| s.parse::<f64>().ok().and_then(Number::from_f64))
            .map(Value::Number)
            .ok_or_else(|| format!("{s:?} is no number")),
        _ => Err("not a number".into()),
    }
}

/// JSON as Terraform's `jsonencode` writes it: compact, `<`, `>` and `&` escaped as Go does.
fn encoded(v: &Value) -> Result<String, String> {
    let json = serde_json::to_string(v).map_err(|e| e.to_string())?;
    Ok(json
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026"))
}

fn jsonencode(args: FuncArgs) -> Out {
    encoded(&args[0]).map(Value::String)
}
