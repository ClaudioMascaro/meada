//! Scoped, ordered selector effects. Syntax supplies assignments; a reader may add
//! known library mutation calls. Exact binding/caller resolution supplies object identity.
use super::*;

pub(crate) struct MemberEffect<'a> {
    pub scope: &'a str,
    pub object: &'a str,
    pub field: Option<&'a str>,
    pub start: usize,
}

#[derive(Clone, Copy)]
pub(super) struct Selection<'a> {
    pub site: Option<(&'a str, usize)>,
    pub writes: CallWrites,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Object<'a> {
    Constant(&'a str, &'a str),
    Local(&'a str, &'a str, &'a str),
    Value(&'a str, usize),
}

#[derive(Clone, Copy)]
struct Moment<'a> {
    path: &'a str,
    scope: &'a str,
    start: usize,
}
struct Effect<'a> {
    field: Option<&'a str>,
    moments: Vec<Moment<'a>>,
}

#[derive(Default)]
pub(super) struct Index<'a> {
    effects: HashMap<Object<'a>, Vec<Effect<'a>>>,
    uncertain: Vec<(&'a str, &'a str, Effect<'a>)>,
    // A default object can survive more than one invocation. Source order in one
    // invocation alone cannot prove its initial fields are still present.
    defaults: HashSet<Object<'a>>,
    /// Each file's outermost loops (`Facts::loops`).
    loops: HashMap<&'a str, &'a [std::ops::Range<usize>]>,
    cut: bool,
}

impl<'a> Index<'a> {
    fn read(code: &Code<'a>, extra: CallWrites) -> Self {
        let mut out = Self::default();
        for (&path, lean) in code.files() {
            let facts = match &lean.member_writes {
                Some(_) => *lean,
                None => match code.full(path) {
                    Some(f) => f,
                    None => {
                        out.cut = true;
                        continue;
                    }
                },
            };
            let Some(writes) = &facts.member_writes else {
                out.cut = true;
                continue;
            };
            if facts.budget.is_some() {
                out.cut = true;
            }
            out.loops.insert(path, &facts.loops);
            for write in writes {
                out.add(
                    code,
                    path,
                    MemberEffect {
                        scope: &write.scope,
                        object: &write.object,
                        field: write.field.as_deref(),
                        start: write.range.end,
                    },
                );
            }
            for index in 0..facts.calls.len() {
                if !code.charge(path, 1) {
                    out.cut = true;
                    break;
                }
                let Some(effects) = extra(code, path, index) else {
                    out.cut = true;
                    continue;
                };
                for effect in effects {
                    out.add(code, path, effect);
                }
            }
        }
        out
    }

    fn add(&mut self, code: &Code<'a>, path: &'a str, write: MemberEffect<'a>) {
        let moments = vec![Moment {
            path,
            scope: write.scope,
            start: write.start,
        }];
        self.object(
            code,
            path,
            write.scope,
            write.object,
            write.field,
            moments,
            &mut HashSet::new(),
            0,
        );
    }

    #[expect(clippy::too_many_arguments)]
    fn object(
        &mut self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        field: Option<&'a str>,
        moments: Vec<Moment<'a>>,
        busy: &mut HashSet<Object<'a>>,
        depth: usize,
    ) {
        if depth >= EVAL_DEPTH || !code.charge(path, 1) {
            self.cut = true;
            return;
        }
        let binding = code.resolver.read_member(path, scope, name);
        let binding = match binding {
            Binding::Local(at, within, name) => code.resolver.consts[at]
                .get(crate::qualify(within, name).as_str())
                .map_or(binding, |&n| Binding::Const(at, n)),
            other => other,
        };
        let (key, value, at, within) = match binding {
            Binding::Const(at, name) => {
                let Some((value, within)) = code.held(at, name) else {
                    self.cut = true;
                    return;
                };
                (Object::Constant(at, name), value, at, within)
            }
            Binding::Local(at, within, name) => {
                let Some(bound) = code.local(at, within, name) else {
                    self.unknown(code, path, scope, field, moments, busy, depth);
                    return;
                };
                (
                    Object::Local(at, &bound.scope, &bound.name),
                    &bound.value,
                    at,
                    bound.within.as_str(),
                )
            }
            Binding::Param(function, n, None) => {
                self.parameter(code, function, n, field, moments, busy, depth);
                return;
            }
            Binding::Hidden | Binding::Unknown => {
                self.unknown(code, path, scope, field, moments, busy, depth);
                return;
            }
            _ => return,
        };
        if !busy.insert(key) {
            self.cut = true;
            return;
        }
        match value {
            Value::Ref(name) => {
                self.object(code, at, within, name, field, moments, busy, depth + 1)
            }
            Value::Await(inner) => {
                if let Value::Ref(name) = inner.as_ref() {
                    self.object(code, at, within, name, field, moments, busy, depth + 1)
                } else {
                    self.unknown(code, at, within, field, moments, busy, depth)
                }
            }
            Value::Call(_) => self.unknown(code, at, within, field, moments, busy, depth),
            _ => self
                .effects
                .entry(key)
                .or_default()
                .push(Effect { field, moments }),
        }
        busy.remove(&key);
    }

    #[expect(clippy::too_many_arguments)]
    fn parameter(
        &mut self,
        code: &Code<'a>,
        function: Target<'a>,
        n: usize,
        field: Option<&'a str>,
        moments: Vec<Moment<'a>>,
        busy: &mut HashSet<Object<'a>>,
        depth: usize,
    ) {
        let width = code.resolver.callers.get(&function).map_or(0, Vec::len)
            + code
                .resolver
                .extension_callers
                .get(&function)
                .map_or(0, Vec::len);
        if !code.charge(function.path, width) {
            self.cut = true;
            return;
        }
        let callers: Vec<_> = code
            .resolver
            .callers
            .get(&function)
            .into_iter()
            .flatten()
            .chain(
                code.resolver
                    .extension_callers
                    .get(&function)
                    .into_iter()
                    .flatten(),
            )
            .copied()
            .collect();
        if callers.is_empty() {
            self.uncertain
                .push((function.path, function.name, Effect { field, moments }));
            return;
        }
        for caller in callers {
            if moments.iter().any(|m| {
                m.path == caller.path
                    && m.start == code.files()[caller.path].calls[caller.index].range.start
            }) {
                self.cut = true;
                continue;
            }
            let Some(call) = code.call(caller.path, caller.index) else {
                self.cut = true;
                continue;
            };
            let Some(values) = code.param_values(function, n, Some((caller.path, caller.index)))
            else {
                self.cut = true;
                continue;
            };
            for (at, within, value) in values {
                if !code.charge(at, moments.len()) {
                    self.cut = true;
                    break;
                }
                let mut route = moments.clone();
                route.push(Moment {
                    path: caller.path,
                    scope: &call.scope,
                    start: call.range.start,
                });
                if let Value::Ref(name) = value {
                    self.object(code, at, within, name, field, route, busy, depth + 1);
                } else if let Some(pointer) = object_value(value) {
                    let key = Object::Value(at, pointer);
                    if code.arg(caller.path, caller.index, function, n).is_none() {
                        self.defaults.insert(key);
                    }
                    self.effects.entry(key).or_default().push(Effect {
                        field,
                        moments: route,
                    });
                } else if !matches!(
                    value,
                    Value::Str(_)
                        | Value::Scalar(_)
                        | Value::Undefined
                        | Value::External(_)
                        | Value::List(_)
                ) {
                    // An opaque actual may alias a selected object in this caller. Its
                    // scoped uncertainty is not complete empty mutation knowledge.
                    self.unknown(code, at, within, field, route, busy, depth + 1);
                }
            }
        }
    }

    // A reassigned alias can still refer to an argument of its function. Refuse an
    // unchanged field for those exact actual objects, with the same caller order;
    // do not suppress sibling objects throughout the file.
    #[expect(clippy::too_many_arguments)]
    fn unknown(
        &mut self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        field: Option<&'a str>,
        moments: Vec<Moment<'a>>,
        busy: &mut HashSet<Object<'a>>,
        depth: usize,
    ) {
        if depth >= EVAL_DEPTH || !code.charge(path, moments.len()) {
            self.cut = true;
            return;
        }
        self.uncertain.push((
            path,
            scope,
            Effect {
                field,
                moments: moments.clone(),
            },
        ));
        let mut within = scope;
        while !within.is_empty() {
            if !code.charge(path, 1) {
                self.cut = true;
                return;
            }
            if let Some(def) = code.def(path, within)
                && matches!(def.kind, Kind::Function | Kind::Method | Kind::Inline)
            {
                let marker = Object::Local(path, within, "");
                if !busy.insert(marker) {
                    self.cut = true;
                    return;
                }
                let function = Target { path, name: within };
                for n in 0..def.params.len() {
                    if !code.charge(path, moments.len() + 1) {
                        self.cut = true;
                        break;
                    }
                    self.parameter(code, function, n, field, moments.clone(), busy, depth + 1);
                }
                busy.remove(&marker);
                break;
            }
            within = scope_of(within);
        }
    }

    /// Whether one loop of `path` holds both positions: a write after a read in it comes before
    /// the read of the next turn.
    fn looped(&self, path: &str, a: usize, b: usize) -> bool {
        let (low, high) = (a.min(b), a.max(b));
        self.loops.get(path).is_some_and(|loops| {
            let at = loops.partition_point(|l| l.end <= low);
            // A write's moment is where it ends, which may be where its loop ends.
            (loops.get(at)).is_some_and(|l| l.start <= low && high <= l.end)
        })
    }

    fn changed(
        &self,
        object: Object<'a>,
        field: &str,
        read: &[Moment<'a>],
        timed: bool,
        charge: impl Fn() -> bool,
    ) -> bool {
        if self.cut {
            return true;
        }
        self.effects
            .get(&object)
            .into_iter()
            .flatten()
            .map(|effect| (None, effect))
            .chain(
                self.uncertain
                    .iter()
                    .map(|(path, scope, effect)| (Some((*path, *scope)), effect)),
            )
            .any(|(uncertain, effect)| {
                if !charge() {
                    return true;
                }
                if let Some((write_path, write_scope)) = uncertain {
                    let (path, within) = match object {
                        Object::Constant(path, name) => (path, scope_of(name)),
                        Object::Local(path, scope, _) => (path, scope),
                        Object::Value(path, _) => (path, ""),
                    };
                    if path != write_path
                        || !(within.is_empty()
                            || write_scope == within
                            || write_scope
                                .strip_prefix(within)
                                .is_some_and(|s| s.starts_with('.')))
                    {
                        return false;
                    }
                }
                if effect.field.is_some_and(|f| f != field) {
                    return false;
                }
                if !timed || self.defaults.contains(&object) {
                    return true;
                }
                // Compare the outermost shared invocation first. Different call sites establish
                // order even when their callee bodies use the same source positions.
                for before in effect.moments.iter().rev() {
                    if let Some(after) = read
                        .iter()
                        .rev()
                        .find(|m| m.path == before.path && m.scope == before.scope)
                        && before.start != after.start
                    {
                        return before.start < after.start
                            || self.looped(before.path, before.start, after.start);
                    }
                }
                // A call's arguments are read before that callee can mutate them. Sharing
                // only an outer invocation does not establish the inner effect's order.
                !read.first().is_some_and(|at| {
                    effect
                        .moments
                        .iter()
                        .any(|m| m.path == at.path && m.scope == at.scope && m.start == at.start)
                })
            })
    }
}

impl<'a> Code<'a> {
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn value_within_at(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        frames: &[Frame<'a>],
        site: Option<(&'a str, usize)>,
        types: fn(&str) -> bool,
        writes: CallWrites,
    ) -> Value {
        let mut fold = Fold {
            chain: true,
            new_types: Some(types),
            selected: Some(Selection { site, writes }),
            ..Fold::default()
        };
        let frames: Vec<_> = frames.iter().rev().copied().collect();
        let fresh_argument = site
            .and_then(|(path, index)| self.call(path, index))
            .is_some_and(|call| {
                call.args
                    .iter()
                    .chain(call.kwargs.iter().map(|(_, v)| v))
                    .any(|v| std::ptr::eq(v, value))
            });
        if !fresh_argument && let Some(pointer) = object_value(value) {
            fold.objects.push((Object::Value(path, pointer), pointer));
        }
        unbind(
            self.eval(path, scope, value, &frames, 0, &mut fold, &mut None),
            true,
        )
    }

    pub(super) fn member_changed(
        &self,
        object: Object<'a>,
        field: &str,
        selected: Selection<'a>,
        frames: &[Frame<'a>],
    ) -> bool {
        let mut read = Vec::new();
        if let Some((path, index)) = selected.site {
            let Some(call) = self.call(path, index) else {
                return true;
            };
            read.push(Moment {
                path,
                scope: &call.scope,
                start: call.range.start,
            });
        }
        for frame in frames {
            let Some(call) = self.call(frame.path, frame.index) else {
                return true;
            };
            read.push(Moment {
                path: frame.path,
                scope: &call.scope,
                start: call.range.start,
            });
        }
        let mut indexes = self.mutations.borrow_mut();
        let index = indexes
            .entry(selected.writes as usize)
            .or_insert_with(|| Index::read(self, selected.writes));
        let path = match object {
            Object::Constant(path, _) | Object::Local(path, _, _) | Object::Value(path, _) => path,
        };
        index.changed(object, field, &read, selected.site.is_some(), || {
            self.charge(path, 1)
        })
    }
}
