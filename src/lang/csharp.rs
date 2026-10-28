//! The C# language pack: one source file's syntax, with no compiler, .NET SDK or MSBuild.
//! SDK-style project files say which files form a project, for the usings that apply to all of
//! them (`join`).

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

/// How many named types may enclose a definition. Deeper bodies keep their enclosing symbol.
// ponytail: real source nests a few types; this prevents quadratic qualified names.
const DEFINITION_DEPTH: usize = 128;
/// How deep arguments are read before their value becomes unknown.
// ponytail: real expressions nest a few levels; this bounds hostile source recursion.
const VALUE_DEPTH: usize = 64;
/// How many segments of a callee are kept before it becomes an unknown expression.
// ponytail: real member paths are short; this keeps crafted chains cheap.
const PATH_DEPTH: usize = 256;
/// Enclosing-name bytes past `QUALIFICATION_FREE` across syntax visits, before each visit's
/// bounded metadata fanout.
// ponytail: a long namespace prefix must not multiply across a broad file without a budget.
const QUALIFICATION_BYTES: usize = 4 * 1024 * 1024;
/// How long an enclosing name is before a visit is charged for it: a real qualified name is
/// shorter, so a generated file of any size reads whole, while a longer prefix pays its excess
/// at every node that repeats it.
// ponytail: a flat 256 bytes; a file under it keeps up to 256 bytes of scope per stored call,
// about 130 MB of facts at `MAX_BYTES`. Lower it if that shows up.
const QUALIFICATION_FREE: usize = 256;
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

    /// The BCL types whose members are few and fixed; the table grows with evidence from the
    /// dogfood repositories, never by guess (ADR 0019). `IEnumerable` is left out: the LINQ
    /// extension methods every file sees (`Contains`, `Count`, `First`) answer a call no own
    /// overload fits by type.
    fn known_outside(&self) -> super::KnownOutside {
        &[
            ("System.IDisposable", &["Dispose"]),
            ("System.IAsyncDisposable", &["DisposeAsync"]),
            ("System.IEquatable", &["Equals"]),
            ("System.IComparable", &["CompareTo"]),
        ]
    }

    /// Every project file (an SDK-style one is a project, `join`), and the props files MSBuild
    /// imports into the projects below them.
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

    /// A project's usings bind in each of its files: those its project file adds (`<Using>`,
    /// implicit usings) and the `global using` directives of its files. A file in no SDK-style
    /// project takes its own. An alias names a namespace unless a type of the repository has
    /// its target's name. Each file's `Facts::unit` is its project, whose files alone the parts
    /// of a partial type come from.
    fn join(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Join {
        // The props files by path, each read once (`None` when unreadable, which hides the
        // files above it all the same).
        let props: HashMap<&str, Option<msbuild::Props>> = (configs.iter())
            .filter(|(path, _)| msbuild::is_props(path))
            .map(|(path, src)| (*path, msbuild::props(path, src)))
            .collect();
        // Each project file with its directory (`a/b/`, empty at the root), read once under
        // the props files MSBuild imports into it.
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
        // Each project's directory and what its file adds.
        let projects: Vec<(String, Vec<Using>)> = (read.iter())
            .map(|(_, dir, project)| (dir.clone(), project.usings.clone()))
            .collect();
        let unit_references = unit_references(&read);
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
            facts.unit_references = unit_references.get(&unit).cloned().unwrap_or_default();
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

/// The projects each project directory references (`Facts::unit_references`), as the directories
/// of the `.csproj` files its `ProjectReference`s reach in the repository (`references`), nearest
/// first, none its own. A project at the root has no directory to name, so it is never listed.
/// The first project file of a directory stands for it, as in `owner`.
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

/// A glob of the namespace `source`'s types: a using's at the top level (`None`), or the inside
/// one that the code in the namespace `scope` reads.
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

/// The symbol of a file's top-level statements.
const MAIN: &str = "Main";

/// A write of a name or of `this.name` (`parse`): the scope written in, whether through
/// `this.`, and the value of a plain `=` (none for `+=`, `++`, `out`, `ref` or a tuple).
type Write = (String, bool, Option<Value>);

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
    /// Whether the node may not run when its definition (method, constructor, lambda) does: it
    /// sits in a branch, a loop, a `catch` or the right of `&&`, `||` or `??` there.
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
    // The names fields and properties bind, and the locals (and other bindings inside a
    // definition) with the class their declaration writes.
    let mut fields = BTreeSet::new();
    let mut locals: Vec<Receiver> = Vec::new();
    // The types declared so far: a local whose scope is one sits in an initializer.
    let mut classes = BTreeSet::new();
    // What a name written in a definition is bound to (`Facts::bound`), and the names assigned
    // anywhere in the file (`=`, `+=`, `++`, `out`, `ref`), which hold no one value.
    let mut bound: Vec<Bound> = Vec::new();
    let mut assigned: BTreeSet<String> = BTreeSet::new();
    // The assignments through a name (`c.BaseAddress = v`, `_c = v`): the scope written in, the
    // path without a leading `this.`, whether it began with one, and the value.
    let mut members: Vec<(String, String, bool, Value)> = Vec::new();
    // The instance `readonly` fields with no initializer, by class and name, and every write of
    // a name or of `this.name` in the file: the scope written in, whether through `this.`, and
    // the value of a plain `=` (none for `+=`, `++`, `out`, `ref` or a tuple).
    let mut readonly: Vec<(String, String)> = Vec::new();
    let mut writes: BTreeMap<String, Vec<Write>> = BTreeMap::new();
    // Initializer calls of `var` locals (by `Class::Result` place) and chained calls (by place
    // in `facts.calls`), linked to the calls they name once every call is read.
    let mut results: Vec<Range<usize>> = Vec::new();
    let mut chains: Vec<(usize, Range<usize>)> = Vec::new();
    // What each method, local function and lambda returns, by its qualified name.
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
            // A partial overload family could turn an ambiguous call into a false exact one.
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
                    label: None,
                    default: None,
                    fields: Vec::new(),
                    closure: Default::default(),
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
                suspends: false,
                hash: String::new(),
                // The declared return type of a method or a local function.
                result: match (kind, constructor) {
                    (Kind::Method | Kind::Function, false) => (node.child_by_field_name("returns"))
                        .or_else(|| node.child_by_field_name("type"))
                        .map(|t| class_of(t, src, &generics)),
                    _ => None,
                }
                .filter(|c| matches!(c, Class::Type(_))),
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
                        // A `static Main` method starts a program, whatever its signature.
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
                    let list = node.child_by_field_name("arguments");
                    (at.args, at.kwargs) = arguments(list, src, 0);
                    if inline(defining, &scope, &classes) {
                        lambdas(list, &scope, &mut at.args, &mut at.kwargs);
                    }
                    at.arity = Some(at.args.len() + at.kwargs.len());
                    // `For<Api>(..)`: the type arguments the call is given, as written.
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
                        // `a?.M()`: the call of `M` on `a`.
                        Some(f) if f.kind() == "conditional_access_expression" => condition(f),
                        _ => None,
                    };
                    at.receiver = object.map(|n| value(n, src, 0));
                    // `Make().Save()`, `Make().x.Save()`, `Make()?.Save()`: the call the chain
                    // is made on.
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
            // `T x = new(..)`, a field or a property too: a construction of the declared type.
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
                // An event holds a delegate, no type of the repository's.
                let declared = (node.kind() == "field_declaration").then(|| {
                    (named(node).find(|n| n.kind() == "variable_declaration"))
                        .and_then(|d| d.child_by_field_name("type"))
                });
                let class = match declared.flatten() {
                    Some(ty) if definitions => class_of(ty, src, &generics),
                    _ => Class::Unknown,
                };
                // A `const` or `static readonly` field is a constant of its type: what a name
                // folds to. A field that holds a construction is bound to it (`Facts::bound`).
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
            // `new T { A = v }` assigns no name in scope.
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
                        // `this.x = v` assigns the field `x`.
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
                    assigned.insert(text(name, src).to_string());
                    let at = (scope.as_ref().clone(), this, given.clone());
                    writes
                        .entry(text(name, src).to_string())
                        .or_default()
                        .push(at);
                    // Every write is a `name=` of its own (`Facts::bound`): the value of a call
                    // or a construction, unknown for any other.
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
                        // A member set by `??=` or `+=`, or by an assignment that may not run,
                        // holds no one value.
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
                // `x.Event += handler` attaches a handler to an event of the name: a bound of
                // its own for each attachment (`x.Event+=`), the lambda as the inline definition
                // it makes and a method group as the name it writes.
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
                    assigned.insert(text(target, src).to_string());
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
                // `out x`, and `out this.x`, which writes the field `x`.
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
                    assigned.insert(text(target, src).to_string());
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
                    // A property initialized with a construction or a call is bound to it.
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
            // The other bindings hide an outer name in the whole definition around them.
            "variable_declaration"
                if !node.parent().is_some_and(|p| {
                    matches!(p.kind(), "field_declaration" | "event_field_declaration")
                }) =>
            {
                // A declared type, or the class a `var` takes from `new T(..)` or a call. An
                // initializer lambda's local is no field of the class.
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
                    // It holds no one value, and shares the name of a local of the scope.
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
                // A lambda in a definition is a scope of its own, which a call's argument names
                // (`lambdas`): a kind may claim it as a handler, and it folds into the symbol
                // around it otherwise. Its parameters hide outer names in it, and what it
                // returns is its own.
                let own = inline(defining, &scope, &classes);
                if own {
                    inner = Rc::new(qualify(&scope, &format!("@{}", node.start_byte())));
                    facts.defs.push(Def {
                        name: inner.as_ref().clone(),
                        kind: Kind::Inline,
                        range: node.byte_range(),
                        params: Vec::new(),
                        bases: Vec::new(),
                        decorators: Vec::new(),
                        return_values: Vec::new(),
                        returns: None,
                        type_params: Vec::new(),
                        result: None,
                        suspends: false,
                        hash: String::new(),
                    });
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
                        // A parameter with a written type types its receiver as a method's
                        // does; an implicit one has none.
                        for (name, class) in parameter_classes(list, src, &generics) {
                            bind_unknown(&mut bound, definitions, &inner, &name);
                            // In a scope of its own the parameter hides a field of the same
                            // name, as a method's does: it is no local read as the field.
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
            // `return x;` in its definition or lambda, and a method's `=> x`.
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
        // A file-scoped namespace holds the declarations after it, its siblings.
        let mut child_scope = (inner.clone(), inner_namespace.clone());
        let mut visits = Vec::new();
        // The top-level statements of a file are the body of a synthetic `Main`, a program.
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
            facts.defs.push(Def {
                name: MAIN.to_string(),
                kind: Kind::Function,
                range: range.clone(),
                params: Vec::new(),
                bases: Vec::new(),
                decorators: Vec::new(),
                return_values: Vec::new(),
                returns: None,
                type_params: Vec::new(),
                result: None,
                suspends: false,
                hash: String::new(),
            });
            facts.programs.push(MAIN.to_string());
        }
        // A definition starts its own run; a branch, a loop or a short circuit may not run all
        // that is inside it.
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
    // The code in each namespace sees the types of it and of the namespaces around it without a
    // using: an inside glob of each, scoped at that namespace, so the climb from a scope reads
    // the innermost first and the file's usings, at the top level, after them all.
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
    // A local is typed only where it is the one binding of its name in its scope; one named
    // like a field may be read as the field outside its block, so its class is unknown.
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
    // A name assigned through a field (`_c.BaseAddress = v`, `this.c.BaseAddress = v`, `_c = v`) is
    // bound in the type that declares the field, so any method of it reads the value; through a
    // local or a parameter, in the scope that declares it.
    let bindings: BTreeSet<(&str, &str)> = (facts.receivers.iter())
        .map(|r| (r.scope.as_str(), r.name.as_str()))
        .collect();
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
        // A member written twice holds no one value; a `name=` is one write, each kept.
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
    // A name assigned anywhere holds no one value, and its initializer is one more write of it
    // (`name=`, the first): the kinds that read a name's only write count them.
    let (kept, initial): (Vec<Bound>, Vec<Bound>) =
        bound.into_iter().partition(|b| !assigned.contains(&b.name));
    let bound = kept;
    let mut all: Vec<Bound> = (initial.into_iter())
        .map(|b| Bound {
            name: format!("{}=", b.name),
            ..b
        })
        .collect();
    all.append(&mut through);
    let through = all;
    // An instance `readonly` field written once in the file, by a plain `=` in a constructor of
    // its class (as `this.f`, or as `f` where no parameter or local of the constructor is `f`),
    // holds that value, written in the constructor: a constant of the class, as Java's `final`
    // fields are (`Const::scope`).
    // ponytail: a part of a `partial` type in another file may assign the field too.
    let enclosing = |scope: &str| {
        let mut class = scope;
        while !classes.contains(class) && !class.is_empty() {
            class = crate::parent(class);
        }
        class.to_string()
    };
    // Each looked up once: the writes by class and name, the constructors, the bindings.
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
    facts.bound = bound;
    facts.bound.extend(through);
    facts.externs = outside.into_iter().collect();
    // A using of the namespace takes its types, internal ones too (they are the assembly's:
    // `Resolver::hidden` keeps them to its files), never the names the file's own usings bind.
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

/// Whether the compilation unit `root` declares a type named `Main` in the global namespace:
/// before any file-scoped namespace, whose declarations are its siblings.
fn declares_main(root: Node, src: &[u8]) -> bool {
    (named(root).take_while(|c| c.kind() != "file_scoped_namespace_declaration")).any(|c| {
        c.kind().ends_with("_declaration")
            && c.kind() != "namespace_declaration"
            && c.child_by_field_name("name")
                .is_some_and(|n| text(n, src) == MAIN)
    })
}

/// Whether a lambda written as an argument in `scope` is a `Kind::Inline` definition: inside
/// a definition the walk names (`defining`), not in a type's initializer.
fn inline(defining: bool, scope: &str, classes: &BTreeSet<String>) -> bool {
    defining && !classes.contains(scope)
}

/// Gives each lambda written as an argument of `list` (`args` its positional arguments and
/// `kwargs` its named ones, as `arguments` read them) as the `Kind::Inline` definition the walk
/// makes of it under `scope`.
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

/// A name bound in `scope` to no value the source gives (a loop variable, a pattern, a lambda
/// parameter): a bound of its own, so that a local of the scope that shares the name is not
/// taken for the only one (`Code::local`).
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

/// Whether a local or a field with this initializer is bound to it (`Facts::bound`): a
/// construction or a call, whose value the kinds read the client a name holds off.
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

/// `at`, made on nothing the repository holds when written outside `definitions`.
fn opaque(mut at: Call, definitions: bool) -> Call {
    if !definitions {
        at.callee = format!("(opaque).{}", at.callee);
        at.by_path = false;
    }
    at
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
                // The `this` parameter of an extension method is the receiver of a call written
                // `x.Method(..)`.
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

/// Each parameter of `list`: its name and the class its type names; a `params` array is no
/// class.
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
    // A `params` array binds its name too, as no class.
    if children(list).any(|c| c.kind() == "params")
        && let Some(name) = list.child_by_field_name("name")
    {
        out.push((text(name, src).to_string(), Class::Unknown));
    }
    out
}

/// The class a written type names, as a callee-like name: unknown for a predefined type, an
/// array, a tuple, `var`, a type parameter or a name with a call in it. A nullable type names
/// its underlying one.
fn class_of(ty: Node, src: &[u8], generics: &[String]) -> Class {
    let ty = match ty.kind() {
        // `ref` of a lambda parameter's type is part of the type node, not a modifier.
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

/// What a conditional access `a?.M` reads its member from: `a`, which the grammar wraps in
/// the `await` around it (`await a?.M()` is `await (a?.M())` in C#).
fn condition(node: Node) -> Option<Node> {
    let condition = node.child_by_field_name("condition")?;
    match condition.kind() {
        "await_expression" => named(condition).next(),
        _ => Some(condition),
    }
}

/// What a receiver is written on: the expression a member access or a conditional access reads
/// its member from.
fn receiver_of(node: Node) -> Option<Node> {
    match node.kind() {
        "member_access_expression" => node.child_by_field_name("expression"),
        "conditional_access_expression" => condition(node),
        _ => None,
    }
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
/// `Dictionary<string,int>`, `int[]`), comments left out, `::` read as `.`.
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
    // `global::A` is `A`, and `alias::A` reads as `alias.A`: a signature holds no `::`, which
    // separates a symbol's path from its name in an id.
    spelling.replace("global::", "").replace("::", ".")
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
            // `a?.B` reads as `a.B`.
            "conditional_access_expression" => {
                let member = (named(n).find(|c| c.kind() == "member_binding_expression"))
                    .and_then(|b| b.child_by_field_name("name"));
                match member {
                    Some(member) => {
                        pending.push(member);
                        pending.extend(condition(n));
                    }
                    // `a?[0]` is no name.
                    None => parts.push("(expression)"),
                }
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

thread_local! {
    /// The scope of the node `parse` is reading, and its shared copy once a read asks for it:
    /// the reads of one node share one allocation of it (`Part::Read`).
    static SCOPE: std::cell::RefCell<(Rc<String>, Option<Arc<str>>)> =
        std::cell::RefCell::new((Rc::new(String::new()), None));
}

/// The member whose call with one string literal is a read of the environment the kind decides
/// on (`Environment.GetEnvironmentVariable("X")`): any other call with a literal
/// (`MapGroup("/api")`, `CreateClient("x")`) stays a call.
// ponytail: the one library form of this pack, as Java's and Kotlin's `getenv` are of theirs
// (`CODING_STANDARDS.md`): the kind decides whether the `Environment` is the framework's.
const ENVIRONMENT_READ: &str = "GetEnvironmentVariable";

/// The methods of `System.String` that trim it: called on a read, they leave it the read.
const TRIMS: &[&str] = &["Trim", "TrimEnd", "TrimStart"];

/// `callee` and `key` as a read in the scope `parse` is reading.
fn read(callee: String, key: String) -> Part {
    let scope = SCOPE.with_borrow_mut(|(scope, shared)| {
        (shared.get_or_insert_with(|| Arc::from(scope.as_str()))).clone()
    });
    Part::Read {
        scope,
        callee,
        key,
        then: Vec::new(),
    }
}

/// The read a value is: a subscript or a call already written as one.
fn read_of(value: &Value) -> Option<Part> {
    match value {
        Value::Str(parts) => match &parts[..] {
            [read @ Part::Read { .. }] => Some(read.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The operands of a chain of `??` from `node`, left to right, or `None` when the chain is
/// longer than `VALUE_DEPTH`.
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

/// An expression as written, as far as this pack folds one: string literals (plain, verbatim,
/// raw and interpolated), `+` between strings, names, calls and constructions; the rest is
/// unknown. A construction's object initializer (`new T { A = 1 }`) is its last argument, an
/// `Object` of what it assigns.
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
        // A raw literal on one line: its content as written. Several lines are indented by the
        // closing quotes' column, which is not read.
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
                    _ => parts.push(Part::Hole),
                }
            }
            // `1 + 2` adds numbers.
            match numbers == sides {
                true => Value::Unknown,
                false => Value::Str(parts),
            }
        }
        // `a ?? b ?? c` keeps a read on its left, unless a fallback is a read too: exactly one.
        "binary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|n| text(n, src) == "??") =>
        {
            let Some(mut operands) = coalesced(node, src) else {
                return Value::Unknown;
            };
            // `x ?? throw ..` is `x` when it gives a value at all.
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
        // `x!` is `x`.
        "postfix_unary_expression" if children(node).any(|c| c.kind() == "!") => named(node)
            .next()
            .map_or(Value::Unknown, |n| value(n, src, depth + 1)),
        // `receiver["key"]`: a read, as far as the receiver is a name.
        "element_access_expression" => {
            let receiver = (node.child_by_field_name("expression")).map(|n| callee(n, src));
            let keys = (node.child_by_field_name("subscript"))
                .map(|list| arguments(Some(list), src, depth).0);
            match (receiver, keys.as_deref()) {
                (Some(receiver), Some([Value::Str(key)])) if !receiver.contains('(') => {
                    match &key[..] {
                        [Part::Text(key)] => {
                            Value::Str(vec![read(format!("{receiver}[]"), key.clone())])
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
            // `nameof(Shop.Orders)` is the last identifier of its argument, as written.
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
            // A member called on a read is the read with the member, for the kind to judge,
            // unless it only trims the string (`System.String`, the language's own runtime).
            if let (
                Some(member),
                Some(Part::Read {
                    mut then,
                    scope,
                    callee,
                    key,
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
                    then,
                }]);
            }
            let callee = function.map_or_else(String::new, |f| callee(f, src));
            let args = arguments(node.child_by_field_name("arguments"), src, depth).0;
            if let ([Value::Str(key)], Some(member)) = (&args[..], member)
                && let [Part::Text(key)] = &key[..]
                && text(member, src) == ENVIRONMENT_READ
                && !callee.contains('(')
            {
                return Value::Str(vec![read(callee, key.clone())]);
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
                // A target-typed `new(..)` names no type here.
                None if node.kind() == "implicit_object_creation_expression" => String::new(),
                None => return Value::Unknown,
            };
            let list = node
                .child_by_field_name("arguments")
                .or_else(|| named(node).find(|n| n.kind() == "argument_list"));
            let mut args = arguments(list, src, depth).0;
            let initializer = named(node).find(|n| n.kind() == "initializer_expression");
            if let Some(set) = initializer {
                let set = (named(set).filter(|n| n.kind() == "assignment_expression"))
                    .filter_map(|a| {
                        let left = a.child_by_field_name("left")?;
                        let right = a.child_by_field_name("right")?;
                        (left.kind() == "identifier")
                            .then(|| (text(left, src).to_string(), value(right, src, depth + 1)))
                    })
                    .collect();
                args.push(Value::Object(set));
            }
            Value::New(ty, args)
        }
        // `new[] { a, b }`, `new T[] { a, b }` and `[a, b]`.
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
        // An awaited value is the value of its operand.
        "parenthesized_expression" | "await_expression" => named(node)
            .next()
            .map_or(Value::Unknown, |n| value(n, src, depth + 1)),
        _ => Value::Unknown,
    }
}

/// An interpolated string, `$"{base}/orders/{id}"`: its text with `{{` and `}}` read as the
/// braces they escape (verbatim ones also `""`), a name or a string in a hole as its parts, any
/// other hole (one with an alignment or a format clause too) a `Hole`. A raw one (`$$"""..."""`)
/// escapes nothing: its holes are the doubled braces the grammar already splits.
fn interpolated(node: Node, src: &[u8], depth: usize) -> Value {
    let start = named(node).find(|n| n.kind() == "interpolation_start");
    let verbatim = start.is_some_and(|s| text(s, src).contains('@'));
    let raw = named(node).any(|n| n.kind() == "interpolation_quote");
    let (mut parts, mut done, mut chunk) = (Vec::new(), String::new(), String::new());
    // The text written between two holes or escapes, as it reads.
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

/// The character a simple escape sequence of a regular string writes.
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

/// The text of a regular string literal, its simple escapes read; `None` with another escape.
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

    /// An Entity Framework model snapshot of about `size` bytes: one `BuildModel` whose entity
    /// lambdas hold a call per property, as the scaffolder writes them.
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

    /// A generated file reads whole at any size the indexer takes: the budget is charged for a
    /// long enclosing name, not for every node of a file whose names are short. The 1 MiB case
    /// takes about 3 s in a local debug build, a twentieth of its bound.
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

    /// A long namespace repeated by every node of a broad file exhausts the budget in bounded
    /// time: the file keeps no definitions and carries the budget `meada status` names.
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

    /// The arguments of the calls of `callee` in `source`, as the strings they fold to.
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

    /// A read under a long `Trim` chain or a long `??` chain stays linear: past `VALUE_DEPTH` it
    /// is an unknown part, never a read.
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
        assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
        assert_eq!(found[0], [Part::Hole, Part::Text("/a".into())]);
        assert_eq!(found[1], [Part::Hole, Part::Text("/b".into())]);
        // Within the depth, the chain is one read.
        let short = format!(
            "class C {{ void M() {{ H.Get({get}{} + \"/c\"); }} }}",
            ".Trim()".repeat(20)
        );
        let [parts] = &strings(&short, "H.Get")[..] else {
            panic!()
        };
        assert!(matches!(parts[0], Part::Read { .. }), "{parts:?}");
    }

    /// Reads of three operands count across the chain, and only the calls of the reading members
    /// with one literal are reads.
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
        // A call with a literal of another member stays a call, as `MapGroup("/api")` does.
        assert!(!is_read("Other.Get(\"X\")".into()));
    }

    /// The pack reads no library: a member called on a read is the read with the members it
    /// calls (a trim leaves it the read), which the kind judges, and `string.Empty` is a name like `String.Empty`.
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

    /// The trims of `System.String` are the pack's runtime: a read they trim is still the read.
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

    /// Top-level statements are a synthetic `Main`, a program, unless the file declares a type
    /// `Main` in the global namespace, which would share its name.
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

    /// A lambda is a `Kind::Inline` scope of its own, in top-level statements as in a method:
    /// its parameters are locals of that scope, never of the code around it.
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

    /// The reads of one node share one allocation of their scope, however long it is.
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

    /// What a call awaited is bound to: the call itself, as the same call not awaited is.
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
                    Value::Call(call) => call.callee.clone(),
                    _ => "other".into(),
                })
                .collect()
        };
        assert_eq!(held("a"), ["c.CreateChannelAsync"]);
        assert_eq!(held("_ch="), ["c.CreateChannelAsync"]);
        assert_eq!(held("b"), ["Task.Delay"]);
        // A parenthesized await is the value of the expression inside.
        assert_eq!(held("n"), ["c.Count"]);
    }

    /// Every write of a name is a `name=` of its own, a call or a construction by its value and
    /// any other by none, and the initializer of a name written again is one of them; a name
    /// written once through its initializer keeps the plain name.
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
        // The initializer is the first write; the constructor's is the second.
        assert_eq!(writes("_s="), ["Make", "x.Create"]);
        assert_eq!(writes("_t="), ["Make", "other"]);
        assert_eq!(writes("l="), ["x.Create", "x.Create"]);
        assert_eq!(writes("n="), ["other", "other"]);
        assert!(writes("_s").is_empty() && writes("l").is_empty());
        // Written once, it is bound by its plain name only.
        assert_eq!(writes("k"), ["x.Create"]);
        assert!(writes("k=").is_empty());
    }

    /// A file of thousands of writes is read in one pass: a write is looked up, never searched for.
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
        assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
    }

    /// Each handler attached to an event of a name with `+=` is a bound of its own, the lambda
    /// as the inline definition it makes and a method group as the name it writes.
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

    /// The constants of `body`, the members of `class C` in namespace `A`, whose value a
    /// constructor writes (`Const::scope`), with that constructor; the value must be a name.
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
        // A nested type's constructor writes a field of its own name, not this one.
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
            // A parameter or a local of the constructor named like the field.
            "readonly HttpClient _h; C(HttpClient _h) { _h = _h; }",
            "readonly HttpClient _h; C() { HttpClient _h = null; _h = new HttpClient(); }",
            // Two writes.
            "readonly HttpClient _h; C(HttpClient h) { _h = h; } C() { _h = new HttpClient(); }",
            "readonly HttpClient _h; C(HttpClient h) { _h = h; Make(out _h); }",
            "readonly HttpClient _h; C(HttpClient h) { _h = h; Make(out this._h); }",
            "readonly HttpClient _h; C(HttpClient h) { _h = h; Make(ref this._h); }",
            "readonly string _s; C(string s) { _s = s; _s += \"/x\"; }",
            "readonly HttpClient _h; C(HttpClient a, HttpClient b) { (_h, _x) = (a, b); }",
            // Outside a constructor, in a lambda of it, or in a nested type's.
            "readonly HttpClient _h; void M(HttpClient h) { _h = h; }",
            "readonly HttpClient _h; C(HttpClient h) { Action a = () => _h = h; }",
            "readonly HttpClient _h; class Inner { Inner(HttpClient h) { _h = h; } }",
            // An initializer, or `static`.
            "readonly HttpClient _h = new HttpClient(); C(HttpClient h) { _h = h; }",
            "static readonly HttpClient S; static C() { S = new HttpClient(); }",
            // Not `readonly`.
            "HttpClient _h; C(HttpClient h) { _h = h; }",
        ] {
            assert_eq!(given(body), none, "{body}");
        }
    }

    /// Many types writing a field of one name each cost their number, not its square.
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
        assert!(took.as_secs() < 10, "{took:?}");
    }
}
