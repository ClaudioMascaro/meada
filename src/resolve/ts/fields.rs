use super::super::{Binding, Caller, Code, Fold, Resolver, Target, Work};
use crate::lang::{Part, Value, literal};
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

#[derive(Default)]
pub(in crate::resolve) struct Writes<'a> {
    program: Marks<'a>,
    tests: Marks<'a>,
    incomplete: [bool; 2],
    contents: [Marks<'a>; 2],
    contents_incomplete: [bool; 2],
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
            let index = usize::from(test);
            result.contents_incomplete[index] |= facts.ts_record_writes_incomplete;
            for (w, contents) in writes
                .iter()
                .map(|w| (w, false))
                .chain(facts.ts_record_writes.iter().map(|w| (w, true)))
            {
                if facts
                    .ts_local_literal_writes
                    .get(&w.object)
                    .is_some_and(|sites| sites.contains(&w.range.start))
                {
                    continue;
                }
                let marks = if contents {
                    &mut result.contents[index]
                } else if test {
                    &mut result.tests
                } else {
                    &mut result.program
                };
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
        assigned: bool,
    ) -> Option<Binding<'a>> {
        let &overwritten = self.files[path].ts_literal_methods.get(name)?.get(member)?;
        let owner = Owner::Literal(Target { path, name });
        let written = self.ts_writes.as_ref().map_or(!assigned, |writes| {
            [
                (&writes.program, writes.incomplete[0]),
                (&writes.tests, writes.incomplete[1]),
            ]
            .into_iter()
            .any(|(marks, incomplete)| {
                marks.contains_key(&(owner, Some(member)))
                    || (!assigned
                        && (incomplete
                            || marks.contains_key(&(owner, None))
                            || marks.contains_key(&(Owner::Unknown, Some(member)))
                            || marks.contains_key(&(Owner::Unknown, None))))
            })
        });
        Some(if overwritten || written {
            Binding::Unknown
        } else {
            self.def(path, &crate::qualify(name, member))
        })
    }

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
                    .or_else(|| c.records.get(field).map(|(offset, _)| *offset))
            })
            .map(|at| (class.path, at));
        let record = info.is_some_and(|c| c.records.contains_key(field));
        if record && (writes.contents_incomplete[0] || (test && writes.contents_incomplete[1])) {
            return true;
        }
        let written = |owner| {
            let sets = [
                (&writes.program, &writes.contents[0]),
                (&writes.tests, &writes.contents[1]),
            ];
            sets[..if test { 2 } else { 1 }]
                .iter()
                .any(|(marks, contents)| {
                    [Some(field), None].iter().any(|&f| {
                        marks
                            .get(&(owner, f))
                            .is_some_and(|only| only.is_none() || *only != seed)
                            || (record && contents.contains_key(&(owner, f)))
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

    #[expect(clippy::too_many_arguments)]
    pub(in crate::resolve) fn ts_record(
        &self,
        (path, scope): (&'a str, &'a str),
        (field, property): (&'a str, &str),
        key: Vec<Part>,
        frames: &[super::super::Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Option<Value> {
        let r = self.resolver;
        let Binding::Def(class) = r.class(path, scope) else {
            return None;
        };
        let field = field.strip_prefix("this.")?;
        let (_, Value::Object(entries)) = r.files[path]
            .ts_classes
            .get(class.name)?
            .records
            .get(field)?
        else {
            return None;
        };
        if r.ts_field_written(class, field, crate::http::is_test_file(path)) {
            return None;
        }
        let (ctor, _) =
            r.defs[path].get_key_value(crate::qualify(class.name, "constructor").as_str())?;
        let selected = literal(&key);
        let pending = key.iter().find_map(Part::parameter).map(str::to_owned);
        let mut seen = std::collections::HashSet::new();
        let mut values = Vec::new();
        for (name, entry) in entries.iter().rev() {
            if matches!(name.as_str(), "..." | "[]") {
                return None;
            }
            if !seen.insert(name) || selected.as_ref().is_some_and(|k| k != name) {
                continue;
            }
            let Value::Object(fields) = entry else {
                return None;
            };
            if fields
                .iter()
                .any(|(key, _)| matches!(key.as_str(), "..." | "[]"))
            {
                return None;
            }
            let (_, value) = fields.iter().rev().find(|(key, _)| key == property)?;
            match self.eval(path, ctor, value, frames, depth, fold, wants) {
                Value::Str(parts) => values.push(parts),
                Value::Ref(name) => values.push(vec![Part::Ref(name)]),
                _ => return None,
            }
        }
        if selected.is_some() {
            values.pop().map(Value::Str)
        } else if values.is_empty() {
            None
        } else {
            Some(Value::Str(vec![Part::RecordValues {
                key: pending,
                values,
            }]))
        }
    }

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
        Some(match (parameter.strip, value) {
            (Some(strip), Value::Str(parts)) => Value::Str(strip.wrap(parts)),
            (Some(strip), Value::Ref(name)) => {
                Value::Str(strip.wrap(vec![crate::lang::Part::Ref(name)]))
            }
            (Some(strip), Value::Unknown) => Value::Str(strip.wrap(vec![crate::lang::Part::Hole])),
            (_, value) => value,
        })
    }
}
