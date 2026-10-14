//! The shared resolution (ADR 0007): connects each call a language pack found to the
//! definitions it reaches, with Meada's own scope and import resolution and no types.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::lang::{Call, Export, Facts, Imported, Kind, Modules};

/// A definition: the repository-relative path of its file and its qualified name.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target<'a> {
    pub path: &'a str,
    pub name: &'a str,
}

pub enum Resolution<'a> {
    /// Scope and import resolution found one definition.
    Exact(Target<'a>),
    /// Only a unique name match in the repository found it.
    Inferred(Target<'a>),
    /// Several definitions fit, sorted.
    Ambiguous(Vec<Target<'a>>),
    Unresolved,
}

/// What a name or a member chain refers to.
enum Binding<'a> {
    Def(Target<'a>),
    /// A whole file, through a namespace import or re-export.
    Module(&'a str),
    /// Something an import brings from outside the repository.
    External,
    /// Neither scope nor import resolution knows it.
    Unknown,
}

/// Seen `(file, exported name)` pairs: re-exports may form cycles.
type Seen<'a> = HashSet<(&'a str, &'a str)>;

/// The files of one language pack, resolved together.
pub struct Resolver<'a> {
    files: &'a BTreeMap<&'a str, &'a Facts>,
    defs: HashMap<&'a str, HashMap<&'a str, Kind>>,
    /// Name inference candidates, sorted: methods by their last name (`true`), top-level
    /// functions and classes by their name (`false`).
    by_name: HashMap<(bool, &'a str), Vec<Target<'a>>>,
    modules: Modules,
}

impl<'a> Resolver<'a> {
    pub fn new(files: &'a BTreeMap<&'a str, &'a Facts>, modules: Modules) -> Self {
        let defs = files
            .iter()
            .map(|(path, facts)| {
                let names = facts
                    .defs
                    .iter()
                    .map(|d| (d.name.as_str(), d.kind))
                    .collect();
                (*path, names)
            })
            .collect();
        let mut by_name: HashMap<_, Vec<Target>> = HashMap::new();
        for (path, facts) in files {
            for def in &facts.defs {
                let name = def.name.as_str();
                let key = match (def.kind, name.rsplit_once('.')) {
                    (Kind::Method, Some((_, last))) => (true, last),
                    (Kind::Function | Kind::Class, None) => (false, name),
                    _ => continue,
                };
                by_name.entry(key).or_default().push(Target { path, name });
            }
        }
        by_name.values_mut().for_each(|targets| targets.sort());
        Self {
            files,
            defs,
            by_name,
            modules,
        }
    }

    pub fn call(&self, path: &'a str, call: &'a Call) -> Resolution<'a> {
        let mut segments = call.callee.split('.');
        let head = segments.next().unwrap_or("");
        let seen = &mut Seen::new();
        let mut binding = match head {
            "this" => self.class(path, &call.scope),
            "super" => Binding::Unknown,
            _ if head.starts_with('(') => Binding::Unknown,
            _ => self.lexical(path, &call.scope, head, seen),
        };
        for member in segments {
            binding = match binding {
                Binding::Def(t) => self.def(t.path, &format!("{}.{member}", t.name)),
                Binding::Module(m) => self.export(m, member, seen),
                other => other,
            };
        }
        match binding {
            Binding::Def(t) => Resolution::Exact(t),
            Binding::External => Resolution::Unresolved,
            Binding::Module(_) | Binding::Unknown => self.infer(&call.callee),
        }
    }

    /// The definitions whose name matches the callee's last name anywhere in the repository:
    /// methods for a member call (`x.area()`), top-level functions and classes for a plain
    /// call (`area()`).
    fn infer(&self, callee: &str) -> Resolution<'a> {
        let (member, last) = match callee.rsplit_once('.') {
            Some((_, last)) => (true, last),
            None => (false, callee),
        };
        let found = self
            .by_name
            .get(&(member, last))
            .map_or(&[][..], Vec::as_slice);
        match found {
            [] => Resolution::Unresolved,
            [one] => Resolution::Inferred(*one),
            all => Resolution::Ambiguous(all.to_vec()),
        }
    }

    fn def(&self, path: &'a str, name: &str) -> Binding<'a> {
        match self.defs[path].get_key_value(name) {
            Some((name, _)) => Binding::Def(Target { path, name }),
            None => Binding::Unknown,
        }
    }

    /// The class whose body encloses `scope`, for `this`.
    fn class(&self, path: &'a str, scope: &str) -> Binding<'a> {
        let defs = &self.defs[path];
        let mut scope = scope;
        while !scope.is_empty() {
            if let Some((name, Kind::Class)) = defs.get_key_value(scope) {
                return Binding::Def(Target { path, name });
            }
            scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
        }
        Binding::Unknown
    }

    /// What `name` refers to from `scope`: the innermost enclosing definition first, then
    /// the top level, then the file's imports. Methods are not lexical bindings.
    fn lexical(
        &self,
        path: &'a str,
        scope: &str,
        name: &'a str,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        let defs = &self.defs[path];
        let mut scope = Some(scope);
        while let Some(s) = scope {
            let candidate = if s.is_empty() {
                name.to_string()
            } else {
                format!("{s}.{name}")
            };
            if let Some((name, kind)) = defs.get_key_value(candidate.as_str())
                && *kind != Kind::Method
            {
                return Binding::Def(Target { path, name });
            }
            scope = (!s.is_empty()).then(|| s.rsplit_once('.').map_or("", |(parent, _)| parent));
        }
        match self.files[path].imports.iter().find(|i| i.local == name) {
            Some(import) => self.import(path, &import.source, &import.name, seen),
            None => Binding::Unknown,
        }
    }

    fn import(
        &self,
        path: &'a str,
        source: &str,
        name: &'a Imported,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        let Some(module) = self.module(path, source) else {
            return Binding::External;
        };
        match name {
            Imported::Namespace => Binding::Module(module),
            Imported::Name(name) => self.export(module, name, seen),
        }
    }

    /// What `path` exports as `name`.
    fn export(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Binding<'a> {
        if !seen.insert((path, name)) {
            return Binding::Unknown;
        }
        let exports = &self.files[path].exports;
        for export in exports {
            match export {
                Export::Local { exported, local } if exported == name => {
                    return self.lexical(path, "", local, seen);
                }
                Export::From {
                    exported,
                    source,
                    name: imported,
                } if exported == name => return self.import(path, source, imported, seen),
                _ => {}
            }
        }
        if name != "default" {
            for export in exports {
                if let Export::All { source } = export
                    && let Some(module) = self.module(path, source)
                    && let found @ (Binding::Def(_) | Binding::Module(_)) =
                        self.export(module, name, seen)
                {
                    return found;
                }
            }
        }
        Binding::Unknown
    }

    /// The file an import of `specifier` from `path` names, if the pack parses it.
    fn module(&self, path: &str, specifier: &str) -> Option<&'a str> {
        (self.modules)(path, specifier)
            .iter()
            .find_map(|candidate| self.files.get_key_value(candidate.as_str()))
            .map(|(path, _)| *path)
    }
}
