use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use tree_sitter::{Node, Parser};

use super::{
    Binds, Bound, Call, Class, Const, Def, Export, Facts, Import, Imported, Invocation, Join, Kind,
    LanguagePack, Layout, Modules, Param, Part, Receiver, Step, Ty, Value, children, named, text,
};
use crate::dotnet::msbuild::{self, Using};
use crate::dotnet::references;
use crate::{Result, qualify};

// ponytail: real source nests a few types; this prevents quadratic qualified names.
const DEFINITION_DEPTH: usize = 128;
// ponytail: real expressions nest a few levels; this bounds hostile source recursion.
const VALUE_DEPTH: usize = 64;
// ponytail: real member paths are short; this keeps crafted chains cheap.
const PATH_DEPTH: usize = 256;
// ponytail: a long namespace prefix must not multiply across a broad file without a budget.
const QUALIFICATION_BYTES: usize = 4 * 1024 * 1024;
// ponytail: a flat 256 bytes; a file under it keeps up to 256 bytes of scope per stored call,
// about 130 MB of facts at `MAX_BYTES`. Lower it if that shows up.
const QUALIFICATION_FREE: usize = 256;
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

    fn static_imports_inherit(&self) -> bool {
        false
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

    fn extension_methods(&self) -> bool {
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

    fn known_outside(&self) -> super::KnownOutside {
        &[
            ("System.IDisposable", &["Dispose"]),
            ("System.IAsyncDisposable", &["DisposeAsync"]),
            ("System.IEquatable", &["Equals"]),
            ("System.IComparable", &["CompareTo"]),
        ]
    }

    fn config_files(&self) -> &'static [&'static str] {
        &[
            "**/*.csproj",
            "**/Directory.Build.props",
            "**/appsettings.json",
        ]
    }

    fn modules(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Result<Modules> {
        Ok(self.layout(configs, files)?.modules)
    }

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
        let others: BTreeMap<String, Rc<[String]>> = others
            .into_iter()
            .map(|(ns, paths)| (ns, paths.into()))
            .collect();
        let modules = layout.modules;
        Ok(Layout {
            modules: Box::new(move |from, name| match modules(from, name) {
                found if found.is_empty() => others.get(name).cloned().unwrap_or_default(),
                found => found,
            }),
            ..layout
        })
    }

    fn join(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Join {
        let props: HashMap<&str, Option<msbuild::Props>> = (configs.iter())
            .filter(|(path, _)| msbuild::is_props(path))
            .map(|(path, src)| (*path, msbuild::props(path, src)))
            .collect();
        let read: Vec<(&str, String, msbuild::Project)> = (configs.iter())
            .filter(|(path, _)| path.ends_with(".csproj"))
            .filter_map(|(path, src)| {
                let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
                let imports = |p: &str| props.get(p).map(|read| read.as_ref()?.import.clone());
                let above = msbuild::chain(dir, imports);
                let above: Vec<&msbuild::Props> = (above.iter())
                    .filter_map(|p| props[p.as_str()].as_ref())
                    .collect();
                let dir = if dir.is_empty() {
                    String::new()
                } else {
                    format!("{dir}/")
                };
                Some((*path, dir, msbuild::read(src, &above)?))
            })
            .collect();
        let projects: Vec<(String, Vec<Using>)> = (read.iter())
            .map(|(_, dir, project)| (dir.clone(), project.usings.clone()))
            .collect();
        let unit_references = unit_references(&read);
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
                    usings: Some(PROJECT_USINGS),
                    ..Facts::default()
                };
                return;
            }
            facts.usings = None;
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
            facts.unit_references = unit_references.get(&unit).cloned().unwrap_or_default();
            facts.unit = unit;
        })
    }
}

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

fn unit_references(read: &[(&str, String, msbuild::Project)]) -> BTreeMap<String, Vec<String>> {
    let units: HashMap<&str, &str> = (read.iter())
        .map(|(path, dir, _)| (*path, dir.as_str()))
        .collect();
    let located: HashMap<&str, Vec<(String, bool)>> = (read.iter())
        .map(|(path, dir, project)| {
            let from = dir.trim_end_matches('/');
            let to = (project.projects.iter())
                .filter_map(|r| Some((references::located(from, &r.include)?, r.private)));
            (*path, to.collect())
        })
        .collect();
    let mut out = BTreeMap::new();
    for (path, unit, _) in read {
        let walked = references::referenced(path, |from| {
            (located.get(from).into_iter().flatten())
                .map(|(to, private)| (to.as_str(), *private))
                .collect()
        });
        let mut found: Vec<String> = Vec::new();
        for to in walked {
            let Some(&dir) = units.get(to) else { continue };
            if !dir.is_empty() && dir != unit && !found.iter().any(|f| f == dir) {
                found.push(dir.to_string());
            }
        }
        out.entry(unit.clone()).or_insert(found);
    }
    out
}

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

fn apply(using: &Using, facts: &mut Facts) {
    match using {
        Using::Namespace(ns) => wildcard(ns, None, facts),
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

fn wildcard(source: &str, scope: Option<&str>, facts: &mut Facts) {
    let alias = format!("@glob{}", facts.imports.len());
    facts.imports.push(Import {
        local: alias.clone(),
        source: source.to_string(),
        name: Imported::Namespace,
    });
    facts.imports.push(Import {
        local: qualify(scope.unwrap_or(""), "*"),
        source: String::new(),
        name: Imported::Glob {
            path: alias,
            inside: scope.is_some(),
            public: false,
        },
    });
}

const MAIN: &str = "Main";

type Write = (String, bool, Option<Value>);

struct Visit<'t> {
    node: Node<'t>,
    scope: Rc<String>,
    namespace: Rc<String>,
    interface: bool,
    visible: bool,
    depth: usize,
    definitions: bool,
    generics: Rc<Vec<String>>,
    conditional: bool,
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
        conditional: false,
    }];
    let mut usings = Vec::new();
    let mut namespaces = BTreeSet::new();
    let mut outside = BTreeSet::new();
    let mut fields = BTreeSet::new();
    let mut locals: Vec<Receiver> = Vec::new();
    let mut classes = BTreeSet::new();
    let mut bound: Vec<Bound> = Vec::new();
    let mut assigned: Vec<(String, String, bool, super::QueryWrite)> = Vec::new();
    let mut members: Vec<(String, String, bool, Value)> = Vec::new();
    let mut readonly: Vec<(String, String)> = Vec::new();
    let mut writes: BTreeMap<String, Vec<Write>> = BTreeMap::new();
    let mut results: Vec<Range<usize>> = Vec::new();
    let mut chains: Vec<(usize, Range<usize>)> = Vec::new();
    let mut returned: BTreeMap<String, Vec<Value>> = BTreeMap::new();
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
        conditional,
    }) = stack.pop()
    {
        let excess = scope.len().saturating_sub(QUALIFICATION_FREE);
        let Some(remaining) = qualification_bytes.checked_sub(excess) else {
            return Ok(Facts {
                budget: Some(QUALIFICATION_BYTES),
                ..Facts::default()
            });
        };
        qualification_bytes = remaining;
        SCOPE.with_borrow_mut(|s| *s = (scope.clone(), None));
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
        let initializer = node.kind() == "constructor_declaration" && modifiers.contains(&"static");
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
                    label: None,
                    default: None,
                    fields: Vec::new(),
                    closure: Default::default(),
                })
                .collect();
            facts.defs.push(Def {
                asynchronous: modifiers.contains(&"async"),
                params,
                bases: match kind {
                    Kind::Class => bases(node, src, &generics),
                    _ => Vec::new(),
                },
                result: match (kind, constructor) {
                    (Kind::Method | Kind::Function, false) => (node.child_by_field_name("returns"))
                        .or_else(|| node.child_by_field_name("type"))
                        .map(|t| class_of(t, src, &generics)),
                    _ => None,
                }
                .filter(|c| matches!(c, Class::Type(_))),
                ..Def::new(inner.as_ref().clone(), kind, node.byte_range())
            });
            if kind != Kind::Class {
                let list = node.child_by_field_name("parameters");
                for (name, class) in parameter_classes(list, src, &generics) {
                    facts.receivers.push(Receiver {
                        scope: inner.as_ref().clone(),
                        name,
                        class,
                    });
                }
            }
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
                        // ponytail: the compiler takes `Main` returning `void`, `int` or a `Task`
                        // of them, with no parameter or a `string[]`, and ignores one beside
                        // top-level statements (CS7022); a signature it rejects (CS0028) still
                        // counts here.
                        if !constructor && name == "Main" {
                            facts.programs.push(inner.as_ref().clone());
                        }
                    }
                }
                Kind::Class => {
                    classes.insert(inner.as_ref().clone());
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
                    let lists = [
                        node.child_by_field_name("parameters"),
                        named(node).find(|n| n.kind() == "parameter_list"),
                    ];
                    for (name, class) in lists
                        .into_iter()
                        .flat_map(|list| parameter_classes(list, src, &generics))
                    {
                        fields.insert(name.clone());
                        facts.receivers.push(Receiver {
                            scope: inner.as_ref().clone(),
                            name,
                            class,
                        });
                    }
                }
                _ => {
                    let own = inner.as_ref().clone();
                    facts.overloads.entry(own.clone()).or_default().push(own);
                }
            }
            if !public {
                facts.hidden.push(inner.as_ref().clone());
                if kind == Kind::Method {
                    facts.private.push(inner.as_ref().clone());
                }
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
                    let list = node.child_by_field_name("arguments");
                    (at.args, at.kwargs) = arguments(list, src, 0);
                    if inline(defining, &scope, &classes) {
                        lambdas(list, &scope, &mut at.args, &mut at.kwargs);
                    }
                    at.arity = Some(at.args.len() + at.kwargs.len());
                    let generic = match function {
                        Some(f) if f.kind() == "member_access_expression" => {
                            f.child_by_field_name("name")
                        }
                        f => f,
                    };
                    if let Some(list) = (generic.filter(|g| g.kind() == "generic_name"))
                        .and_then(|g| named(g).find(|n| n.kind() == "type_argument_list"))
                    {
                        let tys = named(list).map(|t| Ty::Named(type_name(t, src))).collect();
                        at.uses = vec![vec![Step::Argument(tys)]];
                    }
                    let object = match function {
                        Some(f) if f.kind() == "member_access_expression" => {
                            f.child_by_field_name("expression")
                        }
                        Some(f) if f.kind() == "conditional_access_expression" => condition(f),
                        _ => None,
                    };
                    at.receiver = object.map(|n| value(n, src, 0));
                    let mut inner = function.and_then(receiver_of);
                    while let Some(n) = inner.filter(|n| receiver_of(*n).is_some()) {
                        inner = receiver_of(n);
                    }
                    if let Some(inner) = inner.filter(|n| n.kind() == "invocation_expression")
                        && definitions
                        && at.callee.starts_with("(invocation_expression).")
                    {
                        chains.push((facts.calls.len(), inner.byte_range()));
                    }
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
            // ponytail: a target-typed `new(..)` passed, returned or assigned is no call: its
            // type is the parameter's, the return's or the name's, which the pack does not read.
            "implicit_object_creation_expression" => {
                let declared = match node.parent() {
                    Some(d) if d.kind() == "variable_declarator" => (d.parent())
                        .filter(|p| p.kind() == "variable_declaration")
                        .and_then(|p| p.child_by_field_name("type")),
                    Some(p) if p.kind() == "property_declaration" => p.child_by_field_name("type"),
                    _ => None,
                };
                if let Some(ty) = declared.filter(|t| t.kind() != "implicit_type") {
                    let mut at = call(&scope, type_name(ty, src), node.byte_range());
                    at.by_path = true;
                    at.constructs = true;
                    let list = named(node).find(|n| n.kind() == "argument_list");
                    (at.args, at.kwargs) = arguments(list, src, 0);
                    at.arity = Some(at.args.len() + at.kwargs.len());
                    facts.calls.push(opaque(at, definitions));
                }
            }
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
            "field_declaration" | "event_field_declaration" => {
                let declarators = named(node)
                    .filter(|n| n.kind() == "variable_declaration")
                    .flat_map(named)
                    .filter(|n| n.kind() == "variable_declarator");
                let declared = (node.kind() == "field_declaration").then(|| {
                    (named(node).find(|n| n.kind() == "variable_declaration"))
                        .and_then(|d| d.child_by_field_name("type"))
                });
                let class = match declared.flatten() {
                    Some(ty) if definitions => class_of(ty, src, &generics),
                    _ => Class::Unknown,
                };
                // A `const` or `static readonly` field is a constant of its type: what a name
                let constant = definitions
                    && node.kind() == "field_declaration"
                    && (modifiers.contains(&"const")
                        || (modifiers.contains(&"static") && modifiers.contains(&"readonly")));
                for declarator in declarators.filter(|d| d.child_by_field_name("name").is_some()) {
                    let Some(name) = declarator.child_by_field_name("name") else {
                        continue;
                    };
                    fields.insert(text(name, src).to_string());
                    facts.receivers.push(Receiver {
                        scope: scope.as_ref().clone(),
                        name: text(name, src).to_string(),
                        class: class.clone(),
                    });
                    let initial = named(declarator).find(|n| n.id() != name.id());
                    if constant {
                        let name = qualify(&scope, text(name, src));
                        facts.statics.push(name.clone());
                        facts.consts.push(Const {
                            name,
                            value: initial.map_or(Value::Unknown, |n| value(n, src, 0)),
                            ty: None,
                            scope: None,
                        });
                    } else if definitions
                        && initial.is_none()
                        && modifiers.contains(&"readonly")
                        && !modifiers.contains(&"static")
                    {
                        readonly.push((scope.as_ref().clone(), text(name, src).to_string()));
                    } else if let Some(v) = initial.filter(|n| definitions && held(n)) {
                        bound.push(Bound {
                            scope: scope.as_ref().clone(),
                            within: scope.as_ref().clone(),
                            name: text(name, src).to_string(),
                            value: value(v, src, 0),
                        });
                    }
                }
            }
            "assignment_expression"
                if !node
                    .parent()
                    .is_some_and(|p| p.kind() == "initializer_expression") =>
            {
                let left = node.child_by_field_name("left");
                let right = node.child_by_field_name("right");
                let targets = left.into_iter().flat_map(|l| match l.kind() {
                    "tuple_expression" => named(l).flat_map(named).collect(),
                    _ => vec![l],
                });
                let plain = node
                    .child_by_field_name("operator")
                    .is_some_and(|o| text(o, src) == "=");
                let one = plain && left.is_some_and(|l| l.kind() != "tuple_expression");
                let given = right.filter(|_| one).map(|r| value(r, src, 0));
                for target in targets {
                    let (name, this) = match target.kind() {
                        "identifier" => (target, false),
                        "member_access_expression"
                            if target
                                .child_by_field_name("expression")
                                .is_some_and(|e| text(e, src) == "this") =>
                        {
                            match target.child_by_field_name("name") {
                                Some(name) => (name, true),
                                None => continue,
                            }
                        }
                        _ => continue,
                    };
                    let operator = node
                        .child_by_field_name("operator")
                        .map_or("", |op| text(op, src));
                    let change = if this || left.is_none_or(|l| l.kind() != "identifier") {
                        super::QueryWrite::Other
                    } else if let Some(given) = &given {
                        super::QueryWrite::of(given, text(name, src), operator)
                    } else {
                        right.map_or(super::QueryWrite::Other, |r| {
                            super::QueryWrite::of(&value(r, src, 0), text(name, src), operator)
                        })
                    };
                    assigned.push((
                        scope.as_ref().clone(),
                        text(name, src).to_string(),
                        this,
                        change,
                    ));
                    let at = (scope.as_ref().clone(), this, given.clone());
                    writes
                        .entry(text(name, src).to_string())
                        .or_default()
                        .push(at);
                    if definitions {
                        let held = given.clone().filter(|_| right.is_some_and(|r| held(&r)));
                        members.push((
                            scope.as_ref().clone(),
                            format!("{}=", text(name, src)),
                            this,
                            held.unwrap_or(Value::Unknown),
                        ));
                    }
                }
                if let (Some(left), Some(right), true) = (left, right, definitions)
                    && matches!(left.kind(), "member_access_expression" | "identifier")
                {
                    let path = callee(left, src);
                    let rooted = path.strip_prefix("this.");
                    let rest = rooted.unwrap_or(&path);
                    if !path.contains('(') && rest.contains('.') {
                        let value = match !plain || conditional {
                            true => Value::Unknown,
                            false => value(right, src, 0),
                        };
                        members.push((
                            scope.as_ref().clone(),
                            rest.to_string(),
                            rooted.is_some(),
                            value,
                        ));
                    }
                }
                let attaches = node
                    .child_by_field_name("operator")
                    .is_some_and(|o| text(o, src) == "+=");
                if let (Some(left), Some(right), true, true) = (left, right, attaches, definitions)
                    && left.kind() == "member_access_expression"
                {
                    let path = callee(left, src);
                    let rooted = path.strip_prefix("this.");
                    let rest = rooted.unwrap_or(&path);
                    let handler = match right.kind() {
                        "lambda_expression" | "anonymous_method_expression" => {
                            let def = qualify(&scope, &format!("@{}", right.start_byte()));
                            inline(defining, &scope, &classes).then_some(Value::Function(def))
                        }
                        "identifier" | "member_access_expression" => Some(value(right, src, 0)),
                        _ => None,
                    };
                    if let Some(handler) = handler
                        && rest.contains('.')
                        && !path.contains('(')
                    {
                        members.push((
                            scope.as_ref().clone(),
                            format!("{rest}+="),
                            rooted.is_some(),
                            handler,
                        ));
                    }
                }
            }
            "postfix_unary_expression" | "prefix_unary_expression"
                if children(node).any(|c| matches!(c.kind(), "++" | "--")) =>
            {
                for target in named(node).filter(|t| t.kind() == "identifier") {
                    assigned.push((
                        scope.as_ref().clone(),
                        text(target, src).to_string(),
                        false,
                        super::QueryWrite::Other,
                    ));
                    let at = (scope.as_ref().clone(), false, None);
                    writes
                        .entry(text(target, src).to_string())
                        .or_default()
                        .push(at);
                    if definitions {
                        let name = format!("{}=", text(target, src));
                        members.push((scope.as_ref().clone(), name, false, Value::Unknown));
                    }
                }
            }
            "argument" if children(node).any(|c| matches!(c.kind(), "out" | "ref")) => {
                let this = |t: &Node| {
                    t.kind() == "member_access_expression"
                        && (t.child_by_field_name("expression"))
                            .is_some_and(|e| text(e, src) == "this")
                };
                let targets = named(node).filter_map(|t| match t.kind() {
                    "identifier" => Some((t, false)),
                    _ if this(&t) => Some((t.child_by_field_name("name")?, true)),
                    _ => None,
                });
                for (target, this) in targets {
                    assigned.push((
                        scope.as_ref().clone(),
                        text(target, src).to_string(),
                        this,
                        super::QueryWrite::Other,
                    ));
                    let at = (scope.as_ref().clone(), this, None);
                    writes
                        .entry(text(target, src).to_string())
                        .or_default()
                        .push(at);
                    if definitions {
                        let name = format!("{}=", text(target, src));
                        members.push((scope.as_ref().clone(), name, this, Value::Unknown));
                    }
                }
            }
            "property_declaration" | "event_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let class = match node.child_by_field_name("type") {
                        Some(ty) if definitions && node.kind() == "property_declaration" => {
                            class_of(ty, src, &generics)
                        }
                        _ => Class::Unknown,
                    };
                    fields.insert(text(name, src).to_string());
                    facts.receivers.push(Receiver {
                        scope: scope.as_ref().clone(),
                        name: text(name, src).to_string(),
                        class,
                    });
                    let initial = node.child_by_field_name("value");
                    if let Some(v) = initial.filter(|n| definitions && held(n)) {
                        bound.push(Bound {
                            scope: scope.as_ref().clone(),
                            within: scope.as_ref().clone(),
                            name: text(name, src).to_string(),
                            value: value(v, src, 0),
                        });
                    }
                }
            }
            "variable_declaration"
                if !node.parent().is_some_and(|p| {
                    matches!(p.kind(), "field_declaration" | "event_field_declaration")
                }) =>
            {
                let declared = node.child_by_field_name("type");
                let initializer = classes.contains(scope.as_str());
                for declarator in named(node).filter(|n| n.kind() == "variable_declarator") {
                    let Some(name) = declarator.child_by_field_name("name") else {
                        continue;
                    };
                    let value = named(declarator).find(|n| n.id() != name.id());
                    let class = match declared {
                        _ if initializer || !definitions => Class::Unknown,
                        Some(ty) if ty.kind() == "implicit_type" => match value {
                            Some(v) if v.kind() == "object_creation_expression" => v
                                .child_by_field_name("type")
                                .map_or(Class::Unknown, |t| class_of(t, src, &generics)),
                            Some(v) if v.kind() == "invocation_expression" => {
                                results.push(v.byte_range());
                                Class::Result(results.len() - 1)
                            }
                            _ => Class::Unknown,
                        },
                        Some(ty) => class_of(ty, src, &generics),
                        None => Class::Unknown,
                    };
                    if definitions && !initializer {
                        bound.push(Bound {
                            scope: scope.as_ref().clone(),
                            within: scope.as_ref().clone(),
                            name: text(name, src).to_string(),
                            value: value.map_or(Value::Unknown, |v| self::value(v, src, 0)),
                        });
                    }
                    locals.push(Receiver {
                        scope: scope.as_ref().clone(),
                        name: text(name, src).to_string(),
                        class,
                    });
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
                let class = match node.child_by_field_name("type") {
                    Some(ty) if definitions && !classes.contains(scope.as_str()) => {
                        class_of(ty, src, &generics)
                    }
                    _ => Class::Unknown,
                };
                for name in names {
                    bind_unknown(
                        &mut bound,
                        definitions && !classes.contains(scope.as_str()),
                        &scope,
                        text(name, src),
                    );
                    locals.push(Receiver {
                        scope: scope.as_ref().clone(),
                        name: text(name, src).to_string(),
                        class: class.clone(),
                    });
                }
            }
            "lambda_expression" | "anonymous_method_expression" => {
                let own = inline(defining, &scope, &classes);
                if own {
                    inner = Rc::new(qualify(&scope, &format!("@{}", node.start_byte())));
                    facts.defs.push(Def::new(
                        inner.as_ref().clone(),
                        Kind::Inline,
                        node.byte_range(),
                    ));
                    nested += 1;
                }
                let unknown = |name: &str| Receiver {
                    scope: inner.as_ref().clone(),
                    name: name.to_string(),
                    class: Class::Unknown,
                };
                match node.child_by_field_name("parameters") {
                    Some(p) if p.kind() == "implicit_parameter" => {
                        bind_unknown(&mut bound, definitions, &inner, text(p, src));
                        locals.push(unknown(text(p, src)));
                    }
                    list => {
                        for (name, class) in parameter_classes(list, src, &generics) {
                            bind_unknown(&mut bound, definitions, &inner, &name);
                            let receiver = Receiver {
                                scope: inner.as_ref().clone(),
                                name,
                                class,
                            };
                            match own {
                                true => facts.receivers.push(receiver),
                                false => locals.push(Receiver {
                                    class: Class::Unknown,
                                    ..receiver
                                }),
                            }
                        }
                    }
                }
            }
            "return_statement" if defining => {
                if let Some(v) = named(node).next() {
                    let values = returned.entry(scope.as_ref().clone()).or_default();
                    values.push(value(v, src, 0));
                }
            }
            "arrow_expression_clause"
                if defining
                    && node.parent().is_some_and(|p| {
                        matches!(p.kind(), "method_declaration" | "local_function_statement")
                    }) =>
            {
                if let Some(v) = named(node).next() {
                    let values = returned.entry(scope.as_ref().clone()).or_default();
                    values.push(value(v, src, 0));
                }
            }
            _ => {}
        }
        let mut child_scope = (inner.clone(), inner_namespace.clone());
        let mut visits = Vec::new();
        // ponytail: a file that also declares a type `Main` in the global namespace gets none,
        // since the two would share the name; its top-level statements are no program.
        let top = match node.kind() == "compilation_unit" && definitions {
            true => (named(node).filter(|c| c.kind() == "global_statement"))
                .map(|c| c.byte_range())
                .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end))
                .filter(|_| !declares_main(node, src)),
            false => None,
        };
        if let Some(range) = &top {
            facts
                .defs
                .push(Def::new(MAIN.to_string(), Kind::Function, range.clone()));
            facts.programs.push(MAIN.to_string());
        }
        let starts = kind.is_some()
            || matches!(
                node.kind(),
                "lambda_expression" | "anonymous_method_expression"
            );
        let branches = match node.kind() {
            "if_statement"
            | "conditional_expression"
            | "switch_statement"
            | "switch_expression"
            | "while_statement"
            | "do_statement"
            | "for_statement"
            | "foreach_statement"
            | "catch_clause"
            | "conditional_access_expression" => true,
            "binary_expression" => (node.child_by_field_name("operator"))
                .is_some_and(|o| matches!(text(o, src), "&&" | "||" | "??")),
            _ => false,
        };
        for child in named(node) {
            if child.kind() == "attribute_list" {
                continue;
            }
            if child.kind() == "file_scoped_namespace_declaration"
                && let Some(name) = child.child_by_field_name("name")
            {
                let ns = Rc::new(qualify(&inner, &callee(name, src)));
                child_scope = (ns.clone(), ns);
            }
            let main = top.is_some() && child.kind() == "global_statement";
            visits.push(Visit {
                node: child,
                scope: match main {
                    true => Rc::new(MAIN.to_string()),
                    false => child_scope.0.clone(),
                },
                namespace: child_scope.1.clone(),
                interface: in_interface,
                visible: public,
                depth: nested,
                definitions: definitions && (kind.is_none() || defining),
                generics: generics.clone(),
                conditional: !starts && (conditional || branches),
            });
        }
        stack.extend(visits.into_iter().rev());
    }
    let enclosing: BTreeSet<&str> = (namespaces.iter())
        .flat_map(|ns| {
            (ns.match_indices('.').map(|(at, _)| &ns[..at])).chain(std::iter::once(ns.as_str()))
        })
        .collect();
    for ns in enclosing {
        wildcard(ns, Some(ns), &mut facts);
    }
    for using in usings {
        apply(&using, &mut facts);
    }
    let called: BTreeMap<(usize, usize), usize> = (facts.calls.iter().enumerate())
        .filter(|(_, c)| c.decorates.is_none())
        .map(|(n, c)| ((c.range.start, c.range.end), n))
        .collect();
    let call_at = |r: &Range<usize>| called.get(&(r.start, r.end)).copied();
    for (outer, inner) in chains {
        facts.calls[outer].on = call_at(&inner);
    }
    for mut local in locals {
        if let Class::Result(n) = local.class {
            local.class = call_at(&results[n]).map_or(Class::Unknown, Class::Result);
        }
        if fields.contains(&local.name) {
            local.class = Class::Unknown;
        }
        facts.receivers.push(local);
    }
    let mut at = BTreeMap::new();
    let mut receivers: Vec<Receiver> = Vec::new();
    for receiver in std::mem::take(&mut facts.receivers) {
        match at.get(&(receiver.scope.clone(), receiver.name.clone())) {
            Some(&n) => {
                let kept: &mut Receiver = &mut receivers[n];
                if kept.class != receiver.class {
                    kept.class = Class::Unknown;
                }
            }
            None => {
                at.insert(
                    (receiver.scope.clone(), receiver.name.clone()),
                    receivers.len(),
                );
                receivers.push(receiver);
            }
        }
    }
    facts.receivers = receivers;
    for def in &mut facts.defs {
        if def.kind != Kind::Class
            && let Some(values) = returned.remove(&def.name)
        {
            def.return_values = values;
        }
    }
    let bindings: BTreeSet<(&str, &str)> = (facts.receivers.iter())
        .map(|r| (r.scope.as_str(), r.name.as_str()))
        .chain(
            facts
                .defs
                .iter()
                .flat_map(|d| d.params.iter().map(|p| (d.name.as_str(), p.name.as_str()))),
        )
        .collect();
    let mut reassignments = BTreeMap::new();
    for (scope, name, this, change) in assigned {
        let mut at = scope.as_str();
        while !at.is_empty() {
            if (this && classes.contains(at)) || (!this && bindings.contains(&(at, name.as_str())))
            {
                break;
            }
            at = crate::parent(at);
        }
        reassignments
            .entry((at.to_owned(), name))
            .and_modify(|old: &mut super::QueryWrite| *old = old.merge(change))
            .or_insert(change);
    }
    let mut through: Vec<Bound> = Vec::new();
    let mut member_at: HashMap<(String, String), usize> = HashMap::new();
    for (scope, path, this, value) in members {
        let head = path
            .split('.')
            .next()
            .unwrap_or(&path)
            .trim_end_matches('=');
        let mut within = scope.as_str();
        let mut declared = None;
        while !this && declared.is_none() {
            if bindings.contains(&(within, head)) {
                declared = Some(within);
            }
            if within.is_empty() {
                break;
            }
            within = crate::parent(within);
        }
        let at = match (this, declared) {
            (false, Some(s)) => s.to_string(),
            (true, _) => {
                let mut class = scope.as_str();
                while !classes.contains(class) && !class.is_empty() {
                    class = crate::parent(class);
                }
                class.to_string()
            }
            (false, None) => scope.clone(),
        };
        let written = match path.ends_with('=') {
            true => None,
            false => member_at.get(&(at.clone(), path.clone())).copied(),
        };
        match written {
            Some(twice) => through[twice].value = Value::Unknown,
            None => {
                member_at.insert((at.clone(), path.clone()), through.len());
                through.push(Bound {
                    scope: at,
                    within: scope,
                    name: path,
                    value,
                });
            }
        }
    }
    let (mut kept, mut initial) = (Vec::new(), Vec::new());
    for binding in bound {
        let at = (binding.scope.clone(), binding.name.clone());
        if let Some(change) = reassignments.get(&at) {
            if !fields.contains(&binding.name) && !classes.contains(&binding.scope) {
                let mut value = binding.value.clone();
                if change.keep(&mut value) {
                    kept.push(Bound {
                        scope: binding.scope.clone(),
                        within: binding.within.clone(),
                        name: binding.name.clone(),
                        value,
                    });
                }
            }
            initial.push(binding);
        } else {
            kept.push(binding);
        }
    }
    let bound = kept;
    for def in &mut facts.defs {
        def.reassigned_params = def
            .params
            .iter()
            .filter(|p| reassignments.contains_key(&(def.name.clone(), p.name.clone())))
            .map(|p| p.name.clone())
            .collect();
    }
    let mut all: Vec<Bound> = (initial.into_iter())
        .map(|b| Bound {
            name: format!("{}=", b.name),
            ..b
        })
        .collect();
    all.append(&mut through);
    let through = all;
    // ponytail: a part of a `partial` type in another file may assign the field too.
    let enclosing = |scope: &str| {
        let mut class = scope;
        while !classes.contains(class) && !class.is_empty() {
            class = crate::parent(class);
        }
        class.to_string()
    };
    let mut by_class: BTreeMap<(String, &str), Vec<&Write>> = BTreeMap::new();
    for (name, written) in &writes {
        for write in written {
            let at = (enclosing(&write.0), name.as_str());
            by_class.entry(at).or_default().push(write);
        }
    }
    let ctors: BTreeSet<&str> = (facts.constructors.values().flatten())
        .map(String::as_str)
        .collect();
    let bindings: BTreeSet<(&str, &str)> = (facts.receivers.iter())
        .map(|r| (r.scope.as_str(), r.name.as_str()))
        .collect();
    let mut given = Vec::new();
    for (class, name) in readonly {
        let written = by_class.get(&(class.clone(), name.as_str()));
        if let Some([(ctor, this, Some(value))]) = written.map(Vec::as_slice)
            && ctors.contains(ctor.as_str())
            && crate::parent(ctor) == class
            && (*this || !bindings.contains(&(ctor.as_str(), name.as_str())))
        {
            given.push(Const {
                name: qualify(&class, &name),
                value: value.clone(),
                ty: None,
                scope: Some(ctor.clone()),
            });
        }
    }
    facts.consts.extend(given);
    let locals = bound.iter().map(|b| (b.scope.as_str(), b.name.as_str()));
    facts.mentioned = super::mentioned(tree.root_node(), src, &facts.defs, locals, |_| false);
    facts.bound = bound;
    facts.bound.extend(through);
    facts.externs = outside.into_iter().collect();
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

fn declares_main(root: Node, src: &[u8]) -> bool {
    (named(root).take_while(|c| c.kind() != "file_scoped_namespace_declaration")).any(|c| {
        c.kind().ends_with("_declaration")
            && c.kind() != "namespace_declaration"
            && c.child_by_field_name("name")
                .is_some_and(|n| text(n, src) == MAIN)
    })
}

fn inline(defining: bool, scope: &str, classes: &BTreeSet<String>) -> bool {
    defining && !classes.contains(scope)
}

fn lambdas(list: Option<Node>, scope: &str, args: &mut [Value], kwargs: &mut [(String, Value)]) {
    let (mut positional, mut keyword) = (0, 0);
    for argument in (list.into_iter().flat_map(named)).filter(|n| n.kind() == "argument") {
        let name = argument.child_by_field_name("name");
        let expression = named(argument).find(|n| Some(n.id()) != name.map(|m| m.id()));
        let slot = match name {
            Some(_) => kwargs.get_mut(keyword).map(|(_, v)| v),
            None => args.get_mut(positional),
        };
        match name {
            Some(_) => keyword += 1,
            None => positional += 1,
        }
        if let (Some(slot), Some(e)) = (slot, expression)
            && matches!(
                e.kind(),
                "lambda_expression" | "anonymous_method_expression"
            )
        {
            *slot = Value::Function(qualify(scope, &format!("@{}", e.start_byte())));
        }
    }
}

fn bind_unknown(bound: &mut Vec<Bound>, defining: bool, scope: &str, name: &str) {
    if defining {
        bound.push(Bound {
            scope: scope.to_string(),
            within: scope.to_string(),
            name: name.to_string(),
            value: Value::Unknown,
        });
    }
}

fn held(initializer: &Node) -> bool {
    match initializer.kind() {
        "await_expression" => named(*initializer).next().is_some_and(|n| held(&n)),
        kind => matches!(
            kind,
            "object_creation_expression"
                | "implicit_object_creation_expression"
                | "invocation_expression"
        ),
    }
}

fn opaque(mut at: Call, definitions: bool) -> Call {
    if !definitions {
        at.callee = format!("(opaque).{}", at.callee);
        at.by_path = false;
    }
    at
}

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

fn parameters(node: Node, src: &[u8]) -> Vec<(String, String, Binds)> {
    parameters_of(node.child_by_field_name("parameters"), src)
}

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
                if named(p).any(|m| m.kind() == "modifier" && text(m, src) == "this") {
                    Binds::Instance
                } else if defaulted {
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

fn parameter_classes(list: Option<Node>, src: &[u8], generics: &[String]) -> Vec<(String, Class)> {
    let Some(list) = list.filter(|l| l.kind() == "parameter_list") else {
        return Vec::new();
    };
    let mut out: Vec<(String, Class)> = (named(list).filter(|p| p.kind() == "parameter"))
        .filter_map(|p| {
            let name = text(p.child_by_field_name("name")?, src).to_string();
            let class = (p.child_by_field_name("type"))
                .map_or(Class::Unknown, |t| class_of(t, src, generics));
            Some((name, class))
        })
        .collect();
    if children(list).any(|c| c.kind() == "params")
        && let Some(name) = list.child_by_field_name("name")
    {
        out.push((text(name, src).to_string(), Class::Unknown));
    }
    out
}

fn class_of(ty: Node, src: &[u8], generics: &[String]) -> Class {
    let ty = match ty.kind() {
        "nullable_type" | "ref_type" => ty.child_by_field_name("type").unwrap_or(ty),
        _ => ty,
    };
    if !matches!(
        ty.kind(),
        "identifier" | "qualified_name" | "generic_name" | "alias_qualified_name"
    ) {
        return Class::Unknown;
    }
    let name = type_name(ty, src);
    let head = name.split('.').next().unwrap_or(&name);
    if name.contains('(') || generics.iter().any(|g| g == head) {
        return Class::Unknown;
    }
    Class::Type(name)
}

fn condition(node: Node) -> Option<Node> {
    let condition = node.child_by_field_name("condition")?;
    match condition.kind() {
        "await_expression" => named(condition).next(),
        _ => Some(condition),
    }
}

fn receiver_of(node: Node) -> Option<Node> {
    match node.kind() {
        "member_access_expression" => node.child_by_field_name("expression"),
        "conditional_access_expression" => condition(node),
        _ => None,
    }
}

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
        awaited: false,
        labels: None,
        constructs: false,
        range,
        args: Vec::new(),
        kwargs: Vec::new(),
        decorates: None,
        by_path: false,
        root: None,
        on: None,
        unwraps: false,
        awaits: false,
        receiver: None,
        arg_types: Vec::new(),
        uses: Vec::new(),
    }
}

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
    spelling.replace("global::", "").replace("::", ".")
}

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
            "conditional_access_expression" => {
                let member = (named(n).find(|c| c.kind() == "member_binding_expression"))
                    .and_then(|b| b.child_by_field_name("name"));
                match member {
                    Some(member) => {
                        pending.push(member);
                        pending.extend(condition(n));
                    }
                    None => parts.push("(expression)"),
                }
            }
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

thread_local! {
    static SCOPE: std::cell::RefCell<(Rc<String>, Option<Arc<str>>)> =
        std::cell::RefCell::new((Rc::new(String::new()), None));
}

// ponytail: the one library form of this pack, as Java's and Kotlin's `getenv` are of theirs
// (`CODING_STANDARDS.md`): the kind decides whether the `Environment` is the framework's.
const ENVIRONMENT_READ: &str = "GetEnvironmentVariable";

const TRIMS: &[&str] = &["Trim", "TrimEnd", "TrimStart"];

fn read(callee: String, key: String, parameter: bool) -> Part {
    let scope = SCOPE.with_borrow_mut(|(scope, shared)| {
        (shared.get_or_insert_with(|| Arc::from(scope.as_str()))).clone()
    });
    Part::Read {
        scope,
        callee,
        key,
        parameter,
        then: Vec::new(),
    }
}

fn read_of(value: &Value) -> Option<Part> {
    match value {
        Value::Str(parts) => match &parts[..] {
            [read @ Part::Read { .. }] => Some(read.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn coalesced<'t>(node: Node<'t>, src: &[u8]) -> Option<Vec<Node<'t>>> {
    let is_coalesce = |n: Node| {
        n.kind() == "binary_expression"
            && (n.child_by_field_name("operator")).is_some_and(|o| text(o, src) == "??")
    };
    let mut operands = Vec::new();
    let mut current = node;
    loop {
        operands.extend(current.child_by_field_name("left"));
        let right = current.child_by_field_name("right")?;
        match is_coalesce(right) {
            true if operands.len() < VALUE_DEPTH => current = right,
            true => return None,
            false => {
                operands.push(right);
                return Some(operands);
            }
        }
    }
}

fn value(node: Node, src: &[u8], depth: usize) -> Value {
    if depth >= VALUE_DEPTH {
        return Value::Unknown;
    }
    match node.kind() {
        "conditional_expression" => {
            let side = |field| {
                node.child_by_field_name(field)
                    .map_or(Value::Unknown, |n| value(n, src, depth + 1))
            };
            crate::lang::query_alternative(side("consequence"), side("alternative"))
        }
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
        "raw_string_literal" => match named(node).find(|n| n.kind() == "raw_string_content") {
            Some(content) if !text(content, src).contains('\n') => {
                Value::Str(vec![Part::Text(text(content, src).to_string())])
            }
            None => Value::Str(Vec::new()),
            _ => Value::Unknown,
        },
        "interpolated_string_expression" => interpolated(node, src, depth),
        "binary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|n| text(n, src) == "+") =>
        {
            let mut parts = Vec::new();
            let (mut numbers, mut sides) = (0, 0);
            for side in ["left", "right"] {
                sides += 1;
                match node
                    .child_by_field_name(side)
                    .map(|n| value(n, src, depth + 1))
                {
                    Some(Value::Str(found)) => parts.extend(found),
                    Some(Value::Ref(name)) => parts.push(Part::Ref(name)),
                    Some(Value::Scalar(_)) => {
                        numbers += 1;
                        parts.push(Part::Hole);
                    }
                    Some(value) => parts.push(Part::unread(&value)),
                    None => parts.push(Part::Hole),
                }
            }
            match numbers == sides {
                true => Value::Unknown,
                false => Value::Str(parts),
            }
        }
        "binary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|n| text(n, src) == "??") =>
        {
            let Some(mut operands) = coalesced(node, src) else {
                return Value::Unknown;
            };
            operands.retain(|n| n.kind() != "throw_expression");
            if let [one] = operands[..] {
                return value(one, src, depth + 1);
            }
            let mut reads = operands
                .into_iter()
                .map(|n| read_of(&value(n, src, depth + 1)));
            match (reads.next().flatten(), reads.flatten().count()) {
                (Some(left), 0) => Value::Str(vec![left]),
                (Some(_), _) => Value::Str(vec![Part::Hole]),
                _ => Value::Unknown,
            }
        }
        "postfix_unary_expression" if children(node).any(|c| c.kind() == "!") => named(node)
            .next()
            .map_or(Value::Unknown, |n| value(n, src, depth + 1)),
        "element_access_expression" => {
            let receiver = (node.child_by_field_name("expression")).map(|n| callee(n, src));
            let keys = (node.child_by_field_name("subscript"))
                .map(|list| arguments(Some(list), src, depth).0);
            match (receiver, keys.as_deref()) {
                (Some(receiver), Some([Value::Str(key)])) if !receiver.contains('(') => {
                    match &key[..] {
                        [Part::Text(key)] => {
                            Value::Str(vec![read(format!("{receiver}[]"), key.clone(), false)])
                        }
                        _ => Value::Unknown,
                    }
                }
                _ => Value::Unknown,
            }
        }
        "identifier" | "member_access_expression" | "qualified_name" => {
            Value::Ref(callee(node, src))
        }
        "this" => Value::Ref("this".into()),
        "invocation_expression" => {
            let function = node.child_by_field_name("function");
            if let Some(f) = function
                && f.kind() == "identifier"
                && text(f, src) == "nameof"
                && let Some(name) = named(node.child_by_field_name("arguments").unwrap_or(node))
                    .find(|n| n.kind() == "argument")
                    .and_then(|a| named(a).next())
                    .map(|e| callee(e, src))
                    .and_then(|c| c.rsplit('.').next().map(str::to_string))
                    .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_'))
            {
                return Value::Str(vec![Part::Text(name)]);
            }
            let object = function
                .filter(|f| f.kind() == "member_access_expression")
                .and_then(|f| f.child_by_field_name("expression"));
            let receiver = object.map(|n| value(n, src, depth + 1));
            let member = function.and_then(|f| f.child_by_field_name("name"));
            if let (
                Some(member),
                Some(Part::Read {
                    mut then,
                    scope,
                    callee,
                    key,
                    parameter,
                }),
            ) = (member, receiver.as_ref().and_then(read_of))
            {
                let member = text(member, src);
                if !TRIMS.contains(&member) {
                    then.push(member.to_string());
                }
                return Value::Str(vec![Part::Read {
                    scope,
                    callee,
                    key,
                    parameter,
                    then,
                }]);
            }
            let callee = function.map_or_else(String::new, |f| callee(f, src));
            let args = arguments(node.child_by_field_name("arguments"), src, depth).0;
            if let ([key], Some(member)) = (&args[..], member)
                && text(member, src) == ENVIRONMENT_READ
                && !callee.contains('(')
            {
                match key {
                    Value::Str(parts) if let [Part::Text(text)] = parts.as_slice() => {
                        return Value::Str(vec![read(callee, text.clone(), false)]);
                    }
                    Value::Ref(name) => return Value::Str(vec![read(callee, name.clone(), true)]),
                    _ => {}
                }
            }
            Value::Call(Invocation {
                callee,
                receiver: receiver.map(Box::new),
                args,
                kwargs: Vec::new(),
                range: node.byte_range(),
            })
        }
        "object_creation_expression" | "implicit_object_creation_expression" => {
            let ty = match node.child_by_field_name("type") {
                Some(ty) => type_name(ty, src),
                None if node.kind() == "implicit_object_creation_expression" => String::new(),
                None => return Value::Unknown,
            };
            let list = node
                .child_by_field_name("arguments")
                .or_else(|| named(node).find(|n| n.kind() == "argument_list"));
            let mut args = arguments(list, src, depth).0;
            let initializer = named(node).find(|n| n.kind() == "initializer_expression");
            if let Some(set_node) = initializer {
                let set = (named(set_node).filter(|n| n.kind() == "assignment_expression"))
                    .filter_map(|a| {
                        let left = a.child_by_field_name("left")?;
                        let right = a.child_by_field_name("right")?;
                        match left.kind() {
                            "identifier" => {
                                Some((text(left, src).to_string(), value(right, src, depth + 1)))
                            }
                            "element_binding_expression" => {
                                let mut keys = arguments(Some(left), src, depth).0;
                                (keys.len() == 1).then(|| {
                                    (
                                        "[]".to_string(),
                                        Value::List(vec![
                                            keys.remove(0),
                                            value(right, src, depth + 1),
                                        ]),
                                    )
                                })
                            }
                            _ => Some(("...".to_string(), Value::Unknown)),
                        }
                    })
                    .collect();
                let has_fields = named(set_node).any(|n| n.kind() == "assignment_expression");
                if has_fields || named(set_node).next().is_none() {
                    args.push(Value::Object(set));
                } else {
                    args.push(Value::List(
                        named(set_node).map(|n| value(n, src, depth + 1)).collect(),
                    ));
                }
            }
            Value::New(ty, args)
        }
        "initializer_expression" => {
            Value::List(named(node).map(|n| value(n, src, depth + 1)).collect())
        }
        "implicit_array_creation_expression" | "array_creation_expression" => {
            match named(node).find(|n| n.kind() == "initializer_expression") {
                Some(items) => {
                    Value::List(named(items).map(|n| value(n, src, depth + 1)).collect())
                }
                None => Value::Unknown,
            }
        }
        "collection_expression" => Value::List(
            named(node)
                .map(|mut n| {
                    while matches!(n.kind(), "collection_element" | "expression_element") {
                        match named(n).next() {
                            Some(inner) => n = inner,
                            None => return Value::Unknown,
                        }
                    }
                    value(n, src, depth + 1)
                })
                .collect(),
        ),
        "integer_literal" => Value::Scalar(text(node, src).parse().ok()),
        "boolean_literal" => Value::Scalar(None),
        "null_literal" => Value::Undefined,
        "await_expression" => named(node).next().map_or(Value::Unknown, |n| {
            Value::Await(Box::new(value(n, src, depth + 1)))
        }),
        "parenthesized_expression" => named(node)
            .next()
            .map_or(Value::Unknown, |n| value(n, src, depth + 1)),
        _ => Value::Unknown,
    }
}

fn interpolated(node: Node, src: &[u8], depth: usize) -> Value {
    let start = named(node).find(|n| n.kind() == "interpolation_start");
    let verbatim = start.is_some_and(|s| text(s, src).contains('@'));
    let raw = named(node).any(|n| n.kind() == "interpolation_quote");
    let (mut parts, mut done, mut chunk) = (Vec::new(), String::new(), String::new());
    let flush = |chunk: &mut String, done: &mut String| {
        let written = std::mem::take(chunk);
        done.push_str(&match (raw, verbatim) {
            (true, _) => written,
            (_, true) => written
                .replace("{{", "{")
                .replace("}}", "}")
                .replace("\"\"", "\""),
            _ => written.replace("{{", "{").replace("}}", "}"),
        });
    };
    for child in named(node) {
        match child.kind() {
            "string_content" => chunk.push_str(text(child, src)),
            "escape_sequence" if !raw && !verbatim => {
                flush(&mut chunk, &mut done);
                match escape(text(child, src)) {
                    Some(c) => done.push(c),
                    None => return Value::Unknown,
                }
            }
            "interpolation" => {
                flush(&mut chunk, &mut done);
                if !done.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut done)));
                }
                let clause = named(child).any(|n| n.kind().ends_with("_clause"));
                let hole = named(child)
                    .find(|n| !n.kind().ends_with("_clause") && n.kind() != "interpolation_brace");
                match hole.map(|n| value(n, src, depth + 1)) {
                    Some(Value::Str(found)) if !clause => parts.extend(found),
                    Some(Value::Ref(name)) if !clause => parts.push(Part::Ref(name)),
                    _ => parts.push(Part::Hole),
                }
            }
            _ => {}
        }
    }
    flush(&mut chunk, &mut done);
    if !done.is_empty() {
        parts.push(Part::Text(done));
    }
    Value::Str(parts)
}

fn escape(written: &str) -> Option<char> {
    Some(match written {
        "\\\"" => '"',
        "\\'" => '\'',
        "\\\\" => '\\',
        "\\n" => '\n',
        "\\r" => '\r',
        "\\t" => '\t',
        "\\0" => '\0',
        _ => return None,
    })
}

fn string(node: Node, src: &[u8]) -> Option<String> {
    let mut out = String::new();
    for part in named(node) {
        match part.kind() {
            "string_literal_content" => out.push_str(text(part, src)),
            "escape_sequence" => out.push(escape(text(part, src))?),
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

    #[test]
    fn the_locals_bound_keeps_are_counted_where_their_method_writes_them() {
        let counted = |body: &str| -> Vec<String> {
            let src = format!(
                "class C {{\n    List<int> shared = new List<int>();\n    void F() {{ {body} }}\n    \
                 void H() {{ var a = new T(); G(a); }}\n}}\n"
            );
            let facts = CSharp.parse("C.cs", src.as_bytes()).unwrap();
            (facts.mentioned.iter())
                .map(|m| format!("{} {} {}", m.scope, m.name, m.count))
                .collect()
        };
        assert_eq!(
            counted("var a = new T(); var b = new T(); G(a); Run(() => @a.N = 1); G(b, shared);"),
            ["C.F() a 3", "C.F() b 2", "C.H() a 2"]
        );
        assert!(counted("var a = new T(); G(\\u0061);").is_empty());
    }

    #[test]
    fn nameof_is_the_last_identifier_of_its_argument() {
        let name = |arg: &str| {
            let src = format!("class C {{ void M() {{ F({arg}); }} }}");
            let facts = CSharp.parse("C.cs", src.as_bytes()).unwrap();
            match &facts.calls[0].args[..] {
                [Value::Str(parts)] => match &parts[..] {
                    [Part::Text(t)] => Some(t.clone()),
                    _ => None,
                },
                _ => None,
            }
        };
        for (arg, expected) in [
            ("nameof(Orders)", "Orders"),
            ("nameof(Shop.Orders.Topic)", "Topic"),
            ("nameof(List<int>)", "List"),
        ] {
            assert_eq!(name(arg).as_deref(), Some(expected), "{arg}");
        }
        for arg in ["x.nameof(Orders)", "nameof(1)"] {
            assert_eq!(name(arg), None, "{arg}");
        }
    }

    #[test]
    fn a_long_chain_of_calls_is_read_in_linear_time() {
        let source = format!("class C {{ void M() {{ x{}; }} }}", ".A()".repeat(4096));
        let start = std::time::Instant::now();
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        let took = start.elapsed();
        assert_eq!(facts.calls.len(), 4096);
        assert!(took.as_secs() < 25, "{took:?}");
    }

    fn snapshot(size: usize) -> String {
        let mut source = String::from(
            "namespace Acme.Migrations\n{\npartial class AppModelSnapshot\n{\n\
             protected override void BuildModel(ModelBuilder modelBuilder)\n{\n",
        );
        let mut n = 0;
        while source.len() < size {
            source += &format!("modelBuilder.Entity(\"Acme.Entity{n}\", b =>\n{{\n");
            for p in 0..8 {
                source += &format!(
                    "b.Property<int>(\"P{p}\").ValueGeneratedOnAdd().HasColumnType(\"integer\");\n"
                );
            }
            source += "b.HasKey(\"P0\");\n});\n";
            n += 1;
        }
        source + "}\n}\n}\n"
    }

    #[test]
    fn a_generated_model_snapshot_is_read_whole() {
        for size in [154 << 10, 256 << 10, 1 << 20] {
            let source = snapshot(size);
            let start = std::time::Instant::now();
            let facts = CSharp.parse("Snapshot.cs", source.as_bytes()).unwrap();
            let took = start.elapsed();
            assert_eq!(facts.budget, None, "{size}");
            assert!(
                !facts.defs.is_empty() && facts.calls.len() > size / 100,
                "{size}"
            );
            assert!(took.as_secs() < 60, "{size}: {took:?}");
        }
    }

    #[test]
    fn a_long_namespace_across_a_broad_file_is_dropped_in_bounded_time() {
        let mut source = format!("namespace N{};\npublic class C {{\n", "a".repeat(4096));
        for n in 0..2000 {
            source += &format!("public void M{n}() {{ F(); }}\n");
        }
        source += "}\n";
        let start = std::time::Instant::now();
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        let took = start.elapsed();
        assert!(facts.defs.is_empty() && facts.calls.is_empty());
        assert_eq!(facts.budget, Some(QUALIFICATION_BYTES));
        assert!(!facts.unbuilt);
        assert!(took.as_secs() < 60, "{took:?}");
    }

    fn strings(source: &str, callee: &str) -> Vec<Vec<Part>> {
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        (facts.calls.iter().filter(|c| c.callee == callee))
            .flat_map(|c| &c.args)
            .filter_map(|a| match a {
                Value::Str(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_long_trim_or_fallback_chain_around_a_read_is_read_in_linear_time() {
        let get = "Environment.GetEnvironmentVariable(\"X\")";
        let trims = format!("{get}{}", ".Trim()".repeat(4096));
        let falls = format!("{get}{}", " ?? \"d\"".repeat(4096));
        let source = format!(
            "class C {{ void M() {{ H.Get({trims} + \"/a\"); H.Get(({falls}) + \"/b\"); }} }}"
        );
        let start = std::time::Instant::now();
        let found = strings(&source, "H.Get");
        assert!(start.elapsed().as_secs() < 25, "{:?}", start.elapsed());
        assert!(
            matches!(&found[0][..], [Part::Invoke { callee, .. }, Part::Text(path)]
            if callee == "(invocation_expression).Trim" && path == "/a")
        );
        assert_eq!(found[1], [Part::Hole, Part::Text("/b".into())]);
        let short = format!(
            "class C {{ void M() {{ H.Get({get}{} + \"/c\"); }} }}",
            ".Trim()".repeat(20)
        );
        let [parts] = &strings(&short, "H.Get")[..] else {
            panic!()
        };
        assert!(matches!(parts[0], Part::Read { .. }), "{parts:?}");
    }

    #[test]
    fn only_a_chain_with_one_read_and_a_reading_member_make_a_read() {
        let get = |k| format!("Environment.GetEnvironmentVariable(\"{k}\")");
        let is_read = |url: String| {
            let source = format!("class C {{ void M() {{ H.Get({url}); }} }}");
            matches!(&strings(&source, "H.Get")[..], [p] if matches!(p[..], [Part::Read { .. }]))
        };
        assert!(is_read(format!("{} ?? \"d\" ?? \"e\"", get("X"))));
        assert!(is_read(format!("({} ?? \"d\") ?? \"e\"", get("X"))));
        assert!(!is_read(format!("{} ?? \"d\" ?? {}", get("X"), get("Y"))));
        assert!(!is_read(format!("{} ?? {} ?? \"d\"", get("X"), get("Y"))));
        assert!(!is_read(format!("({} ?? \"d\") ?? {}", get("X"), get("Y"))));
        assert!(!is_read("Other.Get(\"X\")".into()));
    }

    #[test]
    fn a_member_called_on_a_read_and_string_empty_are_left_to_the_kind() {
        let source = "class C { void M() { \
                      H.Get(Environment.GetEnvironmentVariable(\"X\").TrimEnd('/').ToLower()); \
                      H.Get(string.Empty); } }";
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        let args: Vec<_> = (facts.calls.iter().filter(|c| c.callee == "H.Get"))
            .flat_map(|c| &c.args)
            .collect();
        let [Value::Str(read), Value::Ref(empty)] = &args[..] else {
            panic!("{}", args.len())
        };
        let [
            Part::Read {
                callee, key, then, ..
            },
        ] = &read[..]
        else {
            panic!("{read:?}")
        };
        assert_eq!(
            (callee.as_str(), key.as_str()),
            ("Environment.GetEnvironmentVariable", "X")
        );
        assert_eq!(then, &["ToLower"]);
        assert_eq!(empty, "string.Empty");
    }

    #[test]
    fn a_trim_called_on_a_read_leaves_it_the_read() {
        let source = "class C { void M() { \
                      H.Get(Environment.GetEnvironmentVariable(\"X\").Trim().TrimStart('/')); } }";
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        let call = facts.calls.iter().find(|c| c.callee == "H.Get").unwrap();
        let [Value::Str(read)] = &call.args[..] else {
            panic!("{}", call.args.len())
        };
        assert!(
            matches!(&read[..], [Part::Read { then, .. }] if then.is_empty()),
            "{read:?}"
        );
    }

    #[test]
    fn top_level_statements_are_a_main_unless_a_type_main_shares_the_name() {
        let mains = |source: &str| {
            let facts = CSharp.parse("Program.cs", source.as_bytes()).unwrap();
            let defs = (facts.defs.iter()).filter(|d| d.name == MAIN).count();
            (defs, facts.programs)
        };
        let top = "Run();\nvoid Run() { }\n";
        assert_eq!(mains(top), (1, vec![MAIN.to_string()]));
        assert_eq!(mains(&format!("{top}class Main {{ }}\n")), (1, vec![]));
        assert_eq!(
            mains(&format!("{top}namespace N {{ class Main {{ }} }}\n")),
            (1, vec![MAIN.to_string()])
        );
    }

    #[test]
    fn a_lambda_s_parameters_are_locals_of_its_inline_scope() {
        let source = "app.MapGet(\"/a\", (Order o) => o.Total());\n\
                      class C { void M() { H.On(x => x.Go()); } }\n";
        let facts = CSharp.parse("Program.cs", source.as_bytes()).unwrap();
        let inline: Vec<&str> = (facts.defs.iter())
            .filter(|d| d.kind == Kind::Inline)
            .map(|d| d.name.as_str())
            .collect();
        let [top, method] = inline[..] else {
            panic!("{inline:?}")
        };
        assert!(top.starts_with("Main.@"), "{top}");
        assert!(method.starts_with("C.M().@"), "{method}");
        let scope = |name: &str| {
            (facts.receivers.iter())
                .filter(|r| r.name == name)
                .map(|r| r.scope.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(scope("o"), [top]);
        assert_eq!(scope("x"), [method]);
    }

    #[test]
    fn the_reads_of_one_node_share_their_scope() {
        let holes = "{c[\"k\"]}".repeat(100);
        let source = format!(
            "namespace {}; class C {{ void M(IConfiguration c) {{ H.Get($\"{holes}\"); }} }}",
            "N".repeat(2_000)
        );
        let found = strings(&source, "H.Get");
        let [parts] = &found[..] else {
            panic!("{}", found.len())
        };
        let scopes: Vec<_> = parts
            .iter()
            .filter_map(|p| match p {
                Part::Read { scope, .. } => Some(scope),
                _ => None,
            })
            .collect();
        assert_eq!(scopes.len(), 100);
        assert!(scopes.iter().all(|s| Arc::ptr_eq(s, scopes[0])));
        assert!(scopes[0].len() > 2_000);
    }

    #[test]
    fn a_name_is_bound_to_the_call_it_awaits() {
        let source = "class C { IChannel _ch; IChannel Ch { get; set; }\n\
            async Task M(IConnection c) {\n\
              var a = await c.CreateChannelAsync();\n\
              _ch = await c.CreateChannelAsync();\n\
              var b = await Task.Delay(1);\n\
              var n = (await c.Count());\n\
            } }";
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        let held = |name: &str| -> Vec<String> {
            (facts.bound.iter().filter(|b| b.name == name))
                .map(|b| match &b.value {
                    Value::Await(value) => match value.as_ref() {
                        Value::Call(call) => call.callee.clone(),
                        _ => "other".into(),
                    },
                    _ => "other".into(),
                })
                .collect()
        };
        assert_eq!(held("a"), ["c.CreateChannelAsync"]);
        assert_eq!(held("_ch="), ["c.CreateChannelAsync"]);
        assert_eq!(held("b"), ["Task.Delay"]);
        assert_eq!(held("n"), ["c.Count"]);
        assert!(
            facts
                .bound
                .iter()
                .all(|b| matches!(b.value, Value::Await(_)))
        );
    }

    #[test]
    fn every_write_of_a_name_is_a_bound_of_its_own() {
        let source = "class C { ServiceBusSender _s = Make(); ServiceBusSender _t = Make();\n\
            C(X x, ServiceBusSender given) { _s = x.Create(\"a\"); this._t = given; }\n\
            void M(X x) { var l = x.Create(\"b\"); l = x.Create(\"c\"); int n = 0; n++; var k = x.Create(\"d\"); }\n\
            }";
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        let writes = |name: &str| -> Vec<&str> {
            (facts.bound.iter().filter(|b| b.name == name))
                .map(|b| match &b.value {
                    Value::Call(call) => call.callee.as_str(),
                    _ => "other",
                })
                .collect()
        };
        assert_eq!(writes("_s="), ["Make", "x.Create"]);
        assert_eq!(writes("_t="), ["Make", "other"]);
        assert_eq!(writes("l="), ["x.Create", "x.Create"]);
        assert_eq!(writes("n="), ["other", "other"]);
        assert!(writes("_s").is_empty() && writes("l").is_empty());
        assert_eq!(writes("k"), ["x.Create"]);
        assert!(writes("k=").is_empty());
    }

    #[test]
    fn thousands_of_writes_of_a_name_are_read_in_linear_time() {
        let mut source = String::from("class C { object _f; void M(X x) {\n");
        for _ in 0..20_000 {
            source += "_f = x.Make();\n";
        }
        source += "} }";
        let start = std::time::Instant::now();
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        assert_eq!(
            facts.bound.iter().filter(|b| b.name == "_f=").count(),
            20_000
        );
        assert!(start.elapsed().as_secs() < 40, "{:?}", start.elapsed());
    }

    #[test]
    fn a_handler_attached_with_plus_equals_is_bound_for_each_attachment() {
        let source = "class C { void M(X x) {\n\
              x.Received += (s, e) => { };\n\
              x.Received += OnReceived;\n\
              x.Count += 1;\n\
              y -= OnReceived;\n\
            }\n\
            void OnReceived(object s, object e) { } }";
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        let attached: Vec<(&str, &str, String)> = (facts.bound.iter())
            .filter(|b| b.name.ends_with("+="))
            .map(|b| {
                let value = match &b.value {
                    Value::Function(def) => format!("function {def}"),
                    Value::Ref(name) => format!("ref {name}"),
                    _ => "other".into(),
                };
                (b.scope.as_str(), b.name.as_str(), value)
            })
            .collect();
        let lambda = source.find("(s, e)").unwrap();
        assert_eq!(
            attached,
            [
                (
                    "C.M(X)",
                    "x.Received+=",
                    format!("function C.M(X).@{lambda}")
                ),
                ("C.M(X)", "x.Received+=", "ref OnReceived".to_string()),
            ]
        );
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
        assert!(facts["p/A.cs"].defs.is_empty() && !facts["p/A.cs"].unbuilt);
        assert_eq!(facts["p/A.cs"].usings, Some(PROJECT_USINGS));
        let facts = joined(
            &[("q/Q.csproj", sdk)],
            &[("q/G.cs", few), ("q/A.cs", "class A {}")],
        );
        assert_eq!(namespaces(&facts["q/A.cs"]), ["A"]);
        assert_eq!(facts["q/A.cs"].usings, None);
    }

    #[test]
    fn a_project_file_that_is_not_sdk_style_is_no_project() {
        let old = "<Project ToolsVersion=\"15.0\" \
                   xmlns=\"http://schemas.microsoft.com/developer/msbuild/2003\">\
                   <ItemGroup><Using Include=\"X\" /></ItemGroup></Project>";
        let files = [("old/A.cs", "class A {}"), ("old/G.cs", "global using Y;")];
        let facts = joined(&[("old/Old.csproj", old)], &files);
        assert!(namespaces(&facts["old/A.cs"]).is_empty());
        assert_eq!(namespaces(&facts["old/G.cs"]), ["Y"]);
    }

    fn given(body: &str) -> Vec<(String, String)> {
        let source = format!("namespace A;\nclass C {{\n{body}\n}}\n");
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        (facts.consts.into_iter())
            .filter_map(|c| {
                let scope = c.scope?;
                assert!(matches!(c.value, Value::Ref(_)), "{}", c.name);
                Some((c.name, scope))
            })
            .collect()
    }

    #[test]
    fn a_readonly_field_one_constructor_writes_once_holds_that_value() {
        let at = |ctor: &str| vec![("A.C._h".to_string(), format!("A.C.{ctor}"))];
        let field = "readonly HttpClient _h;\n";
        for ctor in [
            "C(HttpClient h) { _h = h; }",
            "C(HttpClient h) { this._h = h; }",
            "C(HttpClient h) { _h = h ?? throw new ArgumentNullException(nameof(h)); }",
        ] {
            assert_eq!(
                given(&format!("{field}{ctor}")),
                at("C(HttpClient)"),
                "{ctor}"
            );
        }
        let nested = "class Inner { readonly HttpClient _h; Inner(HttpClient x) { _h = x; } }";
        assert_eq!(
            given(&format!("{field}C(HttpClient h) {{ _h = h; }}\n{nested}")),
            [
                ("A.C._h".to_string(), "A.C.C(HttpClient)".to_string()),
                (
                    "A.C.Inner._h".to_string(),
                    "A.C.Inner.Inner(HttpClient)".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_readonly_field_written_twice_hidden_or_outside_a_constructor_holds_no_value() {
        let none: Vec<(String, String)> = Vec::new();
        for body in [
            "readonly HttpClient _h; C(HttpClient _h) { _h = _h; }",
            "readonly HttpClient _h; C() { HttpClient _h = null; _h = new HttpClient(); }",
            "readonly HttpClient _h; C(HttpClient h) { _h = h; } C() { _h = new HttpClient(); }",
            "readonly HttpClient _h; C(HttpClient h) { _h = h; Make(out _h); }",
            "readonly HttpClient _h; C(HttpClient h) { _h = h; Make(out this._h); }",
            "readonly HttpClient _h; C(HttpClient h) { _h = h; Make(ref this._h); }",
            "readonly string _s; C(string s) { _s = s; _s += \"/x\"; }",
            "readonly HttpClient _h; C(HttpClient a, HttpClient b) { (_h, _x) = (a, b); }",
            "readonly HttpClient _h; void M(HttpClient h) { _h = h; }",
            "readonly HttpClient _h; C(HttpClient h) { Action a = () => _h = h; }",
            "readonly HttpClient _h; class Inner { Inner(HttpClient h) { _h = h; } }",
            "readonly HttpClient _h = new HttpClient(); C(HttpClient h) { _h = h; }",
            "static readonly HttpClient S; static C() { S = new HttpClient(); }",
            "HttpClient _h; C(HttpClient h) { _h = h; }",
        ] {
            assert_eq!(given(body), none, "{body}");
        }
    }

    #[test]
    fn readonly_fields_of_many_types_are_read_in_linear_time() {
        let source: String = (0..8192)
            .map(|n| {
                format!(
                    "class C{n} {{ readonly HttpClient _h; C{n}(HttpClient h) {{ _h = h; }} }}\n"
                )
            })
            .collect();
        let start = std::time::Instant::now();
        let facts = CSharp.parse("C.cs", source.as_bytes()).unwrap();
        let took = start.elapsed();
        assert_eq!(
            facts.consts.iter().filter(|c| c.scope.is_some()).count(),
            8192
        );
        assert!(took.as_secs() < 40, "{took:?}");
    }
}
