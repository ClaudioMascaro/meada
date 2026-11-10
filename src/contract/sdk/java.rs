//! Java request builders and imported client factories, without accepting declared types.
use crate::lang::{Invocation, Value};
use crate::resolve::Code;
use std::ops::Range;

pub(super) fn constructor(name: &str, steps: &[&str], client: &str) -> bool {
    name.strip_suffix(".create")
        .is_some_and(|n| client_type(n, client))
        && steps.is_empty()
        || name
            .strip_suffix(".builder")
            .is_some_and(|n| client_type(n, client))
            && steps.last() == Some(&"build")
            && steps.iter().all(|s| {
                matches!(
                    *s,
                    "build"
                        | "region"
                        | "credentialsProvider"
                        | "endpointOverride"
                        | "httpClient"
                        | "httpClientBuilder"
                        | "overrideConfiguration"
                        | "defaultsMode"
                        | "dualstackEnabled"
                        | "fipsEnabled"
                )
            })
}

pub(super) fn made<'a>(code: &Code<'a>, path: &'a str, index: usize, client: &str) -> bool {
    let Some(chain) = code.chain(path, index) else {
        return false;
    };
    let Some(&start) = chain.last() else {
        return false;
    };
    let Some(call) = code.call(path, start) else {
        return false;
    };
    let steps: Vec<_> = chain
        .iter()
        .rev()
        .skip(1)
        .map(|&i| crate::resolve::method(&code.files()[path].calls[i].callee))
        .collect();
    crate::names::known(code, path, &call.scope, &call.callee, |s| {
        constructor(s, &steps, client)
    })
    .is_some()
}

/// The local a builder chain starts from (`b` in `b.message(..).build()`).
pub(crate) fn root(value: &Value) -> Option<&str> {
    let Value::Call(call) = value else {
        return None;
    };
    let mut call = call;
    loop {
        match call.receiver.as_deref()? {
            Value::Call(next) => call = next,
            Value::Ref(local) => return Some(local),
            _ => return None,
        }
    }
}

/// What a Java file does with the builders it names, read once for the whole run. A builder
/// changes in place, so a name is `exact` only when every mention of its builder is a form the
/// reader follows: a setter called on it (a member of the name's path changes it, any other does
/// not), its `.build()`, an Object method. Every other mention is a change the reader cannot
/// follow, and the side is unresolved: the builder handed to a call or captured, the receiver of
/// a method that runs caller code (`applyMutation`), an initializer or an assignment that makes
/// another name of it (at any depth), and a write of its name the syntax model does not show.
#[derive(Default)]
pub(crate) struct Builders<'a> {
    files: std::cell::RefCell<std::collections::HashMap<&'a str, std::rc::Rc<Uses<'a>>>>,
}

// The work of the linear-cost test: every call, mention and followed value, once.
#[cfg(test)]
thread_local! {
    pub(super) static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn work() {
    #[cfg(test)]
    WORK.with(|w| w.set(w.get() + 1));
}

/// The source positions of the mentions of one kind: where the first ends, and where each starts
/// (in order, once the file is read).
struct Span {
    first_end: usize,
    starts: Vec<usize>,
}

impl Span {
    fn of(range: &Range<usize>) -> Self {
        Self {
            first_end: range.end,
            starts: vec![range.start],
        }
    }

    fn add(&mut self, range: &Range<usize>) {
        self.first_end = self.first_end.min(range.end);
        self.starts.push(range.start);
    }

    /// Whether a mention lies before `at.range`, or starts after it and before `at.later`.
    fn hits(&self, at: &At) -> bool {
        self.first_end <= at.range.start || {
            let next = self.starts.partition_point(|&s| s < at.range.end);
            self.starts.get(next).is_some_and(|&s| s < at.later)
        }
    }
}

/// Where a value is made, and how far past that a mention still changes it.
struct At {
    range: Range<usize>,
    /// A mention starting before this, and after `range`, changes the value: the whole file for a
    /// request the call itself makes, `range.end` (none) for a value made once.
    later: usize,
    /// The outermost loop the value is made in: a builder that outlives it is changed by every
    /// mention in it, for the next turn builds from what this one set.
    looped: Option<Range<usize>>,
}

#[derive(Default)]
struct Mentions<'a> {
    /// How many times the name is written as the start of a call (`b.set(..)`, `b.build()`).
    direct: u32,
    /// A mention the reader cannot follow.
    other: Option<Span>,
    /// The setters called on it, by method.
    sets: Vec<(&'a str, Span)>,
}

impl<'a> Mentions<'a> {
    fn add(&mut self, kind: Kind<'a>, range: &Range<usize>, direct: bool) {
        self.direct += u32::from(direct);
        let span = match kind {
            Kind::Harmless => return,
            Kind::Other => &mut self.other,
            Kind::Set(method) => {
                match self.sets.iter_mut().find(|(m, _)| *m == method) {
                    Some((_, span)) => span.add(range),
                    None => self.sets.push((method, Span::of(range))),
                }
                return;
            }
        };
        match span {
            Some(span) => span.add(range),
            None => *span = Some(Span::of(range)),
        }
    }

    fn changed(&self, setter: &dyn Fn(&str) -> bool, at: &At) -> bool {
        self.other.as_ref().is_some_and(|s| s.hits(at))
            || (self.sets.iter()).any(|(method, span)| setter(method) && span.hits(at))
    }

    fn sorted(&mut self) {
        let spans = self
            .other
            .iter_mut()
            .chain(self.sets.iter_mut().map(|(_, s)| s));
        spans.for_each(|s| s.starts.sort_unstable());
    }
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    /// `build()` makes a value that cannot change, and `Object`'s methods return a value.
    Harmless,
    /// A step on the builder that sets a member of its own name.
    Set(&'a str),
    Other,
}

/// The mentions of each builder name in one file: a local by its declaring scope, and every name
/// by itself for a field, which any method sets.
#[derive(Default)]
struct Uses<'a> {
    locals: std::collections::HashMap<(&'a str, &'a str), Mentions<'a>>,
    fields: std::collections::HashMap<&'a str, Mentions<'a>>,
    /// The outermost loops (and lambdas) of the file, in order.
    loops: &'a [Range<usize>],
    /// How many times the file writes the name of each local and `final` field (`Facts::mentioned`).
    written: std::collections::HashMap<(&'a str, &'a str), u32>,
}

/// One mention of the name of a builder, made in `scope`.
struct Mention<'a, 'r> {
    scope: &'a str,
    name: &'a str,
    kind: Kind<'a>,
    range: &'r Range<usize>,
    /// Written once as the start of a call, not the root of a longer chain.
    direct: bool,
}

impl<'a> Uses<'a> {
    fn add(&mut self, code: &Code<'a>, path: &'a str, mention: Mention<'a, '_>) {
        work();
        let name = mention.name.strip_prefix("this.").unwrap_or(mention.name);
        let declared =
            (code.local(path, mention.scope, name)).map_or(mention.scope, |b| b.scope.as_str());
        let (kind, range, direct) = (mention.kind, mention.range, mention.direct);
        self.locals
            .entry((declared, name))
            .or_default()
            .add(kind, range, direct);
        self.fields
            .entry(name)
            .or_default()
            .add(kind, range, direct);
    }

    fn read(code: &Code<'a>, path: &'a str) -> Self {
        let facts = code.files()[path];
        let mut uses = Self {
            locals: Default::default(),
            fields: Default::default(),
            loops: &facts.loops,
            written: (code.mentioned(path).iter())
                .map(|m| ((m.scope.as_str(), m.name.as_str()), m.count))
                .collect(),
        };
        for index in 0..facts.calls.len() {
            let Some(call) = code.call(path, index) else {
                continue;
            };
            let scope = call.scope.as_str();
            if let Some(root) = steps_root(&call.callee, call.receiver.as_ref()) {
                let method = crate::resolve::method(&call.callee);
                let kind = match method {
                    "build" | "toString" | "hashCode" | "equals" | "getClass" => Kind::Harmless,
                    "applyMutation" => Kind::Other,
                    "copy" if !call.args.is_empty() => Kind::Other,
                    method => Kind::Set(method),
                };
                // The name this call is made on, written once: not the root of a longer chain.
                let direct = match &call.receiver {
                    Some(Value::Ref(_)) => true,
                    Some(_) => false,
                    None => call.callee.contains('.'),
                };
                let range = &call.range;
                let mention = Mention {
                    scope,
                    name: root,
                    kind,
                    range,
                    direct,
                };
                uses.add(code, path, mention);
            }
            for arg in &call.args {
                handed(arg, &mut |name| {
                    let range = &call.range;
                    let kind = Kind::Other;
                    uses.add(
                        code,
                        path,
                        Mention {
                            scope,
                            name,
                            kind,
                            range,
                            direct: false,
                        },
                    )
                });
            }
        }
        // A name made from a builder, or assigned one, is another name of it: first in the file.
        for bound in code.bindings(path).iter().chain(code.assigned(path)) {
            handed(&bound.value, &mut |name| {
                let scope = bound.scope.as_str();
                let (kind, range) = (Kind::Other, &(0..0));
                uses.add(
                    code,
                    path,
                    Mention {
                        scope,
                        name,
                        kind,
                        range,
                        direct: false,
                    },
                )
            });
        }
        (uses.locals.values_mut()).for_each(Mentions::sorted);
        (uses.fields.values_mut()).for_each(Mentions::sorted);
        uses
    }

    /// Whether the builder `name`, a local of `scope` or a field, was changed, as seen from `at`.
    fn changed(
        &self,
        scope: &str,
        name: &str,
        field: bool,
        setter: &dyn Fn(&str) -> bool,
        at: &At,
    ) -> bool {
        // Any method sets a field, whatever the order of the file.
        let anywhere = At {
            range: at.range.clone(),
            later: usize::MAX,
            looped: None,
        };
        self.locals
            .get(&(scope, name))
            .is_some_and(|m| m.changed(setter, at))
            || field && (self.fields.get(name)).is_some_and(|m| m.changed(setter, &anywhere))
    }

    /// Whether the file writes the builder `name` more often than the calls made on it account
    /// for (its declaration and each call started from it): a use the syntax model does not
    /// show, which could be any change. A name with no count is not known to be seen whole.
    fn unseen(&self, scope: &str, name: &str, field: bool) -> bool {
        let (scope, mentions) = match field {
            true => ("", self.fields.get(name)),
            false => (scope, self.locals.get(&(scope, name))),
        };
        let direct = mentions.map_or(0, |m| m.direct);
        self.written
            .get(&(scope, name))
            .is_none_or(|&written| written > 1 + direct)
    }

    /// Where a value built by the call at `range` is made: once, unless a loop holds the call.
    fn made(&self, range: &Range<usize>) -> At {
        let next = self.loops.partition_point(|l| l.end <= range.start);
        let looped = (self.loops.get(next))
            .filter(|l| l.start <= range.start && range.end <= l.end)
            .cloned();
        At {
            range: range.clone(),
            later: range.end,
            looped,
        }
    }
}

/// The name a chain of steps is made on: `b` in `b.message(..).build()` as the stored calls spell
/// it (`b` the receiver of the innermost step, or the head of its callee).
fn steps_root<'v>(callee: &'v str, receiver: Option<&'v Value>) -> Option<&'v str> {
    let (mut callee, mut receiver) = (callee, receiver);
    while let Some(Value::Call(next)) = receiver {
        (callee, receiver) = (&next.callee, next.receiver.as_deref());
    }
    match receiver {
        Some(Value::Ref(name)) => Some(name),
        Some(_) => None,
        None => callee.rsplit_once('.').map(|(head, _)| head),
    }
}

/// The names of the builders a value hands on: itself (`f(b)`), a chain off it that still
/// returns the builder (`f(b.message(..))`), or either inside a constructor (`new T(b)`).
fn handed<'v>(value: &'v Value, to: &mut dyn FnMut(&'v str)) {
    match value {
        Value::Ref(name) => to(name),
        Value::Call(c) => {
            if let Some(root) = steps_root(&c.callee, c.receiver.as_deref())
                && !matches!(
                    crate::resolve::method(&c.callee),
                    "build" | "toString" | "hashCode" | "equals" | "getClass"
                )
            {
                to(root);
            }
        }
        Value::New(_, args) => args.iter().for_each(|a| handed(a, to)),
        _ => {}
    }
}

/// Whether the request `value`, made by the call at `range`, reads a builder changed by a mention
/// the reader cannot follow, or by a setter of a member `setter` accepts, outside `range`. A
/// named value (`Put p = pb.build()`) is what its declaration built, so only what happened to its
/// builder before that `.build()` counts: a value built first is an immutable copy.
pub(super) fn changed<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: &'a Value,
    range: &Range<usize>,
    setter: &dyn Fn(&str) -> bool,
    builders: &Builders<'a>,
) -> bool {
    let uses = (builders.files.borrow_mut().entry(path))
        .or_insert_with(|| std::rc::Rc::new(Uses::read(code, path)))
        .clone();
    let mut walk = Walk {
        code,
        path,
        scope,
        setter,
        uses: &uses,
        seen: std::collections::HashSet::new(),
        depth: 0,
    };
    let at = At {
        range: range.clone(),
        later: usize::MAX,
        looped: None,
    };
    walk.value(value, &at)
}

/// The named values and builders one `changed` question has followed.
struct Walk<'a, 'c> {
    code: &'c Code<'a>,
    path: &'a str,
    scope: &'a str,
    setter: &'c dyn Fn(&str) -> bool,
    uses: &'c Uses<'a>,
    seen: std::collections::HashSet<*const crate::lang::Bound>,
    /// How many named values and builders deep the question now is.
    depth: usize,
}

impl<'a> Walk<'a, '_> {
    fn value(&mut self, value: &'a Value, at: &At) -> bool {
        work();
        match value {
            Value::Ref(name) => self.named(name, at),
            Value::Call(top) => {
                root(value).is_some_and(|b| self.builder(b, at))
                    || std::iter::successors(Some(top), |c| match c.receiver.as_deref() {
                        Some(Value::Call(next)) => Some(next),
                        _ => None,
                    })
                    .flat_map(|c| &c.args)
                    .any(|a| self.value(a, at))
            }
            _ => false,
        }
    }

    /// Whether the built value in the local `name` was made from a changed builder. Each local is
    /// judged once, and past `EVAL_DEPTH` nested ones the name stays unresolved.
    fn named(&mut self, name: &'a str, at: &At) -> bool {
        let Some(bound) = self.code.local(self.path, self.scope, name) else {
            return false;
        };
        if !self.seen.insert(bound) {
            return false;
        }
        self.within(|walk| match &bound.value {
            Value::Call(made) => {
                let at = walk.uses.made(&made.range);
                walk.value(&bound.value, &at)
            }
            alias @ Value::Ref(_) => walk.value(alias, at),
            _ => false,
        })
    }

    /// `f` one value deeper, or `true` (a change the reader cannot follow) past `EVAL_DEPTH`.
    fn within(&mut self, f: impl FnOnce(&mut Self) -> bool) -> bool {
        if self.depth >= crate::resolve::EVAL_DEPTH {
            return true;
        }
        self.depth += 1;
        let changed = f(self);
        self.depth -= 1;
        changed
    }

    /// Whether the builder `name` was changed as seen from `at`, or is another name of one, or was
    /// made from a changed value.
    fn builder(&mut self, name: &'a str, at: &At) -> bool {
        let name = name.strip_prefix("this.").unwrap_or(name);
        let local = self.code.local(self.path, self.scope, name);
        let field = local.is_none()
            && self
                .code
                .constant_value(self.path, self.scope, name)
                .is_some();
        // A type, a parameter or a name the file binds more than once holds no builder to follow.
        if local.is_none() && !field {
            return false;
        }
        let scope = local.map_or(self.scope, |b| b.scope.as_str());
        let declared = match local.map(|b| &b.value) {
            Some(Value::Call(c)) => Some(c.range.start),
            _ => None,
        };
        // A builder declared before the loop its value is made in, set later in that loop, is
        // set before the next turn builds.
        let at = match &at.looped {
            Some(l) if declared.is_none_or(|d| d < l.start) => At {
                range: at.range.clone(),
                later: at.later.max(l.end),
                looped: None,
            },
            _ => At {
                range: at.range.clone(),
                later: at.later,
                looped: None,
            },
        };
        if self.uses.unseen(scope, name, field)
            || self.uses.changed(scope, name, field, self.setter, &at)
        {
            return true;
        }
        match local.map(|b| &b.value) {
            // Another name of a builder is a mention of it, at any depth.
            Some(Value::Ref(_)) => true,
            Some(made @ Value::Call(c)) => {
                root(made).is_some_and(|r| self.code.local(self.path, self.scope, r).is_some())
                    || self.within(|walk| {
                        let at = walk.uses.made(&c.range);
                        walk.value(made, &at)
                    })
            }
            _ => false,
        }
    }
}

/// Whether the builder `local`, read in `scope` of `path`, is set again outside `range` by a
/// step `setter` accepts, or handed to a call there: a Java builder changes in place, so the
/// chain a call reads off it is not what it was given. A step counts anywhere in a chain off the
/// builder (`b.message(..).topicArn(..)`), or off a local holding it or such a chain (a step
/// returns its builder). A local counts in its scope, a `final` field (a constant) anywhere in
/// its file.
pub(crate) fn rebuilt<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    local: &'a str,
    range: &std::ops::Range<usize>,
    setter: &dyn Fn(&str) -> bool,
) -> bool {
    let within = if code.local(path, scope, local).is_some() {
        Some(scope)
    } else if code.constant_value(path, scope, local).is_some() {
        None
    } else {
        return false;
    };
    let named = |name: &str| {
        name == local
            || name.strip_prefix("this.") == Some(local)
            || within
                .and_then(|s| code.local(path, s, name))
                .is_some_and(|b| {
                    matches!(&b.value, Value::Ref(n) if n == local)
                        || unbuilt(&b.value) && root(&b.value) == Some(local)
                })
    };
    // A step that still returns the builder: `build()` makes the request, which cannot change,
    // and `Object`'s methods return a value of their own.
    fn unbuilt(value: &Value) -> bool {
        matches!(value, Value::Call(c) if !matches!(
            crate::resolve::method(&c.callee),
            "build" | "toString" | "hashCode" | "equals" | "getClass"
        ))
    }
    let holds = |value: &Value| match value {
        Value::Ref(name) => named(name),
        value => unbuilt(value) && root(value).is_some_and(named),
    };
    code.files()[path].calls.iter().enumerate().any(|(i, c)| {
        within.is_none_or(|s| c.scope == s)
            && !(range.start <= c.range.start && c.range.end <= range.end)
            && code.call(path, i).is_some_and(|call| {
                let steps = std::iter::successors(
                    Some((call.callee.as_str(), call.receiver.as_ref())),
                    |(_, receiver)| match receiver {
                        Some(Value::Call(next)) => {
                            Some((next.callee.as_str(), next.receiver.as_deref()))
                        }
                        _ => None,
                    },
                );
                let (mut set, mut on) = (false, false);
                for (callee, receiver) in steps {
                    set |= setter(crate::resolve::method(callee));
                    on = match receiver {
                        Some(Value::Call(_)) => continue,
                        Some(value) => holds(value),
                        None => callee.rsplit_once('.').is_some_and(|(head, _)| named(head)),
                    };
                }
                set && on || call.args.iter().any(holds)
            })
    })
}

pub(super) fn names<'v>(call: &'v Invocation, field: &str) -> Option<Vec<Option<&'v Value>>> {
    let Some(request) = call.args.first() else {
        return Some(vec![None]);
    };
    field_of(request, field)
}

pub(super) fn field_of<'v>(value: &'v Value, field: &str) -> Option<Vec<Option<&'v Value>>> {
    if matches!(value, Value::Ref(_)) {
        return None;
    }
    let Value::Call(top) = value else {
        return Some(vec![None]);
    };
    let key = format!("{}{}", field[..1].to_ascii_lowercase(), &field[1..]);
    let steps = std::iter::successors(Some(top), |s| match s.receiver.as_deref() {
        Some(Value::Call(next)) => Some(next),
        _ => None,
    });
    for step in steps {
        let method = crate::resolve::method(&step.callee);
        if method == key {
            return Some(step.args.iter().map(Some).collect());
        }
        // A step that runs the caller's code builds a value the reader does not see.
        if method == "applyMutation" || method == "copy" && !step.args.is_empty() {
            static UNKNOWN: Value = Value::Unknown;
            return Some(vec![Some(&UNKNOWN)]);
        }
    }
    Some(vec![None])
}

fn of(call: &Invocation, classes: &[&str], method: &str) -> bool {
    let Some(Value::External(names)) = call.receiver.as_deref() else {
        return false;
    };
    if crate::resolve::method(&call.callee) != method {
        return false;
    }
    crate::names::one(names, |s| classes.contains(&s)).is_some()
        // A full package spelling whose head is unbound has only on-demand candidates in
        // the folded value. As names::known does, accept its written standard-library name.
        // A local, type or explicit import hiding `java` has no such implicit candidate.
        || classes.iter().any(|class| call.callee == format!("{class}.{method}")
            && names.contains(&format!("java.lang.{class}")))
}

pub(super) fn sequence(call: &Invocation) -> bool {
    of(call, &["java.util.List", "java.util.Set"], "of")
        || of(call, &["java.util.Arrays"], "asList")
}

pub(super) fn map(call: &Invocation) -> bool {
    of(call, &["java.util.Map"], "of")
}

fn client_type(name: &str, client: &str) -> bool {
    name == client
        || name
            .strip_suffix("AsyncClient")
            .is_some_and(|n| Some(n) == client.strip_suffix("Client"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Role;
    use crate::contract::sdk::{Client, Operation, Service, extract};
    use crate::lang::LanguagePack;
    use crate::lang::java::Java;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    const SNS: Service = Service {
        client: Client {
            java: "software.amazon.awssdk.services.sns.SnsClient",
            go: "",
            csharp: "",
        },
        declared: &[],
        kind: "sns",
        operations: &[Operation {
            method: "publish",
            field: "TopicArn",
            role: Role::Provider,
            hierarchy: false,
        }],
    };

    /// The work spent reading `n` named builds in one method, every second one set again, and the
    /// sides that came out.
    fn read(n: usize) -> (usize, Vec<String>) {
        let mut body = String::new();
        for i in 0..n {
            body += &format!(
                "var b{i} = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:1:t{i}\");\n"
            );
            if i % 2 == 1 {
                body += &format!("b{i}.topicArn(\"arn:aws:sns:us-east-1:1:o{i}\");\n");
            }
            body += &format!("PublishRequest r{i} = b{i}.build();\nsns.publish(r{i});\n");
        }
        let src = format!(
            "import software.amazon.awssdk.services.sns.SnsClient;\n\
             import software.amazon.awssdk.services.sns.model.PublishRequest;\n\
             class C {{ void f() {{ var sns = SnsClient.create();\n{body} }} }}\n"
        );
        let facts = Java.parse("C.java", src.as_bytes()).unwrap();
        let files: BTreeMap<&str, &_> = [("C.java", &facts)].into_iter().collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let code = Code::new(&resolver, &files, &Java);
        WORK.with(|w| w.set(0));
        let found = extract(&code, "channel", &[SNS], None);
        let work = WORK.with(|w| w.get());
        let mut sides: Vec<_> = found.into_iter().filter_map(|f| f.contract).collect();
        sides.sort();
        (work, sides)
    }

    /// The reader spends work on a method in proportion to its builds: one pass over the file,
    /// then a lookup per value. A scan of the file per value would make twice the builds cost four
    /// times the work.
    #[test]
    fn the_work_of_reading_named_builds_grows_with_their_number() {
        let (small, sides) = read(400);
        let (large, more) = read(800);
        assert_eq!(sides.len(), 400);
        assert_eq!(more.len(), 800);
        assert!(small > 0 && large < small * 5 / 2, "{small} then {large}");
        // Half are set again and stay unresolved; the others keep their topic.
        assert_eq!(more.iter().filter(|c| *c == "channel::sns").count(), 400);
        assert!(more.contains(&"channel::sns::t0".to_string()));
        assert!(!more.contains(&"channel::sns::t1".to_string()));
    }
}
