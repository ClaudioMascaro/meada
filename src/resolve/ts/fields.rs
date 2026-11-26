use super::super::{Binding, Resolver, Target, Work};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Owner<'a> {
    Class(Target<'a>),
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
                    enclosing
                } else {
                    let receiver = r.at_ts_site(path, w.range.start, || {
                        let b = r.read_member(path, &w.scope, &w.object);
                        r.instance(b, &mut Work::default())
                    });
                    match receiver {
                        Binding::Instance(t) => Some(t),
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
                if let Some(t) = owner {
                    mark(Owner::Class(t));
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
            .and_then(|c| c.constructed.get(field))
            .map(|&at| (class.path, at));
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
