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
    /// The Java files that call a method on each name (the last name before the method), read
    /// once: a field builder is changed from another file by such a call.
    callers: std::cell::OnceCell<std::collections::HashMap<&'a str, Vec<&'a str>>>,
    /// The file and the directory of every type the repository's Java files declare, nested ones
    /// included, with its name: a type of the package or the file shadows the one an import names.
    types: std::cell::OnceCell<std::collections::HashSet<(&'a str, &'a str)>>,
}

impl<'a> Builders<'a> {
    /// Whether `name`, the start of a builder chain, may be a type of the repository or of a
    /// package other than the SDK's: its setters are then the repository's, not the SDK's. A type
    /// the file declares beats an import; a type of its directory beats a wildcard import only.
    fn shadowed(&self, code: &Code<'a>, path: &'a str, name: &str) -> bool {
        let sdk = |source: &str| source.starts_with("software.amazon.awssdk.");
        let (package, simple) = name.rsplit_once('.').unwrap_or(("", name));
        if !package.is_empty() {
            return !sdk(package) && package.starts_with(|c: char| c.is_ascii_lowercase());
        }
        let imported = (code.files()[path].imports.iter()).find(|i| i.local == simple);
        if imported.is_some_and(|i| !sdk(&i.source)) {
            return true;
        }
        let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let types = self.types.get_or_init(|| {
            let mut types = std::collections::HashSet::new();
            for (&file, facts) in code.files() {
                let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
                let classes: Vec<&str> = (facts.defs.iter())
                    .filter(|d| d.kind == crate::lang::Kind::Class)
                    .map(|d| d.name.as_str())
                    .collect();
                for &class in &classes {
                    let (outer, name) = class.rsplit_once('.').unwrap_or(("", class));
                    types.insert((file, name));
                    // A nested type is the package's only through its outer type.
                    if !classes.contains(&outer) {
                        types.insert((dir, name));
                    }
                }
            }
            types
        });
        types.contains(&(path, simple)) || imported.is_none() && types.contains(&(dir, simple))
    }

    /// Whether a file other than `path` calls a method on `name`, a name that may be a field.
    fn called_elsewhere(&self, code: &Code<'a>, path: &str, name: &str) -> bool {
        let callers = self.callers.get_or_init(|| {
            let mut callers: std::collections::HashMap<&str, Vec<&str>> = Default::default();
            for (&file, facts) in code.files() {
                if !file.ends_with(".java") {
                    continue;
                }
                for call in &facts.calls {
                    let Some((head, method)) = call.callee.rsplit_once('.') else {
                        continue;
                    };
                    let last = head.rsplit('.').next().unwrap_or(head);
                    if method != "build" && !last.starts_with('(') {
                        let files = callers.entry(last).or_default();
                        if files.last() != Some(&file) {
                            files.push(file);
                        }
                    }
                }
            }
            callers
        });
        (callers.get(name)).is_some_and(|files| files.iter().any(|&f| f != path))
    }

    fn uses(&self, code: &Code<'a>, path: &'a str) -> std::rc::Rc<Uses<'a>> {
        (self.files.borrow_mut().entry(path))
            .or_insert_with(|| std::rc::Rc::new(Uses::read(code, path)))
            .clone()
    }
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

/// The source positions where the mentions of one kind start (in order, once the file is read).
struct Span {
    starts: Vec<usize>,
}

impl Span {
    fn of(range: &Range<usize>) -> Self {
        Self {
            starts: vec![range.start],
        }
    }

    fn add(&mut self, range: &Range<usize>) {
        self.starts.push(range.start);
    }

    /// Whether a mention that is no step of the chain now followed starts before `at.later`. The
    /// steps of that chain start where it does. A mention before the chain, inside it (in an
    /// argument or a lambda of one of its steps) or around it is another use of the builder,
    /// which the chain's own steps cannot be ordered against.
    fn hits(&self, at: &At) -> bool {
        let other = match self.starts.first() {
            Some(&first) if first == at.own => {
                (self.starts).get(self.starts.partition_point(|&s| s <= at.own))
            }
            first => first,
        };
        other.is_some_and(|&s| s < at.later)
    }
}

/// Where a value is made, and how far past that a mention still changes it.
#[derive(Clone)]
struct At {
    range: Range<usize>,
    /// A mention starting before this changes the value: the whole file for a request the call
    /// itself makes, `range.end` for a value made once.
    later: usize,
    /// Where the chain now followed starts.
    own: usize,
}

impl At {
    /// Where a value built by the call at `range` is made, once.
    fn made(range: &Range<usize>) -> Self {
        Self {
            range: range.clone(),
            later: range.end,
            own: range.start,
        }
    }
}

/// Ranges of one file that hold one another or lie apart, in source order, and the one each is in.
#[derive(Default)]
struct Nest<'a> {
    ranges: &'a [Range<usize>],
    parents: Vec<Option<usize>>,
}

impl<'a> Nest<'a> {
    fn read(ranges: &'a [Range<usize>]) -> Self {
        let mut open: Vec<usize> = Vec::new();
        let parents = (ranges.iter().enumerate())
            .map(|(i, r)| {
                while (open.last()).is_some_and(|&o| ranges[o].end <= r.start) {
                    open.pop();
                }
                let parent = open.last().copied();
                open.push(i);
                parent
            })
            .collect();
        Self { ranges, parents }
    }

    /// Where the outermost range that holds `range` and starts after `declared` ends: what is
    /// declared before it outlives each run of its code. Every range that holds `range` counts
    /// for a name with no declaration to place. Past `EVAL_DEPTH` levels out, the end of the file.
    fn outlived(&self, range: &Range<usize>, declared: Option<usize>) -> Option<usize> {
        let mut at = (self.ranges.partition_point(|r| r.start <= range.start)).checked_sub(1);
        let mut end = None;
        for _ in 0..crate::resolve::EVAL_DEPTH {
            let Some(i) = at else {
                return end;
            };
            let held = &self.ranges[i];
            if range.end <= held.end {
                if declared.is_some_and(|d| held.start <= d) {
                    return end;
                }
                end = Some(held.end);
            }
            at = self.parents[i];
        }
        at.map_or(end, |_| Some(usize::MAX))
    }
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
    /// The code each loop of the file runs again (`Facts::repeated`).
    loops: Nest<'a>,
    /// Every lambda and local or anonymous class of the file (`Facts::deferred`).
    deferred: Nest<'a>,
    /// The locals and parameters the file assigns or declares again: no one builder or value.
    rewritten: std::collections::HashSet<(&'a str, &'a str)>,
    /// Whether the file reads a field by its name as a string: any field may change.
    reflective: bool,
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
        let (kind, range, direct) = (mention.kind, mention.range, mention.direct);
        // `this.name` is the field, whatever local hides the bare name.
        if name.len() == mention.name.len() {
            let declared =
                (code.local(path, mention.scope, name)).map_or(mention.scope, |b| b.scope.as_str());
            self.locals
                .entry((declared, name))
                .or_default()
                .add(kind, range, direct);
        }
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
            loops: Nest::read(&facts.repeated),
            deferred: Nest::read(&facts.deferred),
            rewritten: (code.assigned(path).iter())
                .map(|b| (b.scope.as_str(), b.name.as_str()))
                .collect(),
            reflective: (facts.calls.iter()).any(|c| {
                matches!(
                    crate::resolve::method(&c.callee),
                    "getDeclaredField" | "getField" | "getDeclaredFields" | "getFields"
                )
            }),
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
            later: usize::MAX,
            ..at.clone()
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
/// the reader cannot follow, or by a setter of a member of the path `selector`, outside `range`. A
/// named value (`Put p = pb.build()`) is what its declaration built, so only what happened to its
/// builder before that `.build()` counts: a value built first is an immutable copy.
pub(super) fn changed<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: &'a Value,
    range: &Range<usize>,
    selector: &str,
    builders: &Builders<'a>,
) -> bool {
    let setter = super::setter(selector);
    let mut walk = Walk {
        code,
        builders,
        path,
        scope,
        setter: &setter,
        lists: super::transaction(selector),
        uses: builders.uses(code, path),
        seen: std::collections::HashSet::new(),
        depth: 0,
        on: false,
        shared: false,
    };
    let at = At {
        range: range.clone(),
        later: usize::MAX,
        own: usize::MAX,
    };
    walk.value(value, &at)
}

/// The named values and builders one `changed` question has followed.
struct Walk<'a, 'c> {
    code: &'c Code<'a>,
    builders: &'c Builders<'a>,
    /// The file and scope the value now followed is written in.
    path: &'a str,
    scope: &'a str,
    setter: &'c dyn Fn(&str) -> bool,
    /// Whether a list held in a name is judged (`replaced`): on a transaction's path.
    lists: bool,
    uses: std::rc::Rc<Uses<'a>>,
    /// Each named value judged, with how it was reached (`on`, `shared`).
    seen: std::collections::HashSet<(*const (), bool, bool)>,
    /// How many named values and builders deep the question now is.
    depth: usize,
    /// Whether the value now followed is on the name's path: an argument of a step that sets a
    /// member of it (`transactItems(items)`), or an element of a list that is.
    on: bool,
    /// Whether another name of a value, met on the way to the value now followed, is written
    /// more than twice.
    shared: bool,
}

impl<'a> Walk<'a, '_> {
    fn value(&mut self, value: &'a Value, at: &At) -> bool {
        work();
        match value {
            Value::Ref(name) => self.named(name, at),
            Value::Call(top) => {
                let at = &At {
                    own: top.range.start,
                    ..at.clone()
                };
                let steps = || {
                    std::iter::successors(Some(top), |c| match c.receiver.as_deref() {
                        Some(Value::Call(next)) => Some(next),
                        _ => None,
                    })
                };
                // `applyMutation` hands the builder to caller code, which may keep it and set
                // it again after any step of the chain.
                steps().any(|c| crate::resolve::method(&c.callee) == "applyMutation")
                    || steps()
                        .last()
                        .is_some_and(|c| crate::resolve::method(&c.callee) == "builder")
                        && root(value)
                            .is_some_and(|t| self.builders.shadowed(self.code, self.path, t))
                    || root(value).is_some_and(|b| self.builder(b, at))
                    || steps().any(|c| {
                        // `List.of(..)` and `Arrays.asList(..)` hold what they are given.
                        let method = crate::resolve::method(&c.callee);
                        let on =
                            (self.setter)(method) || self.on && matches!(method, "of" | "asList");
                        let before = std::mem::replace(&mut self.on, on);
                        let changed = c.args.iter().any(|a| self.value(a, at));
                        self.on = before;
                        changed
                    })
            }
            Value::List(items) => items.iter().any(|v| self.value(v, at)),
            _ => false,
        }
    }

    /// Whether a list or an array held in a name may have changed. Either changes in place
    /// through any name that holds it (`items.set(0, e)`, `items[0] = e`), so one on a
    /// transaction's path is followed only when its method writes the name twice, where it is
    /// declared and where this value reads it, and the same holds for each other name of it on
    /// the way. A field is written by any method.
    fn replaced(&self, value: &Value, sole: bool) -> bool {
        let list = match value {
            Value::List(_) | Value::New(..) => true,
            Value::Call(made) => crate::resolve::method(&made.callee) == "asList",
            _ => false,
        };
        self.lists && self.on && list && !(sole && !self.shared)
    }

    /// Whether the built value in the local or `final` field `name` was made from a changed
    /// builder, or is a name the file assigns again. Each is judged once, and past `EVAL_DEPTH`
    /// nested ones the name stays unresolved.
    fn named(&mut self, name: &'a str, at: &At) -> bool {
        if self.uses.rewritten.contains(&(self.scope, name)) {
            return true;
        }
        let Some(bound) = self.code.local(self.path, self.scope, name) else {
            return self.constant(name, at);
        };
        let sole = self.uses.written.get(&(bound.scope.as_str(), name)) == Some(&2);
        if self.replaced(&bound.value, sole) {
            return true;
        }
        let reached = (std::ptr::from_ref(bound).cast(), self.on, self.shared);
        if !self.seen.insert(reached) {
            return false;
        }
        self.within(|walk| match &bound.value {
            Value::Call(made) => {
                let at = At::made(&made.range);
                walk.value(&bound.value, &at)
            }
            alias @ Value::Ref(_) => {
                let before = walk.shared;
                walk.shared = before || !sole;
                let changed = walk.value(alias, at);
                walk.shared = before;
                changed
            }
            // An array holds what its elements were when it was made, judged as the request's
            // own: by every mention of their builders.
            Value::List(items) | Value::New(_, items) => items.iter().any(|v| walk.value(v, at)),
            _ => false,
        })
    }

    /// Whether the `final` field `name` holds what a changed builder built: its initializer runs
    /// once, when the object is made, in the file the field is written in.
    fn constant(&mut self, name: &'a str, at: &At) -> bool {
        let Some((path, scope, value)) = self.code.constant_value(self.path, self.scope, name)
        else {
            return false;
        };
        if self.replaced(value, false) {
            return true;
        }
        let reached = (std::ptr::from_ref(value).cast(), self.on, self.shared);
        if !self.seen.insert(reached) {
            return false;
        }
        self.within(|walk| {
            walk.there(path, scope, |walk| match value {
                Value::Call(made) => {
                    let at = At::made(&made.range);
                    walk.value(value, &at)
                }
                alias @ Value::Ref(_) => walk.value(alias, at),
                _ => false,
            })
        })
    }

    /// `f` with the file and scope a value is written in.
    fn there(&mut self, path: &'a str, scope: &'a str, f: impl FnOnce(&mut Self) -> bool) -> bool {
        let uses = self.builders.uses(self.code, path);
        let kept = (self.path, self.scope);
        let before = std::mem::replace(&mut self.uses, uses);
        (self.path, self.scope) = (path, scope);
        let changed = f(self);
        (self.path, self.scope) = kept;
        self.uses = before;
        changed
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

    /// Whether `name` is a local or a `final` field: a name that holds a builder or a value.
    fn holds(&self, name: &'a str) -> bool {
        self.code.local(self.path, self.scope, name).is_some()
            || self
                .code
                .constant_value(self.path, self.scope, name)
                .is_some()
    }

    /// Whether the builder `name` was changed as seen from `at`, or is another name of one, or was
    /// made from a changed value.
    fn builder(&mut self, name: &'a str, at: &At) -> bool {
        // `this.name` is the field, whatever local or parameter hides the bare name.
        let bare = name.strip_prefix("this.");
        let name = bare.unwrap_or(name);
        let local = match bare {
            Some(_) => None,
            None => self.code.local(self.path, self.scope, name),
        };
        let field = local.is_none()
            && (bare.is_some()
                || (self.code.constant_value(self.path, self.scope, name)).is_some());
        // A type or a parameter holds no builder to follow. A name the file assigns again or
        // declares twice holds no one builder: a change the reader cannot follow.
        if local.is_none() && !field {
            return self.uses.rewritten.contains(&(self.scope, name));
        }
        if field
            && (self.uses.reflective || self.builders.called_elsewhere(self.code, self.path, name))
        {
            return true;
        }
        let scope = local.map_or(self.scope, |b| b.scope.as_str());
        let declared = match local.map(|b| &b.value) {
            Some(Value::Call(c)) => Some(c.range.start),
            _ => None,
        };
        // A builder declared before a loop its value is made in, set later in that loop, is set
        // before the next turn builds. One declared outside a lambda or class its value is made
        // in is set again wherever its caller runs it, later in the source or not.
        let mut later = at.later;
        if let Some(end) = self.uses.loops.outlived(&at.range, declared) {
            later = later.max(end);
        }
        if (self.uses.deferred.outlived(&at.range, declared)).is_some() {
            later = usize::MAX;
        }
        let at = At {
            later,
            ..at.clone()
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
                root(made).is_some_and(|r| self.holds(r))
                    || self.within(|walk| {
                        let at = At::made(&c.range);
                        walk.value(made, &at)
                    })
            }
            // Not a builder of the SDK: a type or a variable of the file's own.
            Some(Value::New(..)) => true,
            Some(_) => false,
            // A `final` field built from a name, or from another builder.
            None => {
                let Some((path, scope, value)) =
                    self.code.constant_value(self.path, self.scope, name)
                else {
                    return false;
                };
                match value {
                    Value::Ref(_) => true,
                    made @ Value::Call(c) => self.there(path, scope, |walk| {
                        root(made).is_some_and(|r| walk.holds(r))
                            || walk.within(|walk| {
                                let at = At::made(&c.range);
                                walk.value(made, &at)
                            })
                    }),
                    _ => false,
                }
            }
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
