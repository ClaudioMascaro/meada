use super::{HashMap, HashSet, Kind, Resolver, Target, own_name};

use crate::lang::{Class, Facts, TsLocal};
use std::collections::{BTreeMap, BTreeSet};

mod fields;
pub(super) use fields::Writes;

type Changes<T> = Vec<(usize, Option<T>)>;

/// Each change in the innermost visible declaration, so repeated calls in sibling blocks
/// need one binary search instead of a scan of every declaration of the name.
pub(super) struct FileLocals<'a> {
    id: usize,
    test: bool,
    changes: HashMap<(&'a str, &'a str), Changes<&'a TsLocal>>,
    keys: std::cell::OnceCell<HashMap<String, Changes<TsLocal>>>,
}

fn changes<'a, T>(bindings: &'a [TsLocal], keep: impl Fn(&'a TsLocal) -> T) -> Changes<T> {
    let mut events = Vec::new();
    for (i, b) in bindings
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.block.is_empty())
    {
        events.push((b.block.start, true, i));
        events.push((b.block.end, false, i));
    }
    events.sort_unstable();
    let mut active = BTreeSet::new();
    let mut changes = Vec::new();
    for (at, enter, i) in events {
        let key = (
            bindings[i].block.start,
            std::cmp::Reverse(bindings[i].block.end),
            i,
        );
        if enter {
            active.insert(key);
        } else {
            active.remove(&key);
        }
        changes.push((at, active.last().map(|k| keep(&bindings[k.2]))));
    }
    changes
}

fn visible<T>(changes: &[(usize, Option<T>)], at: usize) -> Option<&T> {
    let n = changes.partition_point(|(position, _)| *position <= at);
    changes.get(n.checked_sub(1)?)?.1.as_ref()
}

pub(super) fn locals<'a>(files: &BTreeMap<&'a str, &'a Facts>) -> HashMap<&'a str, FileLocals<'a>> {
    files
        .iter()
        .enumerate()
        .map(|(id, (&path, &facts))| {
            (
                path,
                FileLocals {
                    id,
                    test: crate::http::is_test_file(path),
                    changes: facts
                        .ts_locals
                        .iter()
                        .map(|(name, bindings)| {
                            (
                                (super::scope_of(name), own_name(name)),
                                changes(bindings, |b| b),
                            )
                        })
                        .collect(),
                    keys: Default::default(),
                },
            )
        })
        .collect()
}

impl<'a> Resolver<'a> {
    /// None when this name has no ranged declarations at this site, Some(None) in a gap.
    pub(super) fn ts_local(&self, path: &str, scope: &str, name: &str) -> Option<Option<&TsLocal>> {
        let (file, at, _) = self.ts_site.get()?;
        let locals = self.ts_locals.get(path)?;
        if file != locals.id {
            return None;
        }
        if self.ts_key_read.get() {
            let changes = locals.keys.get()?.get(&crate::qualify(scope, name))?;
            return Some(visible(changes, at));
        }
        Some(visible(locals.changes.get(&(scope, name))?, at).copied())
    }

    pub(super) fn record_binding(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        at: usize,
        facts: &'a Facts,
    ) -> (super::Binding<'a>, Option<&TsLocal>) {
        self.ts_locals[path].keys.get_or_init(|| {
            facts
                .ts_locals
                .iter()
                .chain(&facts.ts_record_locals)
                .map(|(name, bindings)| (name.clone(), changes(bindings, TsLocal::clone)))
                .collect()
        });
        self.at_ts_site(path, at, || {
            let previous = self.ts_key_read.replace(true);
            let binding = self.read(path, scope, name);
            let local = match binding {
                super::Binding::Const(file, qualified) if file == path => self
                    .ts_local(path, super::scope_of(qualified), name)
                    .flatten(),
                _ => None,
            };
            self.ts_key_read.set(previous);
            (binding, local)
        })
    }

    pub(super) fn receiver_class(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &str,
    ) -> Option<&'a Class> {
        if let Some(Some(binding)) = self.ts_local(path, scope, name) {
            if self.ts_site.get()?.1 < binding.declared {
                return None;
            }
            return binding
                .receiver
                .and_then(|i| self.files[path].receivers.get(i))
                .map(|r| &r.class);
        }
        let class = self.receivers[path].get(&(scope, name)).copied()?;
        if self.ts
            && self.files[path]
                .ts_classes
                .get(scope)
                .is_some_and(|c| c.constructed.contains_key(name))
        {
            let test = self.ts_site.get().is_some_and(|(_, _, test)| test);
            if self.ts_field_written(Target { path, name: scope }, name, test) {
                return None;
            }
        }
        Some(class)
    }

    pub(super) fn at_ts_site<R>(&self, path: &'a str, at: usize, read: impl FnOnce() -> R) -> R {
        if !self.ts {
            return read();
        }
        let previous = self
            .ts_site
            .replace(self.ts_locals.get(path).map(|f| (f.id, at, f.test)));
        let answer = read();
        self.ts_site.set(previous);
        answer
    }

    /// Bare package specifiers may be workspace imports, so heirs are indexed by name.
    pub(super) fn index_ts_heirs(&self) -> HashMap<String, Vec<Target<'a>>> {
        let mut heirs = HashMap::<String, Vec<Target>>::new();
        for (&path, &facts) in self.files {
            for def in facts.defs.iter().filter(|d| d.kind == Kind::Class) {
                let implements = facts
                    .ts_classes
                    .get(&def.name)
                    .into_iter()
                    .flat_map(|c| &c.implements);
                for base in def.bases.iter().chain(implements) {
                    let imported = self.imported_name(path, crate::parent(&def.name), base);
                    let written = own_name(base);
                    let original = own_name(&imported);
                    let target = Target {
                        path,
                        name: &def.name,
                    };
                    heirs.entry(written.to_string()).or_default().push(target);
                    if original != written {
                        heirs.entry(original.to_string()).or_default().push(target);
                    }
                }
            }
        }
        heirs
    }

    pub(super) fn ts_field(&self, class: Target, member: &str) -> bool {
        self.files[class.path]
            .ts_classes
            .get(class.name)
            .is_some_and(|c| c.fields.iter().any(|f| f == member || f == "*"))
    }

    /// The name closure follows each adjacency list once, including cyclic heritage.
    pub(super) fn ts_any_heir(
        &self,
        name: &str,
        mut accepts: impl FnMut(Target<'a>) -> bool,
    ) -> bool {
        let mut remaining = vec![own_name(name)];
        let mut seen = HashSet::new();
        while let Some(name) = remaining.pop() {
            if !seen.insert(name) {
                continue;
            }
            for &heir in self.ts_heirs.get(name).into_iter().flatten() {
                if accepts(heir) {
                    return true;
                }
                remaining.push(own_name(heir.name));
            }
        }
        false
    }

    pub(super) fn ts_override(&self, class: Target, member: &str) -> bool {
        let name = own_name(class.name);
        if !self.ts_heirs.contains_key(name) {
            return false;
        }
        if let Some(&answer) = self
            .ts_overrides
            .borrow()
            .get(name)
            .and_then(|m| m.get(member))
        {
            return answer;
        }
        let answer = self.ts_any_heir(name, |heir| {
            self.ts_field(heir, member)
                || self.defs[heir.path]
                    .get(format!("{}.{member}", heir.name).as_str())
                    .is_some_and(|d| d.kind == Kind::Method)
        });
        self.ts_overrides
            .borrow_mut()
            .entry(name.to_owned())
            .or_default()
            .insert(member.to_owned(), answer);
        answer
    }
}

impl<'a> Resolver<'a> {
    fn ts_repository_specifier(&self, path: &str, source: &str) -> bool {
        source.starts_with('.')
            || source.starts_with('/')
            || source.starts_with('#')
            || crate::package::npm::split(source)
                .is_some_and(|(name, _)| self.ts_packages.contains(name))
            || self.module(path, source).is_some()
    }

    fn ts_annotation(&self, path: &'a str, scope: &str, class: &'a Class) -> Option<&'a str> {
        let name = match class {
            Class::TsGlobalType(name) => return Some(name),
            Class::Type(name) | Class::TsGeneric(name) => name,
            _ => return None,
        };
        let import = &self.files[path].imports[*self.by_local[path].of(name).last()?];
        if self.ts_repository_specifier(path, &import.source) {
            return None;
        }
        let mut s = scope;
        loop {
            let local = crate::qualify(s, name);
            if self.defs[path].contains_key(local.as_str())
                || self.consts[path].contains(local.as_str())
            {
                return None;
            }
            if s.is_empty() {
                break;
            }
            s = crate::parent(s);
        }
        match &import.name {
            crate::lang::Imported::Name(n) if n != "default" => Some(n),
            crate::lang::Imported::Name(_) => Some(name),
            _ => None,
        }
    }

    /// A value of a known global has no repository members; an outside annotation still
    /// admits literal methods, test doubles and classes whose heritage names that type.
    pub(super) fn ts_library_call(
        &self,
        path: &'a str,
        call: &'a crate::lang::Call,
        matching: bool,
        work: &mut super::Work<'a>,
    ) -> Option<super::Resolution<'a>> {
        use super::{Binding, Busy, Resolution, Through};
        let (head, _) = call.callee.rsplit_once('.')?;
        let span = (call.range.start, call.range.end);
        if self.files[path].ts_global_calls.contains(&span) {
            return Some(Resolution::Unresolved);
        }
        // Await may unwrap the annotated receiver to an unrelated value.
        if call.awaits {
            return None;
        }
        let head = call.root.as_deref().unwrap_or(head);
        let (p, s, n) = if let Some(field) = head.strip_prefix("this.") {
            if self.files[path].ts_rebound_this.contains(&span) {
                return None;
            }
            let Binding::Def(class) = self.class(path, &call.scope) else {
                return None;
            };
            (class.path, class.name, field)
        } else {
            if head.contains('.') || head.starts_with('(') {
                return None;
            }
            self.holder(&self.lexical(path, &call.scope, head, &mut self.seen(path, &call.scope)))?
        };
        let class = self.receiver_class(p, s, n)?;
        let facts = self.files[p];
        let declaration = self
            .ts_local(p, s, n)
            .flatten()
            .map(|local| local.declared)
            .or_else(|| facts.ts_classes.get(s)?.initialized.get(n).copied());
        // A literal supplies its own implementation even when its annotation is from a library.
        if declaration.is_some_and(|at| facts.ts_annotated_literals.contains(&at))
            || (s.is_empty() && declaration.is_none() && facts.ts_literal_methods.contains_key(n))
        {
            return None;
        }
        let ty = match class {
            Class::TsGlobalValue => return Some(Resolution::Unresolved),
            // Only the by-name answer can change for an outside result. The exact lookup
            // of a factory must not recursively repeat the candidate checks for its inputs.
            Class::Result(_) if !matching => return None,
            Class::Result(i) => {
                let made = self.files[p].calls.get(*i)?;
                if !self.enter(Busy::Call(p, *i), work) {
                    return None;
                }
                let target = self.resolved(p, made, false, work);
                self.leave(work);
                let Resolution::Exact(target) = target else {
                    return None;
                };
                let result = self.written_result(
                    target,
                    Through {
                        awaited: made.awaited,
                        ..Through::default()
                    },
                )?;
                self.ts_annotation(target.path, target.name, result)?
            }
            _ => self.ts_annotation(p, s, class)?,
        };
        if !matching {
            return Some(Resolution::Unresolved);
        }
        let mut all = self.candidates(path, &call.callee, call.by_path, None, false, &|| false);
        let mut heirs = HashSet::new();
        self.ts_any_heir(ty, |heir| {
            heirs.insert(heir);
            false
        });
        all.retain(|candidate| {
            if crate::http::is_test_file(candidate.path) {
                return true;
            }
            let owner = Target {
                path: candidate.path,
                name: crate::parent(candidate.name),
            };
            !self.defs[owner.path]
                .get(owner.name)
                .is_some_and(|d| d.kind == Kind::Class)
                || heirs.contains(&owner)
        });
        if let Some(literals) = self.ts_literal_candidates.get(own_name(&call.callee)) {
            all.extend_from_slice(literals);
            all.sort_unstable();
            all.dedup();
        }
        Some(super::one_of(all))
    }
}
