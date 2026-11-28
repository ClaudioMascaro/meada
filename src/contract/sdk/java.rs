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

type MethodCounts<'a> = std::collections::HashMap<(&'a str, Option<usize>), u32>;

#[derive(Default)]
pub(crate) struct Builders<'a> {
    files: std::cell::RefCell<std::collections::HashMap<&'a str, std::rc::Rc<Uses<'a>>>>,
    callers: std::cell::OnceCell<(std::collections::HashMap<&'a str, Vec<&'a str>>, bool)>,
    types: std::cell::OnceCell<std::collections::HashSet<(&'a str, &'a str)>>,
    methods: std::cell::OnceCell<Option<MethodCounts<'a>>>,
}

impl<'a> Builders<'a> {
    fn overridden(&self, code: &Code<'a>, def: &'a crate::lang::Def) -> bool {
        let simple = |name: &'a str| {
            let own = name[crate::parent(name).len()..].trim_start_matches('.');
            own.split('(').next().unwrap_or(own)
        };
        let methods = self.methods.get_or_init(|| {
            let mut methods = std::collections::HashMap::new();
            for facts in code.files().values() {
                if facts.budget.is_some() {
                    return None;
                }
                for d in (facts.defs.iter()).filter(|d| d.kind == crate::lang::Kind::Method) {
                    *methods
                        .entry((simple(&d.name), Some(d.params.len())))
                        .or_insert(0) += 1;
                }
                for name in &facts.anonymous {
                    *methods.entry((name.as_str(), None)).or_insert(0) += 1;
                }
            }
            Some(methods)
        });
        let Some(methods) = methods else {
            return true;
        };
        let name = simple(&def.name);
        methods.get(&(name, Some(def.params.len()))) > Some(&1)
            || methods.contains_key(&(name, None))
    }

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
                    if !classes.contains(&outer) {
                        types.insert((dir, name));
                    }
                }
            }
            types
        });
        types.contains(&(path, simple)) || imported.is_none() && types.contains(&(dir, simple))
    }

    fn called_elsewhere(&self, code: &Code<'a>, path: &str, name: &str, private: bool) -> bool {
        let (callers, reflected) = self.callers.get_or_init(|| {
            let mut callers: std::collections::HashMap<&str, Vec<&str>> = Default::default();
            let mut reflected = false;
            for (&file, facts) in code.files() {
                if !file.ends_with(".java") {
                    continue;
                }
                reflected |= facts.reflects;
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
            (callers, reflected)
        });
        (*reflected || !private)
            && (callers.get(name)).is_some_and(|files| files.iter().any(|&f| f != path))
    }

    fn uses(&self, code: &Code<'a>, path: &'a str) -> std::rc::Rc<Uses<'a>> {
        (self.files.borrow_mut().entry(path))
            .or_insert_with(|| std::rc::Rc::new(Uses::read(code, path)))
            .clone()
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn work() {
    #[cfg(test)]
    WORK.with(|w| w.set(w.get() + 1));
}

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

    fn hits(&self, at: &At) -> bool {
        let other = match self.starts.first() {
            Some(&first) if first == at.own => {
                (self.starts).get(self.starts.partition_point(|&s| s <= at.own))
            }
            first => first,
        };
        other.is_some_and(|&s| s < at.later)
    }

    fn hits_once(&self, at: &At) -> bool {
        let skip = usize::from(self.starts.first() == Some(&at.own));
        (self.starts.get(skip)).is_some_and(|&s| s < at.later)
    }
}

#[derive(Clone)]
struct At {
    range: Range<usize>,
    later: usize,
    own: usize,
}

impl At {
    fn made(range: &Range<usize>) -> Self {
        Self {
            range: range.clone(),
            later: range.end,
            own: range.start,
        }
    }
}

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
    direct: u32,
    bare: u32,
    other: Option<Span>,
    sets: Vec<(&'a str, Span)>,
}

impl<'a> Mentions<'a> {
    fn add(&mut self, kind: Kind<'a>, range: &Range<usize>, direct: bool, bare: bool) {
        self.direct += u32::from(direct);
        self.bare += u32::from(bare);
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

    fn list_changed(&self, at: &At) -> bool {
        self.other.as_ref().is_some_and(|s| s.hits_once(at))
            || (self.sets.iter()).any(|(method, span)| {
                !KEEPS.contains(method) && (span.starts.first()).is_some_and(|&s| s < at.later)
            })
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
    Harmless,
    Set(&'a str),
    Other,
}

#[derive(Default)]
struct Uses<'a> {
    locals: std::collections::HashMap<(&'a str, &'a str), Mentions<'a>>,
    fields: std::collections::HashMap<&'a str, Mentions<'a>>,
    loops: Nest<'a>,
    deferred: Nest<'a>,
    rewritten: std::collections::HashSet<(&'a str, &'a str)>,
    reflective: bool,
    written: std::collections::HashMap<(&'a str, &'a str), u32>,
}

struct Mention<'a, 'r> {
    scope: &'a str,
    name: &'a str,
    kind: Kind<'a>,
    range: &'r Range<usize>,
    direct: bool,
    bare: bool,
}

impl<'a> Uses<'a> {
    fn add(&mut self, code: &Code<'a>, path: &'a str, mention: Mention<'a, '_>) {
        work();
        let name = mention.name.strip_prefix("this.").unwrap_or(mention.name);
        let (kind, range, direct, bare) =
            (mention.kind, mention.range, mention.direct, mention.bare);
        if name.len() == mention.name.len() {
            let declared =
                (code.local(path, mention.scope, name)).map_or(mention.scope, |b| b.scope.as_str());
            self.locals
                .entry((declared, name))
                .or_default()
                .add(kind, range, direct, bare);
        }
        self.fields
            .entry(name)
            .or_default()
            .add(kind, range, direct, bare);
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
            reflective: facts.reflects,
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
                    bare: false,
                };
                uses.add(code, path, mention);
            }
            for arg in &call.args {
                let bare = matches!(arg, Value::Ref(_));
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
                            bare,
                        },
                    )
                });
            }
        }
        for bound in code.bindings(path).iter().chain(code.assigned(path)) {
            let bare = matches!(bound.value, Value::Ref(_));
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
                        bare,
                    },
                )
            });
        }
        (uses.locals.values_mut()).for_each(Mentions::sorted);
        (uses.fields.values_mut()).for_each(Mentions::sorted);
        uses
    }

    fn changed(
        &self,
        scope: &str,
        name: &str,
        field: bool,
        setter: &dyn Fn(&str) -> bool,
        at: &At,
    ) -> bool {
        let anywhere = At {
            later: usize::MAX,
            ..at.clone()
        };
        self.locals
            .get(&(scope, name))
            .is_some_and(|m| m.changed(setter, at))
            || field && (self.fields.get(name)).is_some_and(|m| m.changed(setter, &anywhere))
    }

    fn unseen_list(&self, scope: &str, name: &str) -> bool {
        let seen = (self.locals.get(&(scope, name))).map_or(0, |m| m.direct + m.bare);
        (self.written.get(&(scope, name))).is_none_or(|&written| written > 1 + seen)
    }

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

pub(super) struct Judged {
    pub(super) changed: bool,
    pub(super) parameter: Option<usize>,
    pub(super) fresh: bool,
}

pub(super) fn made_here(value: &Value) -> bool {
    match value {
        Value::List(_) | Value::New(..) => true,
        Value::Call(c) => {
            matches!(crate::resolve::method(&c.callee), "of" | "asList")
                || std::iter::successors(Some(c), |c| match c.receiver.as_deref() {
                    Some(Value::Call(next)) => Some(next),
                    _ => None,
                })
                .last()
                .is_some_and(|c| crate::resolve::method(&c.callee) == "builder")
        }
        _ => false,
    }
}

#[expect(clippy::too_many_arguments)]
pub(super) fn changed<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: &'a Value,
    range: &Range<usize>,
    selector: &str,
    builders: &Builders<'a>,
    read: bool,
) -> Judged {
    let setter = super::setter(selector);
    let list = super::setter(selector.split('.').next().unwrap_or(selector));
    let mut walk = Walk {
        code,
        builders,
        path,
        scope,
        setter: &setter,
        lists: super::reads_entries(selector).then_some(&list),
        uses: builders.uses(code, path),
        seen: std::collections::HashSet::new(),
        depth: 0,
        on: read,
        shared: false,
        parameter: None,
        fresh: false,
    };
    let at = match read {
        true => At::made(range),
        false => At {
            range: range.clone(),
            later: usize::MAX,
            own: usize::MAX,
        },
    };
    let changed = read && walk.passed(value, &at) || walk.value(value, &at);
    Judged {
        changed,
        parameter: walk.parameter,
        fresh: walk.fresh,
    }
}

pub(super) fn hands(value: &Value, name: &str) -> bool {
    let mut found = false;
    handed(value, &mut |n| found |= n == name);
    found
}

struct Walk<'a, 'c> {
    code: &'c Code<'a>,
    builders: &'c Builders<'a>,
    path: &'a str,
    scope: &'a str,
    setter: &'c dyn Fn(&str) -> bool,
    lists: Option<&'c dyn Fn(&str) -> bool>,
    uses: std::rc::Rc<Uses<'a>>,
    seen: std::collections::HashSet<(*const (), bool, bool)>,
    depth: usize,
    on: bool,
    shared: bool,
    parameter: Option<usize>,
    fresh: bool,
}

const KEEPS: [&str; 7] = [
    "sort", "size", "isEmpty", "get", "contains", "stream", "forEach",
];

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
                steps().any(|c| crate::resolve::method(&c.callee) == "applyMutation")
                    || steps()
                        .last()
                        .is_some_and(|c| crate::resolve::method(&c.callee) == "builder")
                        && root(value)
                            .is_some_and(|t| self.builders.shadowed(self.code, self.path, t))
                    || root(value).is_some_and(|b| self.builder(b, at))
                    || steps().any(|c| {
                        let method = crate::resolve::method(&c.callee);
                        if let [sole] = &c.args[..]
                            && self.lists.is_some_and(|list| list(method))
                            && self.passed(sole, at)
                        {
                            return true;
                        }
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

    fn listed(&self, value: &Value) -> bool {
        let list = match value {
            Value::List(_) | Value::New(..) => true,
            Value::Call(made) => crate::resolve::method(&made.callee) == "asList",
            _ => false,
        };
        self.lists.is_some() && self.on && list
    }

    fn list_changed(
        &self,
        scope: &'a str,
        name: &'a str,
        declared: Option<usize>,
        at: &At,
    ) -> bool {
        let at = self.widened(at, declared);
        self.uses.rewritten.contains(&(scope, name))
            || self.uses.unseen_list(scope, name)
            || (self.uses.locals.get(&(scope, name))).is_some_and(|m| m.list_changed(&at))
    }

    fn widened(&self, at: &At, declared: Option<usize>) -> At {
        let mut later = at.later;
        if let Some(end) = self.uses.loops.outlived(&at.range, declared) {
            later = later.max(end);
        }
        if (self.uses.deferred.outlived(&at.range, declared)).is_some() {
            later = usize::MAX;
        }
        At {
            later,
            ..at.clone()
        }
    }

    fn passed(&mut self, mut value: &'a Value, at: &At) -> bool {
        for _ in 0..crate::resolve::EVAL_DEPTH {
            let Value::Ref(name) = value else {
                return false;
            };
            match self.code.local(self.path, self.scope, name) {
                Some(bound) => value = &bound.value,
                None if (self.code.constant_value(self.path, self.scope, name)).is_some() => {
                    return false;
                }
                None => return !self.kept(name, at),
            }
        }
        true
    }

    // ponytail: a varargs parameter, an alias of the parameter, a read in a lambda and a function
    // another definition may stand for (an override) name no table; following them needs the
    // caller's arguments as one list, the alias's own mentions, where the lambda runs and which
    // override a call dispatches to.
    fn kept(&mut self, name: &'a str, at: &At) -> bool {
        let Some(def) = self.code.def(self.path, self.scope) else {
            return false;
        };
        let Some(index) = def.params.iter().position(|p| p.name == name) else {
            return false;
        };
        let declared = Some(def.range.start);
        let function = crate::resolve::Target {
            path: self.path,
            name: self.scope,
        };
        let everywhere = At {
            later: usize::MAX,
            ..at.clone()
        };
        let mentions = self.uses.locals.get(&(self.scope, name));
        if def.params[index].binds != crate::lang::Binds::Any
            || self.code.name_matched(function)
            || self.builders.overridden(self.code, def)
            || (self.uses.deferred.outlived(&at.range, declared)).is_some()
            || self.list_changed(self.scope, name, declared, at)
            || mentions
                .is_some_and(|m| (m.other.as_ref()).is_some_and(|s| s.hits_once(&everywhere)))
            || self.parameter.is_some_and(|p| p != index)
        {
            return false;
        }
        self.fresh |= self.list_changed(self.scope, name, declared, &everywhere);
        self.parameter = Some(index);
        true
    }

    fn named(&mut self, name: &'a str, at: &At) -> bool {
        if self.uses.rewritten.contains(&(self.scope, name)) {
            return true;
        }
        let Some(bound) = self.code.local(self.path, self.scope, name) else {
            return self.constant(name, at);
        };
        let sole = self.uses.written.get(&(bound.scope.as_str(), name)) == Some(&2);
        let declared = match &bound.value {
            Value::Call(c) => Some(c.range.start),
            _ => None,
        };
        if self.listed(&bound.value)
            && (self.shared || self.list_changed(bound.scope.as_str(), name, declared, at))
        {
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
            Value::List(items) | Value::New(_, items) => items.iter().any(|v| walk.value(v, at)),
            _ => false,
        })
    }

    fn constant(&mut self, name: &'a str, at: &At) -> bool {
        let Some((path, scope, value)) = self.code.constant_value(self.path, self.scope, name)
        else {
            return false;
        };
        if self.listed(value) {
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

    fn within(&mut self, f: impl FnOnce(&mut Self) -> bool) -> bool {
        if self.depth >= crate::resolve::EVAL_DEPTH {
            return true;
        }
        self.depth += 1;
        let changed = f(self);
        self.depth -= 1;
        changed
    }

    fn holds(&self, name: &'a str) -> bool {
        self.code.local(self.path, self.scope, name).is_some()
            || self
                .code
                .constant_value(self.path, self.scope, name)
                .is_some()
    }

    fn builder(&mut self, name: &'a str, at: &At) -> bool {
        let bare = name.strip_prefix("this.");
        let name = bare.unwrap_or(name);
        let local = match bare {
            Some(_) => None,
            None => self.code.local(self.path, self.scope, name),
        };
        let constant = match local {
            Some(_) => None,
            None => self.code.constant_value(self.path, self.scope, name),
        };
        let field = local.is_none() && (bare.is_some() || constant.is_some());
        if local.is_none() && !field {
            return self.uses.rewritten.contains(&(self.scope, name));
        }
        if field {
            let private = constant.is_some_and(|(declared, ..)| declared == self.path)
                && (self.code.private_fields(self.path).iter()).any(|f| f == name);
            let (code, path) = (self.code, self.path);
            if self.uses.reflective || self.builders.called_elsewhere(code, path, name, private) {
                return true;
            }
        }
        let scope = local.map_or(self.scope, |b| b.scope.as_str());
        let declared = match local.map(|b| &b.value) {
            Some(Value::Call(c)) => Some(c.range.start),
            _ => None,
        };
        let at = self.widened(at, declared);
        if self.uses.unseen(scope, name, field)
            || self.uses.changed(scope, name, field, self.setter, &at)
        {
            return true;
        }
        match local.map(|b| &b.value) {
            Some(Value::Ref(_)) => true,
            Some(made @ Value::Call(c)) => {
                root(made).is_some_and(|r| self.holds(r))
                    || self.within(|walk| {
                        let at = At::made(&c.range);
                        walk.value(made, &at)
                    })
            }
            Some(Value::New(..)) => true,
            Some(_) => false,
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
        || classes.iter().any(|class| {
            call.callee == format!("{class}.{method}")
                && names.contains(&format!("java.lang.{class}"))
        })
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

    #[test]
    fn the_work_of_reading_named_builds_grows_with_their_number() {
        let (small, sides) = read(400);
        let (large, more) = read(800);
        assert_eq!(sides.len(), 400);
        assert_eq!(more.len(), 800);
        assert!(small > 0 && large < small * 5 / 2, "{small} then {large}");
        assert_eq!(more.iter().filter(|c| *c == "channel::sns").count(), 400);
        assert!(more.contains(&"channel::sns::t0".to_string()));
        assert!(!more.contains(&"channel::sns::t1".to_string()));
    }
}
