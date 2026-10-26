//! The C# language pack: one source file's syntax, with no compiler, .NET SDK or MSBuild.
//! SDK-style project files say which files form a project, for the usings that apply to all of
//! them (`join`).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Node, Parser};

use super::{
    Binds, Call, Class, Def, Export, Facts, Import, Imported, Invocation, Join, Kind, LanguagePack,
    Layout, Modules, Param, Part, Receiver, Value, children, named, text,
};
use crate::dotnet::msbuild::{self, Using};
use crate::{Result, qualify};

/// How many named types may enclose a definition. Deeper bodies keep their enclosing symbol.
// ponytail: real source nests a few types; this prevents quadratic qualified names.
const DEFINITION_DEPTH: usize = 128;
/// How deep arguments are read before their value becomes unknown.
// ponytail: real expressions nest a few levels; this bounds hostile source recursion.
const VALUE_DEPTH: usize = 64;
/// How many segments of a callee are kept before it becomes an unknown expression.
// ponytail: real member paths are short; this keeps crafted chains cheap.
const PATH_DEPTH: usize = 256;
/// Enclosing-name bytes across syntax visits, before each visit's bounded metadata fanout.
// ponytail: a long namespace prefix must not multiply across a broad file without a budget.
const QUALIFICATION_BYTES: usize = 4 * 1024 * 1024;
/// How many distinct usings a project adds to each of its files (`join`), its project file's
/// and its `global using` directives together. A project past it keeps no facts in any file:
/// a partial set could bind a name one of the missing usings makes ambiguous.
// ponytail: real projects write tens; this bounds what a crafted file copies into every file.
const PROJECT_USINGS: usize = 256;

pub struct CSharp;

impl LanguagePack for CSharp {
    fn extensions(&self) -> &'static [&'static str] {
        &["cs"]
    }

    fn parse(&self, _path: &str, src: &[u8]) -> Result<Facts> {
        parse(src)
    }

    fn receiver(&self) -> Option<&'static str> {
        Some("this")
    }

    fn members_in_scope(&self) -> bool {
        true
    }

    fn unordered_globs(&self) -> bool {
        true
    }

    fn qualified_types(&self) -> bool {
        true
    }

    fn base_receiver(&self) -> Option<&'static str> {
        Some("base")
    }

    fn unordered_bases(&self) -> bool {
        true
    }

    fn implicit_members(&self) -> &'static [&'static str] {
        &[
            "Equals",
            "Finalize",
            "GetHashCode",
            "GetType",
            "MemberwiseClone",
            "ReferenceEquals",
            "ToString",
        ]
    }

    /// Every project file: an SDK-style one is a project (`join`).
    fn config_files(&self) -> &'static [&'static str] {
        &["**/*.csproj"]
    }

    fn modules(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Result<Modules> {
        Ok(self.layout(configs, files)?.modules)
    }

    /// Namespaces are Java's packages: types resolve by namespace and name across every
    /// project of the repository, and the files of one namespace share its scope. A namespace
    /// a file declares besides its first (`Facts::package`) names that file too.
    fn layout(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Result<Layout> {
        let layout = super::java::Java.layout(configs, files)?;
        // ponytail: a namespace that is one file's package and another file's second keeps the
        // package's files alone; the other file's types there bind by full name.
        let mut others: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (&path, facts) in files {
            let classes: BTreeSet<&str> = (facts.defs.iter())
                .filter(|d| d.kind == Kind::Class)
                .map(|d| d.name.as_str())
                .collect();
            let namespaces: BTreeSet<&str> = (classes.iter())
                .map(|name| crate::parent(name))
                .filter(|ns| !classes.contains(ns) && *ns != facts.package)
                .collect();
            for ns in namespaces {
                others
                    .entry(ns.to_string())
                    .or_default()
                    .push(path.to_string());
            }
        }
        let modules = layout.modules;
        Ok(Layout {
            modules: Box::new(move |from, name| match modules(from, name) {
                found if found.is_empty() => others.get(name).cloned().unwrap_or_default(),
                found => found,
            }),
            ..layout
        })
    }

    /// A project's usings bind in each of its files: those its project file adds (`<Using>`,
    /// implicit usings) and the `global using` directives of its files. A file in no SDK-style
    /// project takes its own. An alias names a namespace unless a type of the repository has
    /// its target's name. Each file's `Facts::unit` is its project, whose files alone the parts
    /// of a partial type come from.
    fn join(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Join {
        // Each project's directory (`a/b/`, empty at the root) and what its file adds.
        let projects: Vec<(String, Vec<Using>)> = (configs.iter())
            .filter(|(path, _)| path.ends_with(".csproj"))
            .filter_map(|(path, src)| {
                let dir = path
                    .rsplit_once('/')
                    .map_or(String::new(), |(d, _)| format!("{d}/"));
                Some((dir, msbuild::read(src)?))
            })
            .collect();
        // Each project's usings, by its directory, or a file's in none by its path. Their order
        // says nothing: the globs combine, and two aliases of one name are no C#.
        let mut usings: BTreeMap<String, BTreeSet<Using>> = BTreeMap::new();
        for (&path, facts) in files {
            let (unit, own) = owner(&projects, path);
            let found = (usings.entry(unit)).or_insert_with(|| own.iter().cloned().collect());
            for using in facts.project.iter().filter_map(|u| read_using(u)) {
                if found.len() > PROJECT_USINGS {
                    break;
                }
                found.insert(using);
            }
        }
        let types: BTreeSet<String> = (files.values())
            .flat_map(|f| f.defs.iter())
            .filter(|d| d.kind == Kind::Class)
            .map(|d| d.name.clone())
            .collect();
        Box::new(move |path, facts| {
            let (unit, own) = owner(&projects, path);
            let fallback: BTreeSet<Using>;
            let found = match usings.get(&unit) {
                Some(found) => found,
                None => {
                    fallback = own.iter().cloned().collect();
                    &fallback
                }
            };
            if found.len() > PROJECT_USINGS {
                *facts = Facts {
                    unbuilt: true,
                    ..Facts::default()
                };
                return;
            }
            for using in found {
                apply(using, facts);
            }
            for import in &mut facts.imports {
                if matches!(import.name, Imported::Namespace)
                    && !import.local.starts_with('@')
                    && types.contains(&import.source)
                {
                    import.name = Imported::Qualified;
                }
            }
            facts.unit = unit;
        })
    }
}

/// The project that owns `path`, by its directory, and what its project file adds; a file in no
/// project is its own, by its path, adding nothing.
// ponytail: the nearest project directory owns a file, the first project file of a directory
// with two; MSBuild items that include or exclude files are not read.
fn owner<'p>(projects: &'p [(String, Vec<Using>)], path: &str) -> (String, &'p [Using]) {
    (projects.iter().enumerate())
        .filter(|(_, (dir, _))| path.starts_with(dir.as_str()))
        .max_by_key(|(n, (dir, _))| (dir.len(), std::cmp::Reverse(*n)))
        .map_or((path.to_string(), &[][..]), |(_, (dir, own))| {
            (dir.clone(), own.as_slice())
        })
}

/// A using directive as `Facts::project` spells it: `A.B`, `static A.B` or `X = A.B`.
fn read_using(spelled: &str) -> Option<Using> {
    if let Some(ty) = spelled.strip_prefix("static ") {
        return Some(Using::Static(ty.to_string()));
    }
    match spelled.split_once(" = ") {
        Some((alias, target)) => Some(Using::Alias(alias.into(), target.into())),
        None => (!spelled.is_empty()).then(|| Using::Namespace(spelled.to_string())),
    }
}

fn spell(using: &Using) -> String {
    match using {
        Using::Namespace(ns) => ns.clone(),
        Using::Static(ty) => format!("static {ty}"),
        Using::Alias(alias, target) => format!("{alias} = {target}"),
    }
}

/// The imports a using directive writes in the file: a namespace's types on demand, a type's
/// static members and nested types, or an alias, read as a namespace until `join` finds a type
/// of its name.
fn apply(using: &Using, facts: &mut Facts) {
    match using {
        Using::Namespace(ns) => wildcard(ns, false, facts),
        Using::Static(ty) => {
            let alias = format!("@static{}", facts.imports.len());
            facts.imports.push(Import {
                local: alias.clone(),
                source: ty.clone(),
                name: Imported::Qualified,
            });
            facts.imports.push(Import {
                local: "*".into(),
                source: String::new(),
                name: Imported::Glob {
                    path: alias,
                    inside: false,
                    public: false,
                },
            });
        }
        Using::Alias(alias, target) => facts.imports.push(Import {
            local: alias.clone(),
            source: target.clone(),
            name: Imported::Namespace,
        }),
    }
}

fn wildcard(source: &str, inside: bool, facts: &mut Facts) {
    let alias = format!("@glob{}", facts.imports.len());
    facts.imports.push(Import {
        local: alias.clone(),
        source: source.to_string(),
        name: Imported::Namespace,
    });
    facts.imports.push(Import {
        local: "*".into(),
        source: String::new(),
        name: Imported::Glob {
            path: alias,
            inside,
            public: false,
        },
    });
}

struct Visit<'t> {
    node: Node<'t>,
    scope: Rc<String>,
    /// The namespace around the node.
    namespace: Rc<String>,
    interface: bool,
    visible: bool,
    depth: usize,
    /// Whether the node is outside every definition nested past `DEFINITION_DEPTH`, whose
    /// calls would read the wrong scope: theirs resolve to nothing.
    definitions: bool,
    /// The type parameters declared around the node: a type they name is no class.
    generics: Rc<Vec<String>>,
}

fn parse(src: &[u8]) -> Result<Facts> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_c_sharp::LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or("tree-sitter returned no tree")?;
    let mut facts = Facts::default();
    let mut stack = vec![Visit {
        node: tree.root_node(),
        scope: Rc::new(String::new()),
        namespace: Rc::new(String::new()),
        interface: false,
        visible: true,
        depth: 0,
        definitions: true,
        generics: Rc::new(Vec::new()),
    }];
    let mut usings = Vec::new();
    let mut namespaces = BTreeSet::new();
    let mut outside = BTreeSet::new();
    let mut qualification_bytes = QUALIFICATION_BYTES;
    while let Some(Visit {
        node,
        scope,
        namespace,
        interface,
        visible,
        depth,
        definitions,
        mut generics,
    }) = stack.pop()
    {
        let Some(remaining) = qualification_bytes.checked_sub(scope.len()) else {
            // A partial overload family could turn an ambiguous call into a false exact one.
            return Ok(Facts {
                unbuilt: true,
                ..Facts::default()
            });
        };
        qualification_bytes = remaining;
        let (mut inner, mut inner_namespace) = (scope.clone(), namespace.clone());
        let (mut in_interface, mut public, mut nested) = (interface, visible, depth);
        let defining = definitions && depth < DEFINITION_DEPTH;
        let kind = match node.kind() {
            "class_declaration"
            | "struct_declaration"
            | "record_declaration"
            | "interface_declaration" => Some(Kind::Class),
            "method_declaration" | "constructor_declaration" => Some(Kind::Method),
            "local_function_statement" => Some(Kind::Function),
            _ => None,
        };
        let modifiers = modifiers(node, src);
        // A static constructor is the type's initializer, no constructor a call names: its
        // calls are the type's.
        let initializer = node.kind() == "constructor_declaration" && modifiers.contains(&"static");
        // An explicit interface implementation (`long IClock.Now()`) is reached through the
        // interface alone: no symbol of its own, its calls the type's.
        let explicit = named(node).any(|n| n.kind() == "explicit_interface_specifier");
        if let Some(kind) = kind.filter(|_| defining && !initializer && !explicit) {
            let Some(name) = node.child_by_field_name("name") else {
                continue;
            };
            let name = text(name, src);
            let constructor = node.kind() == "constructor_declaration";
            if let Some(declared) = (named(node))
                .find(|n| n.kind() == "type_parameter_list")
                .or_else(|| node.child_by_field_name("type_parameters"))
            {
                let mut names = generics.as_ref().clone();
                names.extend(
                    (named(declared).filter(|p| p.kind() == "type_parameter"))
                        .filter_map(|p| p.child_by_field_name("name"))
                        .map(|n| text(n, src).to_string()),
                );
                generics = Rc::new(names);
            }
            let parameters = match kind {
                Kind::Class => Vec::new(),
                _ => parameters(node, src),
            };
            inner = Rc::new(match kind {
                Kind::Method => {
                    let types: Vec<&str> =
                        parameters.iter().map(|(_, ty, _)| ty.as_str()).collect();
                    qualify(&scope, &format!("{name}({})", types.join(",")))
                }
                _ => qualify(&scope, name),
            });
            let own_public = modifiers.contains(&"public")
                || (modifiers.contains(&"protected") && !modifiers.contains(&"private"))
                || (interface && !modifiers.contains(&"private"));
            public = visible && own_public && kind != Kind::Function;
            let params = (parameters.into_iter())
                .map(|(name, _, binds)| Param {
                    name,
                    binds,
                    default: None,
                    fields: Vec::new(),
                })
                .collect();
            facts.defs.push(Def {
                name: inner.as_ref().clone(),
                kind,
                range: node.byte_range(),
                params,
                bases: match kind {
                    Kind::Class => bases(node, src, &generics),
                    _ => Vec::new(),
                },
                decorators: Vec::new(),
                return_values: Vec::new(),
                returns: None,
                type_params: Vec::new(),
                result: None,
            });
            match kind {
                Kind::Method => {
                    let (families, callable) = match constructor {
                        true => (&mut facts.constructors, scope.as_ref().clone()),
                        false => (&mut facts.overloads, qualify(&scope, name)),
                    };
                    families
                        .entry(callable)
                        .or_default()
                        .push(inner.as_ref().clone());
                    if modifiers.contains(&"static") {
                        facts.statics.push(inner.as_ref().clone());
                    }
                }
                Kind::Class => {
                    // A nested type is a static member of its type (`using static` takes it).
                    if scope != namespace {
                        facts.statics.push(inner.as_ref().clone());
                    } else if facts.package.is_empty() {
                        facts.package = namespace.as_ref().clone();
                    }
                    nested += 1;
                    let relative = (inner.strip_prefix(namespace.as_str()))
                        .and_then(|r| r.strip_prefix('.'))
                        .unwrap_or(&inner);
                    for alias in [relative, inner.as_str()] {
                        facts.exports.push(Export::Local {
                            exported: alias.to_string(),
                            local: inner.as_ref().clone(),
                        });
                        if !public {
                            facts.private.push(alias.to_string());
                        }
                    }
                    if modifiers.contains(&"partial") {
                        facts.partial.push(inner.as_ref().clone());
                    }
                    in_interface = node.kind() == "interface_declaration";
                    if in_interface {
                        facts.interfaces.push(inner.as_ref().clone());
                    }
                    // A primary constructor's parameters are in scope in every member.
                    for (name, _, _) in parameters_of(node.child_by_field_name("parameters"), src)
                        .into_iter()
                        .chain(parameters_of(
                            named(node).find(|n| n.kind() == "parameter_list"),
                            src,
                        ))
                    {
                        facts.receivers.push(receiver(&inner, &name));
                    }
                }
                // A local function is a family of its own, which a bare call in its method
                // reads before the type's (`Resolver::lexical_as`).
                _ => {
                    let own = inner.as_ref().clone();
                    facts.overloads.entry(own.clone()).or_default().push(own);
                }
            }
            if !public {
                facts.hidden.push(inner.as_ref().clone());
            }
            for mut at in attributes(node, src, &scope) {
                at.decorates = Some(inner.as_ref().clone());
                facts.calls.push(at);
            }
        }
        if kind.is_some() && !defining {
            if let Some(name) = node.child_by_field_name("name") {
                facts.locals.push(qualify(&scope, text(name, src)));
            }
            for (name, _, _) in parameters(node, src) {
                facts.locals.push(qualify(&scope, &name));
            }
        }
        match node.kind() {
            "namespace_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    inner = Rc::new(qualify(&scope, &callee(name, src)));
                    inner_namespace = inner.clone();
                    namespaces.insert(inner.as_ref().clone());
                }
            }
            "file_scoped_namespace_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    namespaces.insert(callee(name, src));
                }
            }
            "using_directive" => {
                if let Some(using) = using(node, src) {
                    match children(node).any(|c| c.kind() == "global") {
                        true => facts.project.push(spell(&using)),
                        false => usings.push(using),
                    }
                }
            }
            "invocation_expression" => {
                let function = node.child_by_field_name("function");
                let bare =
                    function.is_some_and(|f| matches!(f.kind(), "identifier" | "generic_name"));
                if !(bare && function.is_some_and(|f| text(f, src) == "nameof")) {
                    let mut at = call(
                        &scope,
                        function.map_or_else(String::new, |f| callee(f, src)),
                        node.byte_range(),
                    );
                    at.by_path = bare;
                    (at.args, at.kwargs) = arguments(node.child_by_field_name("arguments"), src, 0);
                    at.arity = Some(at.args.len() + at.kwargs.len());
                    let object = function
                        .filter(|f| f.kind() == "member_access_expression")
                        .and_then(|f| f.child_by_field_name("expression"));
                    at.receiver = object.map(|n| value(n, src, 0));
                    match object {
                        Some(o)
                            if o.kind() == "identifier"
                                && text(o, src).starts_with(char::is_uppercase) =>
                        {
                            outside.insert(text(o, src).to_string());
                        }
                        Some(o) if o.kind() == "predefined_type" => {
                            outside.insert(text(o, src).to_string());
                        }
                        _ => {}
                    }
                    facts.calls.push(opaque(at, definitions));
                }
            }
            "object_creation_expression" => {
                if let Some(ty) = node.child_by_field_name("type") {
                    let mut at = call(&scope, type_name(ty, src), node.byte_range());
                    at.by_path = true;
                    at.constructs = true;
                    (at.args, at.kwargs) = arguments(node.child_by_field_name("arguments"), src, 0);
                    at.arity = Some(at.args.len() + at.kwargs.len());
                    facts.calls.push(opaque(at, definitions));
                }
            }
            // `: this(..)` calls a constructor of the type, `: base(..)` one of its base.
            "constructor_initializer" if definitions => {
                let class = crate::parent(scope.as_str());
                let callee = match children(node).find(|c| matches!(c.kind(), "this" | "base")) {
                    Some(c) if c.kind() == "this" => class.rsplit('.').next().map(str::to_string),
                    Some(_) => base_class(node, src),
                    None => None,
                };
                if let Some(callee) = callee {
                    let mut at = call(&scope, callee, node.byte_range());
                    at.by_path = true;
                    at.constructs = true;
                    let list = named(node).find(|n| n.kind() == "argument_list");
                    (at.args, at.kwargs) = arguments(list, src, 0);
                    at.arity = Some(at.args.len() + at.kwargs.len());
                    facts.calls.push(at);
                }
            }
            // A field, property or event of the type hides an outer name in its members.
            "field_declaration" | "event_field_declaration" => {
                let declarators = named(node)
                    .filter(|n| n.kind() == "variable_declaration")
                    .flat_map(named)
                    .filter(|n| n.kind() == "variable_declarator");
                for name in declarators.filter_map(|d| d.child_by_field_name("name")) {
                    facts.receivers.push(receiver(&scope, text(name, src)));
                }
            }
            "property_declaration" | "event_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    facts.receivers.push(receiver(&scope, text(name, src)));
                }
            }
            // The other bindings hide an outer name in the whole definition around them.
            "variable_declaration"
                if !node.parent().is_some_and(|p| {
                    matches!(p.kind(), "field_declaration" | "event_field_declaration")
                }) =>
            {
                for declarator in named(node).filter(|n| n.kind() == "variable_declarator") {
                    if let Some(name) = declarator.child_by_field_name("name") {
                        facts.locals.push(qualify(&scope, text(name, src)));
                    }
                }
            }
            "foreach_statement"
            | "catch_declaration"
            | "declaration_expression"
            | "declaration_pattern"
            | "tuple_pattern" => {
                let mut cursor = node.walk();
                let names: Vec<Node> = (node.children_by_field_name("name", &mut cursor))
                    .chain(node.child_by_field_name("left"))
                    .filter(|n| n.kind() == "identifier")
                    .collect();
                for name in names {
                    facts.locals.push(qualify(&scope, text(name, src)));
                }
            }
            "lambda_expression" | "anonymous_method_expression" => {
                match node.child_by_field_name("parameters") {
                    Some(p) if p.kind() == "implicit_parameter" => {
                        facts.locals.push(qualify(&scope, text(p, src)));
                    }
                    list => {
                        for (name, _, _) in parameters_of(list, src) {
                            facts.locals.push(qualify(&scope, &name));
                        }
                    }
                }
            }
            _ => {}
        }
        // A file-scoped namespace holds the declarations after it, its siblings.
        let mut child_scope = (inner.clone(), inner_namespace.clone());
        let mut visits = Vec::new();
        for child in named(node) {
            // Attributes are read as calls on the definition they decorate (`attributes`).
            if child.kind() == "attribute_list" {
                continue;
            }
            if child.kind() == "file_scoped_namespace_declaration"
                && let Some(name) = child.child_by_field_name("name")
            {
                let ns = Rc::new(qualify(&inner, &callee(name, src)));
                child_scope = (ns.clone(), ns);
            }
            visits.push(Visit {
                node: child,
                scope: child_scope.0.clone(),
                namespace: child_scope.1.clone(),
                interface: in_interface,
                visible: public,
                depth: nested,
                definitions: definitions && (kind.is_none() || defining),
                generics: generics.clone(),
            });
        }
        stack.extend(visits.into_iter().rev());
    }
    // The namespaces around the file's code see their types without a using, the innermost
    // first (an inside glob of each, the outermost written first); the usings come after.
    let enclosing: BTreeSet<&str> = (namespaces.iter())
        .flat_map(|ns| {
            (ns.match_indices('.').map(|(at, _)| &ns[..at])).chain(std::iter::once(ns.as_str()))
        })
        .collect();
    let mut enclosing: Vec<&str> = enclosing.into_iter().collect();
    enclosing.sort_by_key(|ns| (ns.matches('.').count(), *ns));
    for ns in enclosing {
        wildcard(ns, true, &mut facts);
    }
    for using in usings {
        apply(&using, &mut facts);
    }
    let mut at = BTreeMap::new();
    for receiver in std::mem::take(&mut facts.receivers) {
        at.entry((receiver.scope.clone(), receiver.name.clone()))
            .or_insert(receiver);
    }
    facts.receivers = at.into_values().collect();
    facts.externs = outside.into_iter().collect();
    // A using of the namespace takes its types, internal ones too (they are the assembly's),
    // never the names the file's own usings bind.
    facts.star = super::Star::Only(
        (facts.exports.iter())
            .filter_map(|e| match e {
                Export::Local { exported, .. } => Some(exported.clone()),
                _ => None,
            })
            .collect(),
    );
    facts.locals.sort();
    facts.locals.dedup();
    for names in facts.overloads.values_mut() {
        names.sort();
    }
    Ok(facts)
}

/// `at`, made on nothing the repository holds when written outside `definitions`.
fn opaque(mut at: Call, definitions: bool) -> Call {
    if !definitions {
        at.callee = format!("(opaque).{}", at.callee);
        at.by_path = false;
    }
    at
}

/// A name bound in `scope` whose class is not read here.
fn receiver(scope: &str, name: &str) -> Receiver {
    Receiver {
        scope: scope.to_string(),
        name: name.to_string(),
        class: Class::Unknown,
    }
}

/// The using directive `node` writes, without its `global`.
fn using(node: Node, src: &[u8]) -> Option<Using> {
    let alias = node.child_by_field_name("name");
    let target = named(node).find(|n| {
        alias.is_none_or(|a| a.id() != n.id())
            && matches!(
                n.kind(),
                "identifier" | "qualified_name" | "generic_name" | "alias_qualified_name"
            )
    })?;
    let target = type_name(target, src);
    Some(match alias {
        Some(alias) => Using::Alias(text(alias, src).to_string(), target),
        None if children(node).any(|c| c.kind() == "static") => Using::Static(target),
        None => Using::Namespace(target),
    })
}

fn modifiers<'s>(node: Node, src: &'s [u8]) -> Vec<&'s str> {
    (named(node).filter(|n| n.kind() == "modifier"))
        .map(|n| text(n, src))
        .collect()
}

/// Each parameter of the method, constructor or local function `node`: its name, its type as
/// written, and which argument binds it.
fn parameters(node: Node, src: &[u8]) -> Vec<(String, String, Binds)> {
    parameters_of(node.child_by_field_name("parameters"), src)
}

/// `parameters` of a `parameter_list`. A `params` array is written on the list itself (its
/// `type` and `name`), last.
fn parameters_of(list: Option<Node>, src: &[u8]) -> Vec<(String, String, Binds)> {
    let Some(list) = list.filter(|l| l.kind() == "parameter_list") else {
        return Vec::new();
    };
    let mut out: Vec<(String, String, Binds)> = (named(list).filter(|p| p.kind() == "parameter"))
        .map(|p| {
            let ty = p.child_by_field_name("type");
            let name = p.child_by_field_name("name");
            let defaulted = named(p).any(|n| {
                !matches!(n.kind(), "attribute_list" | "modifier")
                    && Some(n.id()) != ty.map(|t| t.id())
                    && Some(n.id()) != name.map(|t| t.id())
            });
            (
                name.map_or("", |n| text(n, src)).to_string(),
                ty.map_or_else(String::new, |t| type_spelling(t, src)),
                if defaulted {
                    Binds::Optional
                } else {
                    Binds::Any
                },
            )
        })
        .collect();
    if children(list).any(|c| c.kind() == "params")
        && let Some(name) = list.child_by_field_name("name")
    {
        let ty = list.child_by_field_name("type");
        let ty = ty.map_or_else(String::new, |t| type_spelling(t, src));
        out.push((text(name, src).to_string(), ty, Binds::Rest));
    }
    out
}

/// The attributes of the declaration `node`, each as a call written in `scope`: its name the
/// callee, its arguments positional or named (`Name = value`).
fn attributes(node: Node, src: &[u8], scope: &str) -> Vec<Call> {
    let mut out = Vec::new();
    for attribute in (named(node).filter(|n| n.kind() == "attribute_list"))
        .flat_map(named)
        .filter(|n| n.kind() == "attribute")
    {
        let Some(name) = attribute.child_by_field_name("name") else {
            continue;
        };
        let mut at = call(scope, type_name(name, src), attribute.byte_range());
        let list = named(attribute).find(|n| n.kind() == "attribute_argument_list");
        (at.args, at.kwargs) = arguments(list, src, 0);
        out.push(at);
    }
    out
}

fn call(scope: &str, callee: String, range: Range<usize>) -> Call {
    Call {
        scope: scope.to_string(),
        callee,
        arity: None,
        constructs: false,
        range,
        args: Vec::new(),
        kwargs: Vec::new(),
        decorates: None,
        by_path: false,
        root: None,
        on: None,
        receiver: None,
        arg_types: Vec::new(),
        uses: Vec::new(),
    }
}

/// The bases a type declaration writes, in order, as callee-like names; one whose class
/// cannot be read (a type parameter) stays as an unread base (`(type)`).
fn bases(node: Node, src: &[u8], generics: &[String]) -> Vec<String> {
    let list = named(node).find(|n| n.kind() == "base_list");
    (list.into_iter().flat_map(named))
        .filter(|n| n.kind() != "comment")
        .map(|n| match n.kind() {
            "primary_constructor_base_type" => n.child_by_field_name("type").unwrap_or(n),
            _ => n,
        })
        .map(|ty| {
            let name = type_name(ty, src);
            let head = name.split('.').next().unwrap_or(&name);
            match name.contains('(') || generics.iter().any(|g| g == head) {
                true => "(type)".to_string(),
                false => name,
            }
        })
        .collect()
}

/// The first base the type around `node` writes: the class a `: base(..)` calls.
fn base_class(node: Node, src: &[u8]) -> Option<String> {
    let mut at = node.parent();
    while let Some(n) = at {
        if matches!(n.kind(), "class_declaration" | "record_declaration") {
            let list = named(n).find(|c| c.kind() == "base_list")?;
            let first = named(list).find(|c| c.kind() != "comment")?;
            return Some(type_name(first, src));
        }
        at = n.parent();
    }
    None
}

/// A written type as a callee-like name, without its type arguments: `Repo` for `Repo<int>`,
/// `A.B` for `global::A.B`.
fn type_name(node: Node, src: &[u8]) -> String {
    let name = callee(node, src);
    let mut out = String::new();
    let mut angles = 0usize;
    for c in name.chars() {
        match c {
            '<' => angles += 1,
            '>' => angles = angles.saturating_sub(1),
            _ if angles == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// A type as written, its tokens joined without spaces but between words (`List<T>`,
/// `Dictionary<string,int>`, `int[]`), comments left out.
fn type_spelling(node: Node, src: &[u8]) -> String {
    let mut spelling = String::new();
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if node.is_error() || node.kind() == "comment" {
            continue;
        }
        if node.child_count() == 0 {
            let token = text(node, src);
            let word = |c: char| c.is_alphanumeric() || c == '_';
            if spelling.ends_with(word) && token.starts_with(word) {
                spelling.push(' ');
            }
            spelling.push_str(token);
        } else {
            pending.extend(children(node).collect::<Vec<_>>().into_iter().rev());
        }
    }
    spelling
}

/// The normalized callee of an expression or a name: `a.b.c` for names and member accesses,
/// with type arguments dropped (`Get` for `Get<int>`), `(<node kind>)` for another
/// expression.
fn callee(node: Node, src: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut pending = vec![node];
    while let Some(n) = pending.pop() {
        if parts.len() + pending.len() >= PATH_DEPTH {
            return format!("({})", node.kind());
        }
        match n.kind() {
            "identifier" | "predefined_type" | "this" | "base" => parts.push(text(n, src)),
            "generic_name" => match named(n).find(|c| c.kind() == "identifier") {
                Some(name) => parts.push(text(name, src)),
                None => parts.push("(generic_name)"),
            },
            "member_access_expression" => {
                pending.extend(n.child_by_field_name("name"));
                pending.extend(n.child_by_field_name("expression"));
            }
            "qualified_name" => {
                pending.extend(n.child_by_field_name("name"));
                pending.extend(n.child_by_field_name("qualifier"));
            }
            // `global::A.B` names `A.B` from the root namespace.
            "alias_qualified_name" => {
                pending.extend(n.child_by_field_name("name"));
                if let Some(alias) = n.child_by_field_name("alias")
                    && text(alias, src) != "global"
                {
                    pending.push(alias);
                }
            }
            "invocation_expression" if n.id() != node.id() => parts.push("(invocation_expression)"),
            kind if n.id() == node.id() => return format!("({kind})"),
            _ => parts.push("(expression)"),
        }
    }
    parts.join(".")
}

/// The arguments of an `argument_list` or an `attribute_argument_list`: the positional ones,
/// then the named ones (`x: 1`, `Name = "n"`).
fn arguments(list: Option<Node>, src: &[u8], depth: usize) -> (Vec<Value>, Vec<(String, Value)>) {
    let (mut args, mut kwargs) = (Vec::new(), Vec::new());
    for argument in (list.into_iter().flat_map(named))
        .filter(|n| matches!(n.kind(), "argument" | "attribute_argument"))
    {
        let name = argument.child_by_field_name("name");
        let expression = named(argument).find(|n| Some(n.id()) != name.map(|m| m.id()));
        let value = expression.map_or(Value::Unknown, |n| value(n, src, depth + 1));
        match name {
            Some(name) => kwargs.push((text(name, src).to_string(), value)),
            None => args.push(value),
        }
    }
    (args, kwargs)
}

/// An expression as written, as far as this pack folds one: string literals, names, calls and
/// constructions; the rest is unknown.
// ponytail: interpolated, raw and `+` strings are unknown until URL folding reads them.
fn value(node: Node, src: &[u8], depth: usize) -> Value {
    if depth >= VALUE_DEPTH {
        return Value::Unknown;
    }
    match node.kind() {
        "string_literal" => {
            string(node, src).map_or(Value::Unknown, |s| Value::Str(vec![Part::Text(s)]))
        }
        "verbatim_string_literal" => {
            let written = text(node, src);
            match written
                .strip_prefix("@\"")
                .and_then(|s| s.strip_suffix('"'))
            {
                Some(s) => Value::Str(vec![Part::Text(s.replace("\"\"", "\""))]),
                None => Value::Unknown,
            }
        }
        "identifier" | "member_access_expression" | "qualified_name" => {
            Value::Ref(callee(node, src))
        }
        "this" => Value::Ref("this".into()),
        "invocation_expression" => {
            let function = node.child_by_field_name("function");
            let object = function
                .filter(|f| f.kind() == "member_access_expression")
                .and_then(|f| f.child_by_field_name("expression"));
            Value::Call(Invocation {
                callee: function.map_or_else(String::new, |f| callee(f, src)),
                receiver: object.map(|n| Box::new(value(n, src, depth + 1))),
                args: arguments(node.child_by_field_name("arguments"), src, depth).0,
                range: node.byte_range(),
            })
        }
        "object_creation_expression" => {
            node.child_by_field_name("type")
                .map_or(Value::Unknown, |ty| {
                    let args = arguments(node.child_by_field_name("arguments"), src, depth).0;
                    Value::New(type_name(ty, src), args)
                })
        }
        "integer_literal" => Value::Scalar(text(node, src).parse().ok()),
        "boolean_literal" => Value::Scalar(None),
        "null_literal" => Value::Undefined,
        "parenthesized_expression" => named(node)
            .next()
            .map_or(Value::Unknown, |n| value(n, src, depth + 1)),
        _ => Value::Unknown,
    }
}

/// The text of a regular string literal, its simple escapes read; `None` with another escape.
fn string(node: Node, src: &[u8]) -> Option<String> {
    let mut out = String::new();
    for part in named(node) {
        match part.kind() {
            "string_literal_content" => out.push_str(text(part, src)),
            "escape_sequence" => out.push(match text(part, src) {
                "\\\"" => '"',
                "\\'" => '\'',
                "\\\\" => '\\',
                "\\n" => '\n',
                "\\r" => '\r',
                "\\t" => '\t',
                "\\0" => '\0',
                _ => return None,
            }),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(configs: &[(&str, &str)], files: &[(&str, &str)]) -> BTreeMap<String, Facts> {
        let mut parsed: BTreeMap<String, Facts> = (files.iter())
            .map(|(path, src)| {
                (
                    path.to_string(),
                    CSharp.parse(path, src.as_bytes()).unwrap(),
                )
            })
            .collect();
        let configs: Vec<(&str, &[u8])> = configs.iter().map(|(p, s)| (*p, s.as_bytes())).collect();
        let raw: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (p.as_str(), f)).collect();
        let join = CSharp.join(&configs, &raw);
        for (path, facts) in &mut parsed {
            join(path, facts);
        }
        parsed
    }

    fn namespaces(facts: &Facts) -> Vec<&str> {
        (facts.imports.iter())
            .filter(|i| i.local.starts_with("@glob"))
            .map(|i| i.source.as_str())
            .collect()
    }

    /// A chain of calls costs its length: each call's receiver is read to `VALUE_DEPTH` at most,
    /// not through every link below it.
    #[test]
    fn a_long_chain_of_calls_is_read_in_linear_time() {
        let source = format!("class C {{ void M() {{ x{}; }} }}", ".A()".repeat(4096));
        let start = std::time::Instant::now();
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        let took = start.elapsed();
        assert_eq!(facts.calls.len(), 4096);
        assert!(took.as_secs() < 10, "{took:?}");
    }

    #[test]
    fn implicit_usings_bind_in_an_sdk_project_that_enables_them() {
        let web = "<Project Sdk=\"Microsoft.NET.Sdk.Web\"><PropertyGroup>\
                   <ImplicitUsings>enable</ImplicitUsings></PropertyGroup></Project>";
        let off = "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup>\
                   <ImplicitUsings>disable</ImplicitUsings></PropertyGroup></Project>";
        let files = [("web/A.cs", "class A {}"), ("lib/B.cs", "class B {}")];
        let facts = joined(&[("web/Web.csproj", web), ("lib/Lib.csproj", off)], &files);
        let web = namespaces(&facts["web/A.cs"]);
        assert!(web.contains(&"System.Net.Http"), "{web:?}");
        assert!(web.contains(&"Microsoft.AspNetCore.Builder"), "{web:?}");
        assert!(namespaces(&facts["lib/B.cs"]).is_empty());
    }

    #[test]
    fn a_project_past_its_usings_keeps_no_facts() {
        let sdk = "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>";
        let many: String = (0..=PROJECT_USINGS)
            .map(|n| format!("global using N{n};\n"))
            .collect();
        let few = "global using A;\nglobal using A;\n";
        let crafted = [
            ("p/G.cs", many.as_str()),
            ("p/A.cs", "class A { void M() {} }"),
        ];
        let facts = joined(&[("p/P.csproj", sdk), ("q/Q.csproj", sdk)], &crafted);
        assert!(facts["p/A.cs"].defs.is_empty() && facts["p/A.cs"].unbuilt);
        let facts = joined(
            &[("q/Q.csproj", sdk)],
            &[("q/G.cs", few), ("q/A.cs", "class A {}")],
        );
        assert_eq!(namespaces(&facts["q/A.cs"]), ["A"]);
    }

    #[test]
    fn a_project_file_that_is_not_sdk_style_is_no_project() {
        let old = "<Project ToolsVersion=\"15.0\" \
                   xmlns=\"http://schemas.microsoft.com/developer/msbuild/2003\">\
                   <ItemGroup><Using Include=\"X\" /></ItemGroup></Project>";
        let files = [("old/A.cs", "class A {}"), ("old/G.cs", "global using Y;")];
        let facts = joined(&[("old/Old.csproj", old)], &files);
        // The file's own global using still binds in it, as in a project of its own.
        assert!(namespaces(&facts["old/A.cs"]).is_empty());
        assert_eq!(namespaces(&facts["old/G.cs"]), ["Y"]);
    }
}
