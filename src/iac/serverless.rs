//! Serverless Framework service files as IaC nodes (#654, ADR 0025): each function of a
//! `serverless.yml`, `serverless.yaml` or `serverless.json` with a `service` is a node
//! `functions.<name>` of type `Serverless::Function`, with its handler and events recorded, and
//! the `resources` section (`resources.Resources`) is read as CloudFormation nodes
//! (`cloudformation::nodes`). The file is one evaluation unit, rooted at its path.
//!
//! A function's physical name is its `name`, else `<service>-<stage>-<function>`, the stage being
//! `provider.stage` or `dev`, overridable by `--stage` (`opt:stage`), which Serverless names such a
//! function after first. Variables are resolved over the file alone: `${self:a.b}` is the
//! value at that path (itself expanded), and `${opt:name, default}` is its default with the name
//! marked overridable, since a deployer passes `--name`. Alternatives (`${self:x, 'y'}`) are tried
//! in order. Every other source (`env`, `ssm`, `sls`, `aws`, `cf`, `file(...)`, ...) is an
//! unresolved part: a `*` in a name, `${source:...}` in a recorded text. A `${...}` with no source
//! (`${Queue.Arn}`) or a `::` (`${AWS::Region}`) is CloudFormation's and stays in the text.
//!
//! Each function references the resources its body names through `Ref`, `Fn::GetAtt`, `Fn::Sub`
//! (an event's `arn`, an environment entry), each `exact` with its `via`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{Map, Value, json};

use super::cloudformation::{self, PHYSICAL_NAME_MAX, scalar_text};
use super::compose;
use super::{NODES_MAX, Node, Overridable, REFERENCES_MAX, ROOT_STORED_MAX, Reference, Resolved};
use crate::config_values;
use crate::document::events::{self, Step, Syntax};

/// The type of a function's node.
const FUNCTION: &str = "Serverless::Function";

/// How deep a variable's value may name another (`${self:a}` whose value holds `${self:b}`); a
/// deeper one is unresolved.
// ponytail: a stage or a prefix defined once and reused goes two or three levels; 8 bounds a chain.
const VARIABLE_DEPTH_MAX: usize = 8;

/// How many `self:` values may be followed from one another before the stack is cut. A value
/// met there is not expanded, and each value within `VARIABLE_DEPTH_MAX` above it is kept only
/// for a reach at its own depth or deeper: met nearer the top, it is expanded again, so its answer
/// never depends on where it was met first.
// ponytail: a chain of real values is a few deep (`VARIABLE_DEPTH_MAX`); a built one hundreds of
// levels deep must not run the stack out, so the walk stops here, and a value is expanded at most
// once per depth it is met at (`STACK_MAX + 1` times).
const STACK_MAX: usize = 64;

/// How many bytes the expansions of one file may produce; past it, a variable is unresolved.
// ponytail: a service's variables give a few kilobytes; a file of references to one long value
// must not copy it without bound, so the budget is that of the file's stored rows.
const VARIABLE_BYTES_MAX: usize = ROOT_STORED_MAX;

/// How many events of a function are recorded; the rest are left out.
// ponytail: a function has a handful of events; 64 bounds one built to fill the index.
const EVENTS_MAX: usize = 64;

/// The file names of a service.
const NAMES: [&str; 3] = ["serverless.yml", "serverless.yaml", "serverless.json"];

/// Whether the file at `path` is named like a service file.
pub(crate) fn is_file(path: &str) -> bool {
    NAMES.contains(&path.rsplit('/').next().unwrap_or(path))
}

/// Whether the file `path` with bytes `src` is read for a service: named like one and saying
/// `service`.
pub(crate) fn wanted(path: &str, src: &[u8]) -> bool {
    is_file(path) && src.windows(7).any(|w| w == b"service")
}

/// Whether `doc` is a service: a mapping with a `service` and a `provider`, `functions` or
/// `resources`.
fn is_service(doc: &Value) -> bool {
    let Value::Object(map) = doc else {
        return false;
    };
    map.contains_key("service")
        && ["provider", "functions", "resources"]
            .iter()
            .any(|k| map.contains_key(*k))
}

/// A piece of an expanded text.
#[derive(Clone, Debug, PartialEq)]
enum Seg {
    Lit(String),
    /// An `opt:` variable with the default it falls to: the expression, the default's text and
    /// the line it is written on.
    Opt {
        expr: String,
        default: String,
        line: usize,
    },
    /// A variable no source of the file gives: the expression.
    Unknown(String),
}

/// Whether the inside of a `${...}` is a Serverless variable: `source:...` with one colon after a
/// source name or call (`self`, `opt`, `file(./x.yml)`), not CloudFormation's `AWS::Region`,
/// `!Literal` or `Resource.Attribute`.
fn is_variable(inner: &str) -> bool {
    let Some((head, rest)) = inner.split_once(':') else {
        return false;
    };
    !head.is_empty()
        && !rest.starts_with(':')
        && head
            .chars()
            .all(|c| c.is_alphanumeric() || "_-()./~".contains(c))
}

/// `inner` cut at the commas that are not inside quotes, parentheses or braces.
fn alternatives(inner: &str) -> Vec<&str> {
    let (mut out, mut start, mut depth, mut quote) = (Vec::new(), 0, 0i32, None::<char>);
    for (i, c) in inner.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '(' | '{') => depth += 1,
            (None, ')' | '}') => depth -= 1,
            (None, ',') if depth <= 0 => {
                out.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(inner[start..].trim());
    out
}

/// Merges neighbouring literals; a text longer than `PHYSICAL_NAME_MAX` is one unresolved part.
fn finish(segs: Vec<Seg>) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    let mut size = 0;
    for seg in segs {
        size += match &seg {
            Seg::Lit(s) | Seg::Unknown(s) => s.len(),
            Seg::Opt { expr, default, .. } => expr.len() + default.len(),
        };
        if size > PHYSICAL_NAME_MAX {
            return vec![Seg::Unknown("...".into())];
        }
        match (out.last_mut(), seg) {
            (Some(Seg::Lit(a)), Seg::Lit(b)) => a.push_str(&b),
            (_, seg) => out.push(seg),
        }
    }
    out
}

/// Gives each `opt:` default of `segs` that has no line yet `line`.
fn fill_lines(segs: &mut [Seg], line: usize) {
    for seg in segs {
        if let Seg::Opt { line: l, .. } = seg
            && *l == 0
        {
            *l = line;
        }
    }
}

/// `segs`, the value a variable falls to once an `opt:` may override it: literals only become one
/// `Opt` default of `expr`; literals and defaults stay as they are, with `expr` beside them as an
/// `Opt` of no text (overridable too); anything unresolved makes the whole unresolved.
fn overridable(segs: Vec<Seg>, expr: &str, line: usize) -> Vec<Seg> {
    match &segs[..] {
        [Seg::Lit(default)] => vec![Seg::Opt {
            expr: expr.to_string(),
            default: default.clone(),
            line,
        }],
        s if !s.is_empty() && s.iter().all(|x| !matches!(x, Seg::Unknown(_))) => {
            let mut segs = segs;
            segs.push(Seg::Opt {
                expr: expr.to_string(),
                default: String::new(),
                line,
            });
            segs
        }
        _ => vec![Seg::Unknown(expr.to_string())],
    }
}

/// The stage of a function's default name, `stage` as the file gives it: Serverless names such a
/// function after `--stage` first, then `provider.stage`, then `dev` (`setFunctionNames` of its
/// `Service`), whatever the stage holds, so the name is overridable by `opt:stage`, at `line`. A
/// stage that already goes through an `opt:stage`, or has an unresolved part, stays as it is.
// ponytail: a stage with an unresolved part (`${env:STAGE}`) makes the name a pattern, `*` where
// `--stage` goes too, so `opt:stage` is not listed beside it; list it if a reason needs it.
fn stage_of_default_name(stage: &[Seg], line: usize) -> Vec<Seg> {
    let staged = |s: &Seg| match s {
        Seg::Opt { expr, .. } => expr.split(',').next().map(str::trim) == Some("opt:stage"),
        Seg::Lit(_) => false,
        Seg::Unknown(_) => true,
    };
    if stage.iter().any(staged) {
        return stage.to_vec();
    }
    overridable(finish(stage.to_vec()), "opt:stage", line)
}

/// The bytes a piece counts for against `PHYSICAL_NAME_MAX` and `VARIABLE_BYTES_MAX`.
fn seg_len(seg: &Seg) -> usize {
    match seg {
        Seg::Lit(s) | Seg::Unknown(s) => s.len(),
        Seg::Opt { expr, default, .. } => expr.len() + default.len(),
    }
}

/// What one alternative of a variable gives.
enum Alt {
    /// A value, resolved or not.
    Value(Vec<Seg>),
    /// No value at that source: the next alternative is tried.
    Absent,
    /// An `opt:`, which a deployer may pass: the next alternative gives its default.
    Opt,
}

/// A memoized `self:` value.
struct Memo {
    segs: Vec<Seg>,
    /// 1 plus the height of the tallest `self:` value it uses.
    height: usize,
    /// The shallowest stack depth the entry holds at.
    from: usize,
}

/// The expression of the variables of a file, over its document.
struct Vars<'d> {
    doc: &'d Value,
    /// The line of each string scalar that holds a `${`, by key path.
    lines: &'d HashMap<String, usize>,
    /// What each `self:` path expanded to. A height is a property of the value, not of where it
    /// was reached: an entry holds at any depth (0) unless the expansion met the stack cut and
    /// stayed within `VARIABLE_DEPTH_MAX`, when it holds from the depth it was expanded at
    /// (there its height is only a lower bound).
    memo: HashMap<String, Memo>,
    /// The paths being expanded, with the stack depth each is expanded at.
    busy: HashMap<String, usize>,
    /// The height of the tallest `self:` value the current expansion used.
    reach: usize,
    /// Whether the current expansion used a value whose answer depends on the stack cut.
    cut: bool,
    /// The shallowest depth of a busy path the current expansion met again (`usize::MAX` for
    /// none): each value from there down is in a cycle.
    cycle: usize,
    /// How many `self:` values are being expanded inside one another.
    stack: usize,
    /// The bytes the file's expansions may still produce (`VARIABLE_BYTES_MAX`).
    room: usize,
}

impl Vars<'_> {
    fn new<'d>(doc: &'d Value, lines: &'d HashMap<String, usize>) -> Vars<'d> {
        Vars {
            doc,
            lines,
            memo: HashMap::new(),
            busy: HashMap::new(),
            reach: 0,
            cut: false,
            cycle: usize::MAX,
            stack: 0,
            room: VARIABLE_BYTES_MAX,
        }
    }

    /// The line of the string scalar at `at`, or of the nearest enclosing one (a tagged node's
    /// scalars sit at the path of the tag).
    fn line_at(&self, at: &[Step]) -> usize {
        (0..=at.len())
            .rev()
            .find_map(|n| self.lines.get(&config_values::key_path(&at[..n])).copied())
            .unwrap_or(1)
    }

    /// The value at the dotted path `path` of the document, with its steps.
    fn lookup(&self, path: &str) -> Option<(&Value, Vec<Step>)> {
        let (mut v, mut steps) = (self.doc, Vec::new());
        for key in path.split('.') {
            match v {
                Value::Object(m) => {
                    v = m.get(key)?;
                    steps.push(Step::Key(key.to_string()));
                }
                Value::Array(a) => {
                    let n: usize = key.parse().ok()?;
                    v = a.get(n)?;
                    steps.push(Step::Index(n));
                }
                _ => return None,
            }
        }
        (!v.is_null()).then_some((v, steps))
    }

    /// `text` with its Serverless variables expanded.
    fn expand(&mut self, text: &str, depth: usize) -> Vec<Seg> {
        let mut segs = Vec::new();
        let mut size = 0;
        let mut rest = text;
        while let Some(open) = rest.find("${") {
            segs.push(Seg::Lit(rest[..open].to_string()));
            let after = &rest[open + 2..];
            // The matching brace, counting the ones a nested variable opens.
            let mut level = 1usize;
            let close = after.char_indices().find(|&(_, c)| {
                match c {
                    '{' => level += 1,
                    '}' => level -= 1,
                    _ => {}
                }
                level == 0
            });
            let Some((end, _)) = close else {
                segs.push(Seg::Lit(rest[open..].to_string()));
                rest = "";
                break;
            };
            let inner = &after[..end];
            let from = segs.len() - 1;
            if !is_variable(inner) {
                segs.push(Seg::Lit(rest[open..open + 3 + end].to_string()));
            } else if depth >= VARIABLE_DEPTH_MAX || self.room == 0 {
                segs.push(Seg::Unknown(inner.to_string()));
            } else {
                let value = self.resolve(inner, depth);
                self.room = self.room.saturating_sub(value.iter().map(seg_len).sum());
                segs.extend(value);
            }
            size += segs[from..].iter().map(seg_len).sum::<usize>();
            if size > PHYSICAL_NAME_MAX {
                // `finish` would give the same: the rest is not expanded.
                return vec![Seg::Unknown("...".into())];
            }
            rest = &after[end + 1..];
        }
        segs.push(Seg::Lit(rest.to_string()));
        finish(segs)
    }

    /// The value of the variable `inner`: its alternatives tried in order. An alternative with a
    /// variable inside (`self:custom.${opt:stage}`) has it expanded first into one source,
    /// never cut again at a comma; a key an `opt:` default chose is overridable with that `opt:`.
    fn resolve(&mut self, inner: &str, depth: usize) -> Vec<Seg> {
        let mut opted = false;
        let mut keyed: Vec<(String, usize)> = Vec::new();
        let mut value: Option<Vec<Seg>> = None;
        for alt in alternatives(inner) {
            let source = if alt.contains("${") {
                let flat = self.expand(alt, depth + 1);
                let text = (flat.iter())
                    .map(|s| match s {
                        Seg::Lit(t) => Some(t.as_str()),
                        Seg::Opt { default, .. } => Some(default.as_str()),
                        Seg::Unknown(_) => None,
                    })
                    .collect::<Option<String>>();
                keyed.extend(flat.iter().filter_map(|s| match s {
                    Seg::Opt { expr, line, .. } => Some((expr.clone(), *line)),
                    _ => None,
                }));
                match text {
                    Some(text) => text,
                    None => {
                        value = Some(vec![Seg::Unknown(inner.to_string())]);
                        break;
                    }
                }
            } else {
                alt.to_string()
            };
            match self.alternative(source.trim(), inner) {
                Alt::Value(v) => {
                    value = Some(v);
                    break;
                }
                Alt::Absent => {}
                Alt::Opt => opted = true,
            }
        }
        let Some(mut value) = value else {
            return vec![Seg::Unknown(inner.to_string())];
        };
        // The override a deployer passes is each `opt:` inside, at its line, then the outer one.
        for (expr, line) in keyed {
            value = overridable(value, &expr, line);
        }
        if opted {
            value = overridable(value, inner, 0);
        }
        value
    }

    /// What the one source `alt` of the variable `inner` gives.
    fn alternative(&mut self, alt: &str, inner: &str) -> Alt {
        let unknown = || vec![Seg::Unknown(inner.to_string())];
        let quoted = alt.len() >= 2
            && ((alt.starts_with('\'') && alt.ends_with('\''))
                || (alt.starts_with('"') && alt.ends_with('"')));
        if quoted {
            Alt::Value(vec![Seg::Lit(alt[1..alt.len() - 1].to_string())])
        } else if let Some(path) = alt.strip_prefix("self:") {
            match self.value_of(path.trim()) {
                None => Alt::Absent,
                Some(found) => Alt::Value(found.unwrap_or_else(unknown)),
            }
        } else if alt.starts_with("opt:") {
            Alt::Opt
        } else if alt.contains(':') || alt.contains('(') {
            Alt::Value(unknown())
        } else {
            Alt::Value(vec![Seg::Lit(alt.to_string())])
        }
    }

    /// The expansion of the value at `path`: `None` when the document has none, `Some(None)` when
    /// it is no text (a mapping, a list) or names itself. `provider.stage` left out is `dev`, as
    /// Serverless reads it.
    fn value_of(&mut self, path: &str) -> Option<Option<Vec<Seg>>> {
        if let Some(m) = self.memo.get(path)
            && self.stack >= m.from
        {
            self.reach = self.reach.max(m.height);
            self.cut |= m.from > 0;
            return Some(Some(m.segs.clone()));
        }
        // Only a string is copied out of the document: a mapping is never cloned.
        let (text, steps) = {
            let Some((v, steps)) = self.lookup(path) else {
                return (path == "provider.stage").then(|| Some(vec![Seg::Lit("dev".into())]));
            };
            match v {
                Value::String(s) => (s.clone(), steps),
                other => return Some(scalar_text(other).map(|t| vec![Seg::Lit(t)])),
            }
        };
        if let Some(&at) = self.busy.get(path) {
            // A cycle: each value from `at` down is in it, and unresolved wherever it is met.
            self.cycle = self.cycle.min(at);
            return Some(None);
        }
        let depth = self.stack;
        if depth >= STACK_MAX {
            // Not expanded: its height is at least 1, and what uses it is no answer of its own
            // until it is taller than `VARIABLE_DEPTH_MAX` whatever this value holds.
            self.reach = self.reach.max(1);
            self.cut = true;
            return Some(Some(vec![Seg::Unknown(path.to_string())]));
        }
        self.busy.insert(path.to_string(), depth);
        let outer = std::mem::replace(&mut self.reach, 0);
        let outer_cut = std::mem::replace(&mut self.cut, false);
        let outer_cycle = std::mem::replace(&mut self.cycle, usize::MAX);
        self.stack += 1;
        let mut segs = self.expand(&text, 0);
        self.stack -= 1;
        self.busy.remove(path);
        let height = self.reach.saturating_add(1);
        self.reach = outer.max(height);
        let in_cycle = self.cycle <= depth;
        // The value above is in the cycle only when it closes higher than this one.
        self.cycle = outer_cycle.min(if self.cycle < depth {
            self.cycle
        } else {
            usize::MAX
        });
        // A lower bound past the limit is unresolved however far the cut value goes.
        let certain = in_cycle || !self.cut || height > VARIABLE_DEPTH_MAX;
        self.cut = outer_cut || !certain;
        if in_cycle || height > VARIABLE_DEPTH_MAX {
            segs = vec![Seg::Unknown(path.to_string())];
        }
        fill_lines(&mut segs, self.line_at(&steps));
        let from = if certain { 0 } else { depth };
        let memo = Memo {
            segs: segs.clone(),
            height,
            from,
        };
        self.memo.insert(path.to_string(), memo);
        Some(Some(segs))
    }

    /// `expand` for a string scalar at `at`: each `opt:` default gets the line of the scalar.
    fn string(&mut self, text: &str, at: &[Step]) -> Vec<Seg> {
        let line = self.line_at(at);
        let mut segs = self.expand(text, 0);
        fill_lines(&mut segs, line);
        segs
    }
}

/// A recorded text of `segs`: an `opt:` default in place, an unresolved variable as `${expr}`.
fn text_of(segs: &[Seg]) -> Resolved {
    let mut out = Resolved {
        text: String::new(),
        overridable: false,
    };
    for seg in segs {
        match seg {
            Seg::Lit(s) => out.text += s,
            Seg::Opt { default, .. } => {
                out.text += default;
                out.overridable = true;
            }
            Seg::Unknown(expr) => out.text += &format!("${{{expr}}}"),
        }
    }
    out
}

/// A physical name of `segs`: each unresolved variable a `*`, with whether one was and the
/// `opt:` defaults it went through.
fn name_of(segs: &[Seg], path: &str) -> (String, bool, BTreeSet<Overridable>) {
    let (mut text, mut pattern, mut through) = (String::new(), false, BTreeSet::new());
    for seg in segs {
        match seg {
            Seg::Lit(s) => text += s,
            Seg::Opt {
                expr,
                default,
                line,
            } => {
                text += default;
                through.insert(Overridable {
                    path: path.to_string(),
                    line: *line,
                    variable: expr.clone(),
                    tfvars: false,
                });
            }
            Seg::Unknown(_) => {
                text.push('*');
                pattern = true;
            }
        }
    }
    (text, pattern, through)
}

/// The resources section's synthetic template parameters: one per `opt:` default met.
#[derive(Default)]
struct Synthetic {
    /// The parameter name of each (expression, default, line).
    names: BTreeMap<(String, String, usize), String>,
    params: Map<String, Value>,
    lines: BTreeMap<String, usize>,
    /// The expression each parameter stands for.
    shown: BTreeMap<String, String>,
}

impl Synthetic {
    fn param(&mut self, expr: &str, default: &str, line: usize) -> String {
        let n = self.names.len();
        let name = self
            .names
            .entry((expr.to_string(), default.to_string(), line))
            .or_insert_with(|| format!("opt:{n}"))
            .clone();
        self.params
            .entry(name.clone())
            .or_insert_with(|| json!({"Type": "String", "Default": default}));
        self.lines.entry(name.clone()).or_insert(line);
        self.shown
            .insert(format!("Parameters.{name}"), expr.to_string());
        name
    }
}

/// `v` with its Serverless variables expanded for the CloudFormation reader: a string that
/// resolves wholly is that text; one with an `opt:` default becomes a `Ref` to a parameter that
/// holds it and one with an unresolved part an `Fn::ImportValue`, joined with the literal text
/// (or, inside an `Fn::Sub`, a `${...}` variable of the same two kinds). `sub` is whether `v` is
/// an `Fn::Sub`'s template.
fn substitute(
    vars: &mut Vars,
    synthetic: &mut Synthetic,
    v: &Value,
    at: &mut Vec<Step>,
    sub: bool,
) -> Value {
    match v {
        Value::String(s) if s.contains("${") => {
            let segs = vars.string(s, at);
            if let [Seg::Lit(text)] = &segs[..] {
                return Value::String(text.clone());
            }
            if sub {
                let mut text = String::new();
                for seg in &segs {
                    match seg {
                        Seg::Lit(s) => text += s,
                        Seg::Opt {
                            expr,
                            default,
                            line,
                        } => text += &format!("${{{}}}", synthetic.param(expr, default, *line)),
                        Seg::Unknown(_) => text += "${serverless:unresolved}",
                    }
                }
                return Value::String(text);
            }
            let items: Vec<Value> = (segs.iter())
                .map(|seg| match seg {
                    Seg::Lit(s) => Value::String(s.clone()),
                    Seg::Opt {
                        expr,
                        default,
                        line,
                    } => json!({"Ref": synthetic.param(expr, default, *line)}),
                    Seg::Unknown(expr) => json!({"Fn::ImportValue": expr}),
                })
                .collect();
            json!({"Fn::Join": ["", items]})
        }
        Value::Array(list) => Value::Array(
            (list.iter().enumerate())
                .map(|(i, item)| {
                    at.push(Step::Index(i));
                    let out = substitute(vars, synthetic, item, at, sub && i == 0);
                    at.pop();
                    out
                })
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            (map.iter())
                .map(|(k, item)| {
                    at.push(Step::Key(k.clone()));
                    let out = substitute(vars, synthetic, item, at, k == "Fn::Sub");
                    at.pop();
                    (k.clone(), out)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The text a value of an event takes: a string or number expanded, a `Fn::GetAtt` as
/// `Resource.Attribute`, a `Ref` as the name, anything else unresolved.
fn event_value(vars: &mut Vars, v: &Value, at: &[Step]) -> Option<Resolved> {
    match v {
        Value::String(s) => Some(text_of(&vars.string(s, at))),
        Value::Object(map) => match map.iter().collect::<Vec<_>>()[..] {
            [(f, arg)] if f == "Fn::GetAtt" => {
                let (resource, attribute) = cloudformation::get_att(arg)?;
                let text = match attribute {
                    Some(a) => format!("{resource}.{a}"),
                    None => resource.to_string(),
                };
                Some(Resolved {
                    text,
                    overridable: false,
                })
            }
            [(f, Value::String(name))] if f == "Ref" => Some(Resolved {
                text: name.clone(),
                overridable: false,
            }),
            _ => Some(Resolved {
                text: "*".into(),
                overridable: false,
            }),
        },
        // A list (the rates of one `schedule`): its items, each read the same way.
        Value::Array(items) => {
            let mut at = at.to_vec();
            let parts: Vec<Resolved> = (items.iter().enumerate())
                .filter_map(|(i, item)| {
                    at.push(Step::Index(i));
                    let part = event_value(vars, item, &at);
                    at.pop();
                    part
                })
                .collect();
            (!parts.is_empty()).then(|| Resolved {
                text: (parts.iter().map(|p| p.text.as_str()))
                    .collect::<Vec<_>>()
                    .join(" "),
                overridable: parts.iter().any(|p| p.overridable),
            })
        }
        other => scalar_text(other).map(|text| Resolved {
            text,
            overridable: false,
        }),
    }
}

/// An HTTP event's `method` in capitals and its `path` from the root, as written otherwise.
fn http_part(key: &str, mut part: Resolved) -> Resolved {
    if key == "method" && !part.text.contains("${") {
        part.text = part.text.to_uppercase();
    }
    if key == "path" && !part.text.starts_with(['/', '*', '$']) {
        part.text.insert(0, '/');
    }
    part
}

/// An event as recorded: its type, then what identifies it, `http GET /users`, `sqs Jobs.Arn`.
fn event_text(vars: &mut Vars, event: &Value, at: &mut Vec<Step>) -> Option<Resolved> {
    let (kind, body) = match event {
        Value::Object(map) => match map.iter().collect::<Vec<_>>()[..] {
            [(k, b)] => (k.clone(), b),
            _ => return None,
        },
        _ => return None,
    };
    at.push(Step::Key(kind.clone()));
    let mut parts: Vec<Resolved> = Vec::new();
    let http = kind == "http" || kind == "httpApi";
    match body {
        Value::String(s) if http && s.split_once(' ').is_some() => {
            // `GET /users`: the method and the path.
            let (method, path) = s.split_once(' ').unwrap_or_default();
            parts.push(http_part("method", text_of(&vars.string(method, at))));
            parts.push(http_part("path", text_of(&vars.string(path.trim(), at))));
        }
        Value::Object(map) if http => {
            for key in ["method", "path"] {
                if let Some(v) = map.get(key) {
                    at.push(Step::Key(key.into()));
                    parts.extend(event_value(vars, v, at).map(|p| http_part(key, p)));
                    at.pop();
                }
            }
        }
        Value::Object(map) => {
            let key = [
                "arn",
                "topicName",
                "bucket",
                "route",
                "rate",
                "cron",
                "name",
            ]
            .into_iter()
            .find(|k| map.contains_key(*k));
            if let Some(key) = key {
                at.push(Step::Key(key.into()));
                parts.extend(event_value(vars, &map[key], at));
                at.pop();
            }
        }
        scalar => parts.extend(event_value(vars, scalar, at)),
    }
    at.pop();
    let mut out = Resolved {
        text: kind.clone(),
        overridable: false,
    };
    for part in parts {
        out.text += &format!(" {}", part.text);
        out.overridable |= part.overridable;
    }
    Some(out)
}

/// The IaC nodes and references of the file `src` at `path`: none unless it is a service. The file
/// is its own root, labelled by `path`.
pub(super) fn evaluate(path: &str, src: &[u8]) -> (Vec<Node>, Vec<Reference>) {
    // The first and last line of the scalars under each function and each resource, and the line
    // of each string that holds a variable.
    let mut spans: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut resource_spans: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut lines: HashMap<String, usize> = HashMap::new();
    // The line of `provider.stage`, where a default name's `--stage` stands.
    let mut stage_line = None;
    let documents = events::read_keyed(Syntax::of(path), src, true, |scalar| {
        if scalar.document != 0 {
            return;
        }
        if matches!(scalar.path, [Step::Key(p), Step::Key(s)] if p == "provider" && s == "stage") {
            stage_line.get_or_insert(scalar.line);
        }
        if scalar.value.as_str().is_some_and(|s| s.contains("${")) {
            lines
                .entry(config_values::key_path(scalar.path))
                .or_insert(scalar.line);
        }
        let widen = |m: &mut BTreeMap<String, (usize, usize)>, id: &String| {
            let span = m.entry(id.clone()).or_insert((scalar.line, scalar.line));
            span.0 = span.0.min(scalar.line);
            span.1 = span.1.max(scalar.line);
        };
        match scalar.path {
            [Step::Key(top), Step::Key(id), ..] if top == "functions" => widen(&mut spans, id),
            [Step::Key(top), Step::Key(section), Step::Key(id), ..]
                if top == "resources" && section == "Resources" =>
            {
                widen(&mut resource_spans, id)
            }
            _ => {}
        }
    });
    let Ok((documents, key_lines)) = documents else {
        return (Vec::new(), Vec::new());
    };
    let Some(doc) = documents.first().filter(|d| is_service(d)) else {
        return (Vec::new(), Vec::new());
    };
    let mut vars = Vars::new(doc, &lines);
    let text = String::from_utf8_lossy(src);
    let file_lines: Vec<&str> = text.lines().collect();

    // The resources section as a template, its variables expanded.
    let mut synthetic = Synthetic::default();
    let declared = doc["resources"]["Resources"].as_object();
    let mut at = vec![Step::Key("resources".into()), Step::Key("Resources".into())];
    let resources = declared.map_or_else(Map::new, |declared| {
        (declared.iter())
            .take(NODES_MAX)
            .map(|(id, r)| {
                at.push(Step::Key(id.clone()));
                let out = substitute(&mut vars, &mut synthetic, r, &mut at, false);
                at.pop();
                (id.clone(), out)
            })
            .collect()
    });
    let known: BTreeSet<String> = (resources.iter())
        .filter(|(_, r)| (r.get("Type").and_then(Value::as_str)).is_some_and(|t| t.contains("::")))
        .map(|(id, _)| id.clone())
        .collect();

    // The functions.
    let service = match &doc["service"] {
        Value::Object(m) => m.get("name").cloned().unwrap_or(Value::Null),
        other => other.clone(),
    };
    let service_at = match &doc["service"] {
        Value::Object(_) => vec![Step::Key("service".into()), Step::Key("name".into())],
        _ => vec![Step::Key("service".into())],
    };
    let service_segs = match &service {
        Value::String(s) => vars.string(s, &service_at),
        other => scalar_text(other).map_or_else(
            || vec![Seg::Unknown("service".into())],
            |t| vec![Seg::Lit(t)],
        ),
    };
    let stage_at = vec![Step::Key("provider".into()), Step::Key("stage".into())];
    let stage_segs = match &doc["provider"]["stage"] {
        Value::String(s) => vars.string(s, &stage_at),
        Value::Null => vec![Seg::Lit("dev".into())],
        other => scalar_text(other).map_or_else(
            || vec![Seg::Unknown("provider.stage".into())],
            |t| vec![Seg::Lit(t)],
        ),
    };
    let key_line = compose::KeyLines::new(&file_lines, "functions", &key_lines);
    let (mut nodes, mut references) = (Vec::new(), Vec::new());
    let mut room = ROOT_STORED_MAX;
    let mut take = |bytes: usize| match room.checked_sub(bytes) {
        Some(r) => {
            room = r;
            true
        }
        None => false,
    };
    if let Some(Value::Object(functions)) = doc.get("functions") {
        let empty = Map::new();
        for (key, f) in functions.iter().take(NODES_MAX) {
            let body = f.as_object().unwrap_or(&empty);
            let mut at = vec![Step::Key("functions".into()), Step::Key(key.clone())];
            let (first, last) = spans.get(key).copied().unwrap_or((1, 1));
            let line = key_line.of(key, first);
            let name_segs = match body.get("name") {
                Some(Value::String(s)) => {
                    at.push(Step::Key("name".into()));
                    let segs = vars.string(s, &at);
                    at.pop();
                    Some(segs)
                }
                Some(_) => None,
                None => {
                    let mut all = service_segs.clone();
                    all.push(Seg::Lit("-".into()));
                    // `--stage` stands at `provider.stage`, or at the function with none.
                    all.extend(stage_of_default_name(
                        &stage_segs,
                        stage_line.unwrap_or(line),
                    ));
                    all.push(Seg::Lit(format!("-{key}")));
                    Some(finish(all))
                }
            };
            let (name, pattern, through) = match name_segs.as_deref().map(|s| name_of(s, path)) {
                Some((text, pattern, through)) => (Some(text), pattern, through),
                None => (None, false, BTreeSet::new()),
            };
            let handler = match body.get("handler") {
                Some(Value::String(s)) => {
                    at.push(Step::Key("handler".into()));
                    let segs = vars.string(s, &at);
                    at.pop();
                    Some(text_of(&segs))
                }
                _ => None,
            };
            let mut events = Vec::new();
            if let Some(Value::Array(list)) = body.get("events") {
                at.push(Step::Key("events".into()));
                for (i, e) in list.iter().take(EVENTS_MAX).enumerate() {
                    at.push(Step::Index(i));
                    events.extend(event_text(&mut vars, e, &mut at));
                    at.pop();
                }
                at.pop();
            }
            let size = key.len()
                + name.as_ref().map_or(0, String::len)
                + handler.as_ref().map_or(0, |h| h.text.len())
                + events.iter().map(|e| e.text.len()).sum::<usize>();
            if !take(size) {
                break;
            }
            let address = format!("functions.{key}");
            nodes.push(Node {
                address: address.clone(),
                root: path.to_string(),
                ty: FUNCTION.to_string(),
                name,
                pattern,
                overridable: through.into_iter().collect(),
                path: path.to_string(),
                line,
                end_line: key_line.end_of(line, last),
                block: config_values::key_path(&at[..2]),
                image: None,
                build: None,
                build_image: false,
                role: None,
                handler,
                events,
                subscription: None,
                environment: Vec::new(),
                images: Vec::new(),
                provisioned: None,
                ingress: Vec::new(),
            });
            let targets = cloudformation::referenced(&address, body, |t| known.contains(t));
            for (target, via) in targets.into_iter().take(REFERENCES_MAX) {
                if !take(address.len() + target.len() + via.len()) {
                    break;
                }
                references.push(Reference {
                    root: path.to_string(),
                    source: address.clone(),
                    target,
                    via: Some(via),
                });
            }
        }
    }

    // The resources, read as a template.
    let template = json!({"Parameters": synthetic.params, "Resources": resources});
    let resource_key_line = compose::KeyLines::new(&file_lines, "resources", &key_lines);
    let unit = cloudformation::Unit {
        path,
        doc: &template,
        spans: &resource_spans,
        defaults: &synthetic.lines,
        key_line: &resource_key_line,
        block: vec![Step::Key("resources".into()), Step::Key("Resources".into())],
        limit: NODES_MAX.saturating_sub(nodes.len()),
        budget: room,
    };
    let (resource_nodes, resource_references) = cloudformation::nodes(&unit);
    for mut node in resource_nodes {
        // A synthetic parameter shows as the expression it stands for.
        for o in &mut node.overridable {
            if let Some(expr) = synthetic.shown.get(&o.variable) {
                o.variable = expr.clone();
            }
        }
        node.overridable.sort();
        node.overridable.dedup();
        nodes.push(node);
    }
    references.extend(resource_references);
    // A function may name a resource the node ceiling or the byte room left out.
    let stored: HashSet<&str> = nodes.iter().map(|n| n.address.as_str()).collect();
    references.retain(|r| stored.contains(r.target.as_str()));
    (nodes, references)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(text: &str) -> (Vec<Node>, Vec<Reference>) {
        evaluate("serverless.yml", text.as_bytes())
    }

    /// The name and pattern flag of the function `f` of a service with `extra` before its
    /// functions.
    fn name(extra: &str, function: &str) -> (Option<String>, bool) {
        let text = format!(
            "service: svc\nprovider:\n  name: aws\n{extra}\nfunctions:\n  f:\n    handler: h.f\n{function}"
        );
        let (nodes, _) = unit(&text);
        let n = nodes.iter().find(|n| n.address == "functions.f").unwrap();
        (n.name.clone(), n.pattern)
    }

    fn known(text: &str) -> (Option<String>, bool) {
        (Some(text.into()), false)
    }

    fn pattern(text: &str) -> (Option<String>, bool) {
        (Some(text.into()), true)
    }

    fn named(value: &str) -> (Option<String>, bool) {
        name(
            "custom:\n  a: A\n  b: ${self:custom.a}-B\n  n: 7",
            &format!("    name: {value}\n"),
        )
    }

    #[test]
    fn a_service_file_is_named_like_one_and_has_a_service() {
        let ok = "service: s\nprovider:\n  name: aws\nfunctions:\n  f:\n    handler: h\n";
        assert_eq!(unit(ok).0.len(), 1);
        assert_eq!(unit(&ok.replace("service", "other")).0.len(), 0);
        assert_eq!(unit("service: s\n").0.len(), 0);
        assert_eq!(unit("- service\n").0.len(), 0);
        assert_eq!(unit("service: [\n").0.len(), 0);
        let json = r#"{"service": "s", "functions": {"f": {"handler": "h"}}}"#;
        assert_eq!(evaluate("a/serverless.json", json.as_bytes()).0.len(), 1);
        assert!(wanted("a/serverless.yaml", ok.as_bytes()));
        assert!(!wanted("a/other.yml", ok.as_bytes()));
        assert!(!wanted("a/serverless.yml", b"stage: x"));
        // The service may be a mapping with a name, and a function the file leaves open a node.
        let named = "service:\n  name: m\nfunctions:\n  f: ${file(f.yml)}\n";
        let (nodes, _) = unit(named);
        assert_eq!(nodes[0].name.as_deref(), Some("m-dev-f"));
        assert_eq!(nodes[0].handler, None);
    }

    #[test]
    fn a_function_is_named_by_service_stage_and_key_unless_it_sets_a_name() {
        assert_eq!(name("", ""), known("svc-dev-f"));
        assert_eq!(name("  stage: prod", ""), known("svc-prod-f"));
        assert_eq!(named("plain"), known("plain"));
        assert_eq!(named("[a]"), (None, false));
    }

    #[test]
    fn self_resolves_to_the_value_at_its_path_through_other_variables() {
        assert_eq!(named("${self:custom.a}"), known("A"));
        assert_eq!(named("${self:custom.b}-x"), known("A-B-x"));
        assert_eq!(named("${self:custom.n}"), known("7"));
        assert_eq!(named("${self:service}"), known("svc"));
        assert_eq!(named("${self:provider.name}"), known("aws"));
        // A path the file lacks falls to the next alternative, and with none is unresolved.
        assert_eq!(named("${self:custom.missing, 'z'}"), known("z"));
        assert_eq!(named("${self:custom.missing}"), pattern("*"));
        assert_eq!(named("${self:custom}"), pattern("*"));
        // A variable inside a variable is expanded first.
        assert_eq!(named("${self:custom.${self:custom.a, 'a'}}"), pattern("*"));
        let nested = name(
            "custom:\n  A: hit\n  k: A",
            "    name: ${self:custom.${self:custom.k}}\n",
        );
        assert_eq!(nested, known("hit"));
    }

    #[test]
    fn the_default_of_opt_resolves_and_is_overridable() {
        let (nodes, _) = unit(
            "service: s\nprovider:\n  stage: ${opt:stage, 'qa'}\nfunctions:\n  f:\n    handler: h\n",
        );
        let n = &nodes[0];
        assert_eq!((n.name.as_deref(), n.pattern), (Some("s-qa-f"), false));
        assert_eq!(n.overridable.len(), 1);
        let o = &n.overridable[0];
        assert_eq!(
            (o.variable.as_str(), o.line, o.tfvars),
            ("opt:stage, 'qa'", 3, false)
        );
        // With no default, an `opt:` alone is unresolved; a later alternative is its default.
        assert_eq!(named("${opt:stage}"), pattern("*"));
        assert_eq!(named("${opt:stage, self:custom.a}"), known("A"));
        assert_eq!(named("${opt:stage, dev}"), known("dev"));
        let (nodes, _) = unit("service: s\nfunctions:\n  f:\n    name: ${opt:n, 'x'}\n");
        assert_eq!(nodes[0].overridable.len(), 1);
        // A literal and a `self:` value are no override.
        let (nodes, _) = unit("service: s\nfunctions:\n  f:\n    name: ${self:service}\n");
        assert!(nodes[0].overridable.is_empty());
    }

    #[test]
    fn an_opt_over_an_overridable_fallback_or_choosing_a_key_stays_overridable() {
        let text = "service: s\nprovider:\n  stage: ${opt:stage, 'dev'}\ncustom:\n  stage: ${opt:stage, self:provider.stage}\n  tables:\n    dev: t-dev\n    prod: t-prod\nfunctions:\n  a:\n    name: ${self:custom.stage}\n  b:\n    name: ${self:custom.tables.${self:provider.stage}}\n  c:\n    name: ${self:custom.tables.${opt:stage, 'prod'}}\n  d:\n    name: ${self:custom.${opt:k, 'zzz'}}\n  e:\n    name: ${self:custom, 'fallback'}\n";
        let (nodes, _) = unit(text);
        let by = |a: &str| {
            let n = nodes.iter().find(|n| n.address == a).unwrap();
            (n.name.clone(), n.pattern, n.overridable.len())
        };
        // The outer `opt:` and the one in its fallback are both overrides of the name.
        assert_eq!(by("functions.a"), (Some("dev".into()), false, 2));
        assert_eq!(by("functions.b"), (Some("t-dev".into()), false, 1));
        assert_eq!(by("functions.c"), (Some("t-prod".into()), false, 1));
        assert_eq!(by("functions.d"), (Some("*".into()), true, 0));
        // A mapping is no text: unresolved, whatever follows.
        assert_eq!(by("functions.e"), (Some("*".into()), true, 0));
    }

    #[test]
    fn a_chain_deeper_than_the_limit_with_a_wide_fan_out_ends_in_linear_work() {
        let fan = vec!["${self:custom.NEXT}"; 20].join("-");
        let mut custom = String::from("custom:\n");
        for i in 0..12 {
            custom += &format!("  c{i}: {}\n", fan.replace("NEXT", &format!("c{}", i + 1)));
        }
        custom += "  c12: end\n";
        let start = std::time::Instant::now();
        let got = name(&custom, "    name: ${self:custom.c0}\n");
        // Every path past the depth limit is an unresolved part: a pattern of stars.
        assert!(
            got.1 && got.0.as_ref().is_some_and(|n| n.starts_with('*')),
            "{got:?}"
        );
        assert!(start.elapsed().as_secs() < 20);
    }

    #[test]
    fn a_hostile_chain_leaves_the_values_beside_it_resolved_in_any_order() {
        let fan = vec!["${self:custom.NEXT}"; 20].join("-");
        let mut custom = String::from("custom:\n  ok: fine\n");
        for i in 0..12 {
            custom += &format!("  c{i}: {}\n", fan.replace("NEXT", &format!("c{}", i + 1)));
        }
        custom += "  c12: end\n";
        for order in [["c0", "ok"], ["ok", "c0"]] {
            let functions: String = order
                .iter()
                .map(|v| format!("  f{v}:\n    name: ${{self:custom.{v}}}\n"))
                .collect();
            let text = format!("service: s\n{custom}functions:\n{functions}");
            let (nodes, _) = unit(&text);
            let name = |a: &str| nodes.iter().find(|n| n.address == a).unwrap().name.clone();
            assert_eq!(name("functions.fok").as_deref(), Some("fine"));
            assert_eq!(name("functions.fc0").as_deref(), Some("*"));
        }
        // A value that sits above a chain of its own height resolves wherever it is first met.
        let text = "service: s\ncustom:\n  a: ${self:custom.b}\n  b: ${self:custom.c}\n  c: x\nfunctions:\n  f1:\n    name: ${self:custom.b}\n  f2:\n    name: ${self:custom.a}\n";
        let (nodes, _) = unit(text);
        assert!(nodes.iter().all(|n| n.name.as_deref() == Some("x")));
    }

    #[test]
    fn a_value_under_a_chain_past_the_stack_cut_resolves_in_any_order() {
        let mut custom = String::from("custom:\n");
        for i in 0..64 {
            custom += &format!("  c{i}: ${{self:custom.c{}}}\n", i + 1);
        }
        custom += "  c64: end\n";
        let low =
            "  low57:\n    name: ${self:custom.c57}\n  low63:\n    name: ${self:custom.c63}\n";
        // Functions are evaluated in name order: `aa` before the others, `zz` after them.
        for top in ["aa", "zz"] {
            let functions = format!("  {top}:\n    name: ${{self:custom.c0}}\n{low}");
            let (nodes, _) = unit(&format!("service: s\n{custom}functions:\n{functions}"));
            let name = |a: &str| nodes.iter().find(|n| n.address == a).unwrap().name.clone();
            // c57 is 8 high and c63 2: both resolve; c0 is 65 high.
            assert_eq!(name("functions.low57").as_deref(), Some("end"));
            assert_eq!(name("functions.low63").as_deref(), Some("end"));
            assert_eq!(name(&format!("functions.{top}")).as_deref(), Some("*"));
        }
    }

    #[test]
    fn a_chain_past_the_stack_cut_with_a_wide_fan_out_ends_in_linear_work() {
        let fan = vec!["${self:custom.NEXT}"; 20].join("-");
        let mut custom = String::from("custom:\n");
        for i in 0..100 {
            custom += &format!("  c{i}: {}\n", fan.replace("NEXT", &format!("c{}", i + 1)));
        }
        custom += "  c100: end\n";
        let start = std::time::Instant::now();
        let functions =
            "  top:\n    name: ${self:custom.c0}\n  low:\n    name: ${self:custom.c99}\n";
        let (nodes, _) = unit(&format!("service: s\n{custom}functions:\n{functions}"));
        let name = |a: &str| nodes.iter().find(|n| n.address == a).unwrap().name.clone();
        assert!(name("functions.top").is_some_and(|n| n.starts_with('*')));
        assert_eq!(
            name("functions.low").as_deref(),
            Some(vec!["end"; 20].join("-").as_str())
        );
        assert!(start.elapsed().as_secs() < 20);
    }

    #[test]
    fn an_opt_over_a_fallback_with_an_opt_of_its_own_names_both() {
        let text = "service: s\ncustom:\n  p: ${opt:region, 'eu'}\nfunctions:\n  f:\n    name: ${opt:fname, self:custom.p}\n";
        let (nodes, _) = unit(text);
        let o: Vec<_> = (nodes[0].overridable.iter())
            .map(|o| (o.variable.as_str(), o.line))
            .collect();
        assert_eq!(nodes[0].name.as_deref(), Some("eu"));
        assert_eq!(
            o,
            [("opt:region, 'eu'", 3), ("opt:fname, self:custom.p", 6)]
        );
    }

    #[test]
    fn a_key_given_by_a_nested_variable_is_one_source_not_alternatives() {
        let text = "service: s\ncustom:\n  k: \"missing, 'evil'\"\nfunctions:\n  f:\n    name: ${self:custom.${self:custom.k}}\n";
        let (nodes, _) = unit(text);
        assert_eq!(nodes[0].name.as_deref(), Some("*"));
        assert!(nodes[0].pattern);
    }

    #[test]
    fn a_cycle_is_unresolved_whole_in_any_order() {
        let custom =
            "custom:\n  a: ${self:custom.b}\n  b: ${self:custom.a}-C\n  c: x-${self:custom.a}\n";
        for (first, second) in [("a", "b"), ("b", "a"), ("c", "a")] {
            let text = format!(
                "service: s\n{custom}functions:\n  f1:\n    name: ${{self:custom.{first}}}\n  f2:\n    name: ${{self:custom.{second}}}\n  f3:\n    name: ${{self:custom.c}}\n"
            );
            let (nodes, _) = unit(&text);
            let name = |a: &str| nodes.iter().find(|n| n.address == a).unwrap().name.clone();
            assert_eq!(
                name("functions.f1").as_deref(),
                Some(if first == "c" { "x-*" } else { "*" })
            );
            assert_eq!(name("functions.f2").as_deref(), Some("*"));
            assert_eq!(name("functions.f3").as_deref(), Some("x-*"));
        }
    }

    #[test]
    fn a_left_out_stage_is_dev_for_self_as_for_a_name() {
        let text = "service: s\ncustom:\n  t: ${self:service}-${self:provider.stage}-orders\nfunctions:\n  f:\n    handler: h\n  g:\n    name: ${self:custom.t}\n";
        let (nodes, _) = unit(text);
        let name = |a: &str| nodes.iter().find(|n| n.address == a).unwrap().name.clone();
        assert_eq!(name("functions.f").as_deref(), Some("s-dev-f"));
        assert_eq!(name("functions.g").as_deref(), Some("s-dev-orders"));
    }

    #[test]
    fn a_default_name_is_overridable_by_the_stage_option() {
        let listed = |n: &Node| -> Vec<(String, usize, bool)> {
            (n.overridable.iter())
                .map(|o| (o.variable.clone(), o.line, o.tfvars))
                .collect()
        };
        // A literal stage: `--stage` comes first in the name, at `provider.stage`'s line.
        let (nodes, _) = unit(
            "service: s\nprovider:\n  stage: prod\nfunctions:\n  f:\n    handler: h\n  g:\n    name: s-${self:provider.stage}-g\n",
        );
        let by = |a: &str| nodes.iter().find(|n| n.address == a).unwrap();
        let f = by("functions.f");
        assert_eq!((f.name.as_deref(), f.pattern), (Some("s-prod-f"), false));
        assert_eq!(listed(f), [("opt:stage".to_string(), 3, false)]);
        // `${self:provider.stage}` reads the file, never `--stage`: a name that writes it is none.
        let g = by("functions.g");
        assert_eq!((g.name.as_deref(), listed(g)), (Some("s-prod-g"), vec![]));
        // No stage in the file: `dev`, at the function's line.
        let (nodes, _) = unit("service: s\nfunctions:\n  f:\n    handler: h\n");
        assert_eq!(nodes[0].name.as_deref(), Some("s-dev-f"));
        assert_eq!(listed(&nodes[0]), [("opt:stage".to_string(), 3, false)]);
        // A stage another `opt:` gives is overridable by both; one `opt:stage` gives names that
        // one alone; an unresolved one has none.
        let (nodes, _) = unit(
            "service: s\nprovider:\n  stage: ${opt:env, 'qa'}\nfunctions:\n  f:\n    handler: h\n",
        );
        assert_eq!(nodes[0].name.as_deref(), Some("s-qa-f"));
        let both = [
            ("opt:env, 'qa'".to_string(), 3, false),
            ("opt:stage".to_string(), 3, false),
        ];
        assert_eq!(listed(&nodes[0]), both);
        let (nodes, _) = unit(
            "service: s\nprovider:\n  stage: ${opt:stage , 'qa'}\nfunctions:\n  f:\n    handler: h\n",
        );
        assert_eq!(
            listed(&nodes[0]),
            [("opt:stage , 'qa'".to_string(), 3, false)]
        );
        let (nodes, _) = unit(
            "service: s\nprovider:\n  stage: ${env:STAGE}\nfunctions:\n  f:\n    handler: h\n",
        );
        let n = &nodes[0];
        assert_eq!(
            (n.name.as_deref(), n.pattern, listed(n)),
            (Some("s-*-f"), true, vec![])
        );
    }

    #[test]
    fn expansions_past_the_file_budget_are_unresolved() {
        let doc = json!({"custom": {"big": "x".repeat(100)}});
        let lines = HashMap::new();
        let mut vars = Vars::new(&doc, &lines);
        vars.room = 250;
        let texts: Vec<Resolved> = (0..4)
            .map(|_| text_of(&vars.expand("${self:custom.big}", 0)))
            .collect();
        // 100 bytes each: the third leaves 50 short and the fourth meets no room.
        assert!(texts[..3].iter().all(|t| t.text == "x".repeat(100)));
        assert_eq!(texts[3].text, "${self:custom.big}");
        // A text grown past `PHYSICAL_NAME_MAX` stops expanding at once.
        let mut vars = Vars::new(&doc, &lines);
        let long = vec!["${self:custom.big}"; 1000].join("");
        assert_eq!(vars.expand(&long, 0), [Seg::Unknown("...".into())]);
        assert!(VARIABLE_BYTES_MAX - vars.room < 2 * PHYSICAL_NAME_MAX);
    }

    #[test]
    fn two_opts_that_pick_a_key_are_both_overridable() {
        let text = "service: s\ncustom:\n  devX: t\nfunctions:\n  f:\n    name: ${self:custom.${opt:a, 'dev'}${opt:b, 'X'}}\n";
        let (nodes, _) = unit(text);
        let o: Vec<_> = nodes[0]
            .overridable
            .iter()
            .map(|o| o.variable.as_str())
            .collect();
        assert_eq!(nodes[0].name.as_deref(), Some("t"));
        assert_eq!(o, ["opt:a, 'dev'", "opt:b, 'X'"]);
    }

    #[test]
    fn a_nested_variable_names_the_opt_inside_as_overridable() {
        let text = "service: s\nprovider:\n  stage: ${opt:stage, 'dev'}\ncustom:\n  devTable: t\nfunctions:\n  f:\n    name: ${self:custom.${self:provider.stage}Table}\n";
        let (nodes, _) = unit(text);
        let o = &nodes[0].overridable;
        assert_eq!(o.len(), 1);
        assert_eq!((o[0].variable.as_str(), o[0].line), ("opt:stage, 'dev'", 3));
    }

    #[test]
    fn a_value_reached_past_the_depth_limit_is_not_kept_for_a_shallow_reference() {
        let mut custom = String::from("custom:\n  shallow: ${self:custom.c7}\n");
        for i in 0..8 {
            custom += &format!("  c{i}: ${{self:custom.c{}}}\n", i + 1);
        }
        custom += "  c8: end\n";
        // `deep` reaches c7 at the limit and gets nothing; `shallow` reaches it at once.
        let text = format!(
            "service: s\n{custom}functions:\n  deep:\n    name: ${{self:custom.c0}}\n  near:\n    name: ${{self:custom.shallow}}\n"
        );
        let (nodes, _) = unit(&text);
        let name = |a: &str| nodes.iter().find(|n| n.address == a).unwrap().name.clone();
        assert_eq!(name("functions.deep").as_deref(), Some("*"));
        assert_eq!(name("functions.near").as_deref(), Some("end"));
    }

    #[test]
    fn env_ssm_and_the_other_sources_are_unresolved_parts() {
        assert_eq!(named("${env:X}"), pattern("*"));
        assert_eq!(named("pre-${env:X, 'd'}-post"), pattern("pre-*-post"));
        assert_eq!(named("${ssm:/a/b}-q"), pattern("*-q"));
        assert_eq!(named("${sls:stage}"), pattern("*"));
        assert_eq!(named("${aws:accountId}"), pattern("*"));
        assert_eq!(named("${cf:stack.out}"), pattern("*"));
        assert_eq!(named("${file(./x.yml):key}"), pattern("*"));
        assert_eq!(named("${opt:s, env:X, 'd'}"), pattern("*"));
        assert_eq!(named("${self:custom.a}${env:X}"), pattern("A*"));
        // CloudFormation's own `${...}` is no Serverless variable.
        assert_eq!(named("${AWS::Region}-q"), known("${AWS::Region}-q"));
        assert_eq!(named("${Queue.Arn}"), known("${Queue.Arn}"));
        assert_eq!(named("open-${unclosed"), known("open-${unclosed"));
    }

    #[test]
    fn variables_naming_themselves_or_nested_past_the_ceiling_are_unresolved() {
        let cycle = name(
            "custom:\n  a: ${self:custom.b}\n  b: ${self:custom.a}",
            "    name: ${self:custom.a}\n",
        );
        assert_eq!(cycle, pattern("*"));
        let mut chain = String::from("custom:\n");
        for i in 0..20 {
            chain += &format!("  c{i}: ${{self:custom.c{}}}\n", i + 1);
        }
        chain += "  c20: end\n";
        assert_eq!(name(&chain, "    name: ${self:custom.c0}\n"), pattern("*"));
        let mut shallow = String::from("custom:\n");
        for i in 0..3 {
            shallow += &format!("  c{i}: ${{self:custom.c{}}}\n", i + 1);
        }
        shallow += "  c3: end\n";
        assert_eq!(
            name(&shallow, "    name: ${self:custom.c0}\n"),
            known("end")
        );
    }

    #[test]
    fn a_name_grown_past_the_ceiling_is_unresolved() {
        let long = "x".repeat(PHYSICAL_NAME_MAX);
        assert_eq!(
            name(
                &format!("custom:\n  l: {long}"),
                "    name: ${self:custom.l}-y\n"
            ),
            pattern("*")
        );
        let fits = "x".repeat(PHYSICAL_NAME_MAX - 2);
        let (text, _) = name(
            &format!("custom:\n  l: {fits}"),
            "    name: ${self:custom.l}-y\n",
        );
        assert_eq!(text.map(|t| t.len()), Some(PHYSICAL_NAME_MAX - 2 + 2));
    }

    /// The events and handler of the function `f` of a service that holds `events`.
    fn recorded(events: &str) -> (Option<Resolved>, Vec<String>) {
        let text = format!(
            "service: s\nprovider:\n  stage: ${{opt:stage, 'dev'}}\nfunctions:\n  f:\n    handler: ${{env:H}}.run\n    events:\n{events}"
        );
        let (nodes, _) = unit(&text);
        let n = &nodes[0];
        (
            n.handler.clone(),
            n.events.iter().map(|e| e.text.clone()).collect(),
        )
    }

    #[test]
    fn a_handler_and_events_are_recorded_as_text() {
        let (handler, events) = recorded(
            "      - http:\n          path: users/{id}\n          method: get\n      - http: 'POST /users'\n      - httpApi: '*'\n      - httpApi:\n          path: /v2\n          method: any\n      - sqs: arn:aws:sqs:eu:1:q\n      - sns:\n          topicName: t-${opt:stage, 'dev'}\n      - schedule: rate(1 minute)\n      - schedule:\n          rate: cron(0 1 * * ? *)\n      - s3:\n          bucket: b\n      - websocket:\n          route: $connect\n      - eventBridge:\n          pattern: {}\n      - stream:\n          arn: !GetAtt Table.StreamArn\n      - sqs:\n          arn: !Ref Q\n      - sqs:\n          arn: !Join ['', [a, b]]\n      - {a: 1, b: 2}\n      - plain\n",
        );
        assert_eq!(
            handler,
            Some(Resolved {
                text: "${env:H}.run".into(),
                overridable: false
            })
        );
        assert_eq!(
            events,
            [
                "http GET /users/{id}",
                "http POST /users",
                "httpApi *",
                "httpApi ANY /v2",
                "sqs arn:aws:sqs:eu:1:q",
                "sns t-dev",
                "schedule rate(1 minute)",
                "schedule cron(0 1 * * ? *)",
                "s3 b",
                "websocket $connect",
                "eventBridge",
                "stream Table.StreamArn",
                "sqs Q",
                "sqs *",
            ]
        );
    }

    #[test]
    fn a_schedule_list_and_a_path_with_no_method_are_recorded() {
        let (_, events) = recorded(
            "      - schedule:\n          rate:\n            - rate(1 hour)\n            - cron(0 1 * * ? *)\n      - http:\n          path: orders\n",
        );
        assert_eq!(
            events,
            ["schedule rate(1 hour) cron(0 1 * * ? *)", "http /orders"]
        );
    }

    #[test]
    fn events_past_the_ceiling_are_left_out() {
        let items = "      - schedule: rate(1 minute)\n".repeat(EVENTS_MAX + 5);
        assert_eq!(recorded(&items).1.len(), EVENTS_MAX);
    }

    #[test]
    fn a_function_references_the_resources_its_body_names() {
        let text = "service: s\nfunctions:\n  f:\n    handler: h\n    environment:\n      Q: !Ref Queue\n      T: !Sub '${Table.Arn}'\n      X: !Ref Missing\n    events:\n      - sqs:\n          arn: !GetAtt Queue.Arn\nresources:\n  Resources:\n    Queue:\n      Type: AWS::SQS::Queue\n    Table:\n      Type: AWS::DynamoDB::Table\n    Plain:\n      Type: notatype\n";
        let (nodes, references) = unit(text);
        let via: Vec<_> = references
            .iter()
            .map(|r| {
                (
                    r.source.as_str(),
                    r.target.as_str(),
                    r.via.as_deref().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            via,
            [
                ("functions.f", "Queue", "Fn::GetAtt.Arn"),
                ("functions.f", "Queue", "Ref"),
                ("functions.f", "Table", "Fn::Sub"),
            ]
        );
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn the_resources_section_reads_variables_into_names_and_overridable_defaults() {
        let text = "service: s\nprovider:\n  stage: dev\ncustom:\n  p: ${self:service}-${opt:stage, 'dev'}\nresources:\n  Resources:\n    Q:\n      Type: AWS::SQS::Queue\n      Properties:\n        QueueName: ${self:custom.p}-q\n    S:\n      Type: AWS::SNS::Topic\n      Properties:\n        TopicName: !Sub '${self:custom.p}-${AWS::Region}-${env:X}'\n    L:\n      Type: AWS::Logs::LogGroup\n      Properties:\n        LogGroupName: ${ssm:/x}\n";
        let (nodes, _) = unit(text);
        let by = |a: &str| nodes.iter().find(|n| n.address == a).unwrap();
        let q = by("Q");
        assert_eq!((q.name.as_deref(), q.pattern), (Some("s-dev-q"), false));
        assert_eq!(q.overridable.len(), 1);
        assert_eq!(
            (q.overridable[0].variable.as_str(), q.overridable[0].line),
            ("opt:stage, 'dev'", 5)
        );
        let s = by("S");
        assert_eq!((s.name.as_deref(), s.pattern), (Some("s-dev-*-*"), true));
        assert_eq!(by("L").name.as_deref(), Some("*"));
        assert_eq!(q.block, "resources.Resources.Q");
        assert_eq!((q.line, q.end_line), (8, 11));
    }

    #[test]
    fn the_same_opt_on_two_lines_names_the_line_each_resource_goes_through() {
        let text = "service: s\nresources:\n  Resources:\n    A:\n      Type: AWS::SQS::Queue\n      Properties:\n        QueueName: ${opt:stage, 'dev'}-a\n    B:\n      Type: AWS::SQS::Queue\n      Properties:\n        QueueName: ${opt:stage, 'dev'}-b\n";
        let (nodes, _) = unit(text);
        let line = |a: &str| {
            let n = nodes.iter().find(|n| n.address == a).unwrap();
            (n.overridable.iter().map(|o| o.line)).collect::<Vec<_>>()
        };
        assert_eq!((line("A"), line("B")), (vec![7], vec![11]));
    }

    #[test]
    fn functions_and_resources_share_the_node_ceiling() {
        let mut text = String::from("service: s\nfunctions:\n");
        for i in 0..3 {
            text += &format!("  f{i}:\n    handler: h\n");
        }
        text += "resources:\n  Resources:\n";
        for i in 0..NODES_MAX {
            text += &format!("    R{i}:\n      Type: AWS::SQS::Queue\n");
        }
        // A function naming the last resource, which the ceiling leaves out.
        let last = format!("R{}", NODES_MAX - 1);
        let text = text.replace(
            "  f0:\n    handler: h\n",
            &format!("  f0:\n    handler: h\n    environment:\n      X: !Ref {last}\n"),
        );
        let (nodes, references) = unit(&text);
        assert_eq!(nodes.len(), NODES_MAX);
        assert_eq!(nodes.iter().filter(|n| n.ty == FUNCTION).count(), 3);
        assert!(references.is_empty(), "{references:?}");
    }
}
