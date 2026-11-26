use super::super::{Binding, Caller, Code, Fold, Resolver, Target, Work};
use crate::lang::Value;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Owner<'a> {
    Class(Target<'a>),
    Literal(Target<'a>),
    Unknown,
    UnknownIn(Target<'a>),
}

type Site<'a> = (&'a str, usize);
type Marks<'a> = HashMap<(Owner<'a>, Option<&'a str>), Option<Site<'a>>>;

/// Keep a single write's location so a field may exclude its own initialization.
#[derive(Default)]
pub(in crate::resolve) struct Writes<'a> {
    program: Marks<'a>,
    tests: Marks<'a>,
    incomplete: [bool; 2],
}

impl<'a> Writes<'a> {
    pub(in crate::resolve) fn index(r: &Resolver<'a>) -> Self {
        let mut result = Self::default();
        for (&path, &facts) in r.files {
            let test = crate::http::is_test_file(path);
            let Some(writes) = &facts.member_writes else {
                result.incomplete[usize::from(test)] = true;
                continue;
            };
            let marks = if test {
                &mut result.tests
            } else {
                &mut result.program
            };
            for w in writes {
                let enclosing = match r.class(path, &w.scope) {
                    Binding::Def(t) => Some(t),
                    _ => None,
                };
                let owner = if w.object == "this" {
                    enclosing.map(Owner::Class)
                } else {
                    let receiver = r.at_ts_site(path, w.range.start, || {
                        let b = r.read_member(path, &w.scope, &w.object);
                        if let Binding::Const(p, name) = b
                            && r.files[p].ts_literal_methods.contains_key(name)
                        {
                            return b;
                        }
                        r.instance(b, &mut Work::default())
                    });
                    match receiver {
                        Binding::Instance(t) => Some(Owner::Class(t)),
                        Binding::Const(path, name) => Some(Owner::Literal(Target { path, name })),
                        // A property written directly on the constructor is static.
                        Binding::Def(t) if r.is_class(t) => continue,
                        _ => None,
                    }
                };
                let mut mark = |owner| {
                    marks
                        .entry((owner, w.field.as_deref()))
                        .and_modify(|only| *only = None)
                        .or_insert(Some((path, w.range.start)));
                };
                if let Some(owner) = owner {
                    mark(owner);
                } else {
                    mark(Owner::Unknown);
                    if let Some(t) = enclosing {
                        mark(Owner::UnknownIn(t));
                    }
                }
            }
        }
        result
    }
}

impl<'a> Resolver<'a> {
    pub(in crate::resolve) fn ts_literal_method(
        &self,
        path: &'a str,
        name: &'a str,
        member: &str,
    ) -> Option<Binding<'a>> {
        let &overwritten = self.files[path].ts_literal_methods.get(name)?.get(member)?;
        let owner = Owner::Literal(Target { path, name });
        let written = self.ts_writes.as_ref().is_none_or(|writes| {
            [
                (&writes.program, writes.incomplete[0]),
                (&writes.tests, writes.incomplete[1]),
            ]
            .into_iter()
            .any(|(marks, incomplete)| {
                incomplete
                    || [owner, Owner::Unknown].into_iter().any(|o| {
                        marks.contains_key(&(o, Some(member))) || marks.contains_key(&(o, None))
                    })
            })
        });
        Some(if overwritten || written {
            Binding::Unknown
        } else {
            self.def(path, &crate::qualify(name, member))
        })
    }

    /// A field or local binds its constructor where the new expression was written.
    pub(in crate::resolve) fn ts_construction(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &str,
        class: &'a str,
        work: &mut Work<'a>,
    ) -> Binding<'a> {
        let info = self.files[path].ts_classes.get(scope);
        let (scope, position) = if let Some(at) = info.and_then(|c| c.constructed.get(name)) {
            let ctor = crate::qualify(scope, "constructor");
            let Some((&ctor, _)) = self.defs[path].get_key_value(ctor.as_str()) else {
                return Binding::Unknown;
            };
            (ctor, Some(*at))
        } else {
            let local = self.ts_local(path, scope, name).flatten();
            (
                scope,
                local
                    .map(|l| l.declared)
                    .or_else(|| info.and_then(|c| c.initialized.get(name).copied())),
            )
        };
        let mut resolve = || self.resolve(path, scope, class, &mut self.seen(path, scope), work);
        match position {
            Some(at) => self.at_ts_site(path, at, resolve),
            None => resolve(),
        }
    }

    pub(in crate::resolve) fn ts_field_written(
        &self,
        class: Target<'a>,
        field: &str,
        test: bool,
    ) -> bool {
        let Some(writes) = &self.ts_writes else {
            return true;
        };
        if writes.incomplete[0] || (test && writes.incomplete[1]) {
            return true;
        }
        let info = self.files[class.path].ts_classes.get(class.name);
        let seed = info
            .and_then(|c| {
                c.constructed
                    .get(field)
                    .copied()
                    .or_else(|| c.parameters.get(field)?.assignment)
            })
            .map(|at| (class.path, at));
        let written = |owner| {
            [&writes.program, &writes.tests][..if test { 2 } else { 1 }]
                .iter()
                .any(|marks| {
                    [Some(field), None].iter().any(|&f| {
                        marks
                            .get(&(owner, f))
                            .is_some_and(|only| only.is_none() || *only != seed)
                    })
                })
        };
        if written(Owner::Class(class)) {
            return true;
        }
        if info.is_some_and(|c| c.private.iter().any(|f| f == field)) {
            return written(Owner::UnknownIn(class));
        }
        written(Owner::Unknown)
            || self.ts_any_heir(class.name, |heir| {
                written(Owner::Class(heir))
                    || self.files[heir.path]
                        .ts_classes
                        .get(heir.name)
                        .is_some_and(|c| c.initialized.contains_key(field))
            })
    }
}

impl<'a> Code<'a> {
    pub(in crate::resolve) fn field_callers(&self, target: Target<'a>) -> &[Caller<'a>] {
        let callers = if self.resolver.is_class(target) {
            &self.resolver.ts_constructions
        } else {
            &self.resolver.callers
        };
        callers.get(&target).map_or(&[], Vec::as_slice)
    }

    /// Read the argument at each exact construction, including values supplied to its factory.
    pub(in crate::resolve) fn ts_field_argument(
        &self,
        path: &'a str,
        scope: &'a str,
        field: &'a str,
        fold: &mut Fold<'a, '_>,
    ) -> Option<Value> {
        let r = self.resolver;
        let Binding::Def(class) = r.class(path, scope) else {
            return None;
        };
        let parameter = r.files[path]
            .ts_classes
            .get(class.name)?
            .parameters
            .get(field)?;
        let Some(frames) = fold.field_frames else {
            return Some(Value::Unknown);
        };
        if r.ts_any_heir(class.name, |_| true)
            || r.ts_field_written(class, field, crate::http::is_test_file(path))
        {
            return Some(Value::Unknown);
        }
        let (name, def) =
            r.defs[path].get_key_value(crate::qualify(class.name, "constructor").as_str())?;
        let ctor = Target { path, name };
        let &(n, None) = r.params[path].get(&(ctor.name, parameter.name.as_str()))? else {
            return Some(Value::Unknown);
        };
        if !self.unchanged_parameter(ctor, &parameter.name) {
            return Some(Value::Unknown);
        }
        let Some((i, frame)) = frames.iter().enumerate().find(|(_, f)| f.def == class) else {
            if self.field_callers(class).is_empty() {
                return Some(Value::Unknown);
            }
            fold.field_wants.get_or_insert(class);
            return Some(Value::Ref(format!("this.{field}")));
        };
        let call = self.call(frame.path, frame.index)?;
        let mut wants = None;
        fold.field_frames = Some(&frames[i + 1..]);
        let value = match self.arg(frame.path, frame.index, ctor, n) {
            Some(arg) => self.argument_value(
                frame.path,
                &call.scope,
                arg,
                frames,
                i + 1,
                fold,
                &mut wants,
            ),
            None => match &def.params[n].default {
                Some(default) => self.eval(path, ctor.name, default, &[], 0, fold, &mut wants),
                None => Value::Undefined,
            },
        };
        fold.field_frames = Some(frames);
        if let Some(target) = wants {
            if self.field_callers(target).is_empty() {
                return Some(super::super::unbind(value, fold.chain, &[]));
            }
            fold.field_wants.get_or_insert(target);
        }
        Some(value)
    }
}
