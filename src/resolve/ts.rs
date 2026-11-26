use super::{HashMap, HashSet, Kind, Resolver, Target, own_name};

use crate::lang::{Class, Facts, TsLocal};
use std::collections::{BTreeMap, BTreeSet};

mod fields;
pub(super) use fields::Writes;

type Changes<'a> = Vec<(usize, Option<&'a TsLocal>)>;

/// Each change in the innermost visible declaration, so repeated calls in sibling blocks
/// need one binary search instead of a scan of every declaration of the name.
pub(super) struct FileLocals<'a> {
    id: usize,
    test: bool,
    changes: HashMap<(&'a str, &'a str), Changes<'a>>,
}

pub(super) fn locals<'a>(files: &BTreeMap<&'a str, &'a Facts>) -> HashMap<&'a str, FileLocals<'a>> {
    let mut result = HashMap::new();
    for (id, (&path, &facts)) in files.iter().enumerate() {
        let mut names = HashMap::new();
        for (qualified, bindings) in &facts.ts_locals {
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
                changes.push((at, active.last().map(|k| &bindings[k.2])));
            }
            names.insert((super::scope_of(qualified), own_name(qualified)), changes);
        }
        result.insert(
            path,
            FileLocals {
                id,
                test: crate::http::is_test_file(path),
                changes: names,
            },
        );
    }
    result
}

impl<'a> Resolver<'a> {
    /// None when this name has no ranged declarations at this site, Some(None) in a gap.
    pub(super) fn ts_local(
        &self,
        path: &str,
        scope: &str,
        name: &str,
    ) -> Option<Option<&'a TsLocal>> {
        let (file, at, _) = self.ts_site.get()?;
        let locals = self.ts_locals.get(path)?;
        if file != locals.id {
            return None;
        }
        let changes = locals.changes.get(&(scope, name))?;
        let n = changes.partition_point(|(position, _)| *position <= at);
        Some(n.checked_sub(1).and_then(|i| changes[i].1))
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
