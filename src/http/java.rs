//! Spring MVC and JAX-RS (`jax_rs`, #87) providers (spec #26): a method of a `@RestController` or `@Controller` class
//! carrying a mapping annotation serves the class prefix (`@RequestMapping`) joined to each of
//! its paths. The pack reads an annotation as a call decorating its target (`Call::decorates`,
//! the target a class or a method), so the annotations, `RequestMethod.X` and the constants of
//! the paths are read here, never in the pack: an annotation or an enum counts only when
//! `Code::outside` says an import from Spring binds its name (a class or an enum of the
//! repository with the same name hides it), and a path folds `static final` constants and `+`
//! through `Code::value`. `@RequestMapping` without `method` serves every method (`ANY`).
//! `server.servlet.context-path` is not applied.
//! The consumers are `clients.rs`'s and `declarative.rs`'s.

mod clients;
mod declarative;

use super::{ANY, METHODS, contract, join, path_literal, provider};
use crate::contract::{Found, Site};
use crate::index::Indexed;
use crate::jvm::names::{is_method, java_files, known, one, typed};
use crate::lang::{Call, Value};
use crate::resolve::{Code, Follow, Of, passes_nothing};

const WEB: &str = "org.springframework.web.bind.annotation.";
const CONTROLLERS: &[&str] = &[
    "org.springframework.web.bind.annotation.RestController",
    "org.springframework.stereotype.Controller",
];
const MAPPING: &str = "org.springframework.web.bind.annotation.RequestMapping";
/// The method annotations that give one method, by the annotation's name in `WEB`.
const SHORTCUTS: &[(&str, &str)] = &[
    ("GetMapping", "GET"),
    ("PostMapping", "POST"),
    ("PutMapping", "PUT"),
    ("PatchMapping", "PATCH"),
    ("DeleteMapping", "DELETE"),
];
/// The members of Spring's `RequestMethod`, which its `HttpMethod` has too.
const REQUEST_METHODS: &[&str] = &[
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE",
];
const REQUEST_METHOD: &str = "org.springframework.web.bind.annotation.RequestMethod.";
/// What a mapping annotation names its paths by: `value` or its alias `path`.
const MAPPING_PATHS: &[&str] = &["value", "path"];

/// The consumers of the clients (`clients.rs`) and of the declarative clients (`declarative.rs`).
pub(super) fn consumers<'a>(code: &Code<'a>, indexed: &Indexed) -> Vec<Found> {
    let mut found = clients::consumers(code, indexed);
    found.extend(declarative::consumers(code, indexed));
    found
}

/// Spring MVC providers: see the module.
pub(super) fn providers<'a>(code: &Code<'a>) -> Vec<Found> {
    let mut found = Vec::new();
    for (path, facts) in java_files(code) {
        // What Spring annotation a call is, by its name outside the repository.
        let spring = |call: &'a Call| -> Option<String> {
            known(code, path, &call.scope, &call.callee, annotation)
        };
        let annotations: Vec<(usize, &'a Call, String)> = (facts.calls.iter().enumerate())
            .filter(|(_, c)| c.decorates.is_some())
            .filter_map(|(n, c)| Some((n, c, spring(c)?)))
            .collect();
        for (n, call, name) in &annotations {
            // A method's annotation; a class carries the prefix and the controller marker.
            let Some(target) = call.decorates.as_deref().filter(|t| is_method(facts, t)) else {
                continue;
            };
            let on = |class: &'a str| {
                annotations
                    .iter()
                    .filter(move |(_, c, _)| c.decorates.as_deref() == Some(class))
            };
            // A method of a controller class: its annotation's scope is the class.
            let shortcut = shortcut(name);
            if shortcut.is_none() && name != MAPPING
                || !on(&call.scope).any(|(.., n)| CONTROLLERS.contains(&n.as_str()))
            {
                continue;
            }
            // The index keeps no argument: they come from the full facts of the file.
            let Some(given) = code.call(path, *n) else {
                continue;
            };
            let methods = match shortcut {
                Some(method) => vec![method.to_string()],
                None => match request_methods(code, path, given) {
                    Some(methods) => methods,
                    None => continue,
                },
            };
            let prefixes = match on(&call.scope).find(|(.., n)| n == MAPPING) {
                None => vec![String::new()],
                Some((c, ..)) => match code.call(path, *c) {
                    // A class-level `method` restricts every method of the class: not read.
                    Some(class) if class.keyword("method").is_none() => {
                        match paths(code, path, class, MAPPING_PATHS) {
                            Some(prefixes) => prefixes,
                            None => continue,
                        }
                    }
                    _ => continue,
                },
            };
            let Some(routes) = paths(code, path, given, MAPPING_PATHS) else {
                continue;
            };
            for prefix in &prefixes {
                for route in &routes {
                    let full = join(prefix, route);
                    if !balanced(&full) {
                        continue;
                    }
                    for method in &methods {
                        let site = Site::Def {
                            path: path.into(),
                            name: target.into(),
                        };
                        found.push(provider(site, contract(method, &full)));
                    }
                }
            }
        }
    }
    jax_rs(code, &mut found);
    found
}

/// The packages of JAX-RS, before and after the move to Jakarta EE.
const JAX_RS: &[&str] = &["javax.ws.rs.", "jakarta.ws.rs."];

/// Whether `spelled` is a JAX-RS annotation this module reads: `Path` or a method annotation,
/// which is the HTTP method itself (`METHODS`).
fn jax_rs_annotation(spelled: &str) -> bool {
    (JAX_RS.iter())
        .filter_map(|p| spelled.strip_prefix(p))
        .any(|n| n == "Path" || METHODS.contains(&n))
}

/// JAX-RS providers: a method of a class with `@Path` carrying a method annotation serves the
/// class path joined to its own `@Path`. A method with only `@Path` is a sub-resource locator,
/// and `@ApplicationPath` is not applied, and an interface is a client, not a resource. Names count as Spring's do, through `known`.
fn jax_rs<'a>(code: &Code<'a>, found: &mut Vec<Found>) {
    for (path, facts) in java_files(code) {
        let annotations: Vec<(usize, &'a Call, String)> = (facts.calls.iter().enumerate())
            .filter(|(_, c)| c.decorates.is_some())
            .filter_map(|(n, c)| {
                let name = known(code, path, &c.scope, &c.callee, jax_rs_annotation)?;
                Some((n, c, name))
            })
            .collect();
        let path_of = |target: &str| {
            (annotations.iter()).find(|(_, c, n)| {
                c.decorates.as_deref() == Some(target)
                    && (JAX_RS.iter()).any(|p| n.strip_prefix(p) == Some("Path"))
            })
        };
        for (_, call, name) in &annotations {
            let verb = (JAX_RS.iter())
                .filter_map(|p| name.strip_prefix(p))
                .find(|n| METHODS.contains(n));
            let (Some(verb), Some(target)) = (verb, call.decorates.as_deref()) else {
                continue;
            };
            if !is_method(facts, target) {
                continue;
            }
            // The index keeps no argument: they come from the full facts of the file.
            let read = |entry: Option<&(usize, &'a Call, String)>| {
                let given = code.call(path, entry?.0)?;
                paths(code, path, given, MAPPING_PATHS)
            };
            // An interface with `@Path` is a client (MicroProfile, Quarkus), not a server.
            if facts.interfaces.contains(&call.scope) {
                continue;
            }
            let Some(prefixes) = read(path_of(&call.scope)) else {
                continue;
            };
            let routes = match path_of(target) {
                None => Some(vec![String::new()]),
                some => read(some),
            };
            let Some(routes) = routes else { continue };
            for (prefix, route) in prefixes
                .iter()
                .flat_map(|p| routes.iter().map(move |r| (p, r)))
            {
                let full = join(prefix, route);
                if balanced(&full) {
                    let site = Site::Def {
                        path: path.into(),
                        name: target.into(),
                    };
                    found.push(provider(site, contract(verb, &full)));
                }
            }
        }
    }
}

/// Whether `spelled` is a Spring annotation this module reads.
fn annotation(spelled: &str) -> bool {
    CONTROLLERS.contains(&spelled) || mapping(spelled)
}

/// Whether `spelled` is one of Spring's mapping annotations: `@RequestMapping` or a shortcut.
fn mapping(spelled: &str) -> bool {
    spelled == MAPPING || shortcut(spelled).is_some()
}

/// The one method of the mapping shortcut `spelled` (`@GetMapping`), if it is one.
fn shortcut(spelled: &str) -> Option<&'static str> {
    let name = spelled.strip_prefix(WEB)?;
    (SHORTCUTS.iter())
        .find(|(s, _)| *s == name)
        .map(|(_, method)| *method)
}

/// The member of a Spring method enum that `spelled` names, the enum spelled `of` with its dot:
/// `RequestMethod` (`REQUEST_METHOD`) or `HttpMethod`, which have the same members.
fn member_of<'s>(spelled: &'s str, of: &str) -> Option<&'s str> {
    let member = spelled.strip_prefix(of)?;
    REQUEST_METHODS.contains(&member).then_some(member)
}

/// Whether `client` (a name, or a call made in place) is of the client type `ty`: written as
/// one, or made by a call it traces back to that `makes` accepts (`makes(code, path, index)`).
fn made<'a>(
    code: &Code<'a>,
    (path, scope): (&'a str, &'a str),
    client: &'a Value,
    ty: &str,
    makes: impl Fn(&Code<'a>, &'a str, usize) -> bool,
) -> bool {
    match client {
        Value::Ref(name) => {
            if one(&typed(code, path, scope, name), |t| t == ty).is_some() {
                return true;
            }
            let follow = Follow::Values {
                passes: passes_nothing,
            };
            let origins = code.origins(path, scope, Of::Name(name), follow);
            (origins.iter().filter_map(|o| o.call)).any(|(at, i)| makes(code, at, i))
        }
        Value::Call(inner) => code
            .call_index(path, &inner.range)
            .is_some_and(|i| makes(code, path, i)),
        _ => false,
    }
}

/// The paths of a mapping annotation: one of the elements `keys` names (Spring's `value` or
/// `path`), or the one unnamed argument, a string or an array of them, constants folded; the
/// root when it gives none. `None` when it gives two, or a value that is no list of strings; a
/// path that cannot be read yields none.
fn paths<'a>(code: &Code<'a>, file: &'a str, call: &'a Call, keys: &[&str]) -> Option<Vec<String>> {
    let given: Vec<&Value> = (call.args.iter())
        .chain(keys.iter().filter_map(|k| call.keyword(k)))
        .collect();
    let value = match given.as_slice() {
        [] => return Some(vec![String::new()]),
        [one] => code.value(file, &call.scope, one),
        _ => return None,
    };
    match value {
        Value::List(items) if items.is_empty() => Some(vec![String::new()]),
        Value::List(items) => Some(items.into_iter().filter_map(path_literal).collect()),
        one => path_literal(one).map(|p| vec![p]),
    }
}

/// The methods `@RequestMapping(method = ..)` names: every method without one, and those of
/// `RequestMethod.X` that Spring's enum binds (a member read through an import from Spring, so
/// a type of the repository by that name is none).
fn request_methods<'a>(code: &Code<'a>, file: &'a str, call: &'a Call) -> Option<Vec<String>> {
    let member = |value: &'a Value| -> Option<String> {
        let Value::Ref(name) = value else {
            return None;
        };
        let member = |s: &str| member_of(s, REQUEST_METHOD).is_some();
        let spelled = known(code, file, &call.scope, name, member)?;
        member_of(&spelled, REQUEST_METHOD).map(str::to_string)
    };
    let methods: Vec<String> = match call.keyword("method") {
        None => return Some(vec![ANY.into()]),
        Some(Value::List(items)) => items.iter().filter_map(member).collect(),
        Some(one) => member(one).into_iter().collect(),
    };
    let empty = matches!(call.keyword("method"), Some(Value::List(items)) if items.is_empty());
    match (methods.is_empty(), empty) {
        (true, true) => Some(vec![ANY.into()]),
        (true, false) => None,
        _ => Some(methods),
    }
}

/// Whether every `{` of `path` closes within its segment, as `{id:[0-9]{3}}` does: a pattern
/// with a `/` inside braces is not read.
fn balanced(path: &str) -> bool {
    path.split('/').all(|segment| {
        let mut depth = 0usize;
        for c in segment.chars() {
            match c {
                '{' => depth += 1,
                '}' => match depth.checked_sub(1) {
                    Some(d) => depth = d,
                    None => return false,
                },
                _ => {}
            }
        }
        depth == 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::java::Java;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    const IMPORTS: &str = "import org.springframework.web.bind.annotation.GetMapping;\n\
        import org.springframework.web.bind.annotation.RequestMapping;\n\
        import org.springframework.web.bind.annotation.RequestMethod;\n\
        import org.springframework.web.bind.annotation.RestController;\n";

    /// The contracts of the providers one controller file gives.
    fn served(imports: &str, body: &str) -> Vec<String> {
        served_with(imports, body, &[])
    }

    /// The same, with other files of the repository (`(path, source)`).
    fn served_with(imports: &str, body: &str, others: &[(&str, &str)]) -> Vec<String> {
        let src = format!("package a;\n{imports}\n{body}\n");
        let mut parsed = vec![("a/C.java", Java.parse("a/C.java", src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, Java.parse(path, source.as_bytes()).unwrap()));
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let code = Code::new(&resolver, &files, &Java);
        let mut found: Vec<String> = (providers(&code).into_iter())
            .filter_map(|f| f.contract)
            .collect();
        found.sort();
        found
    }

    fn controller(annotations: &str, methods: &str) -> Vec<String> {
        served(
            IMPORTS,
            &format!("@RestController {annotations} class C {{ {methods} }}"),
        )
    }

    #[test]
    fn a_method_less_request_mapping_serves_every_method_and_an_empty_list_too() {
        let ok = |m: &str| {
            controller(
                "",
                &format!("@RequestMapping(value = \"/x\"{m}) void f() {{}}"),
            )
        };
        assert_eq!(ok(""), ["http::ANY::/x"]);
        assert_eq!(ok(", method = {}"), ["http::ANY::/x"]);
        assert_eq!(ok(", method = RequestMethod.TRACE"), ["http::TRACE::/x"]);
    }

    #[test]
    fn a_method_that_is_not_springs_enum_member_serves_nothing() {
        let with = |imports: &str, local: &str, m: &str| {
            served(
                imports,
                &format!(
                    "@RestController class C {{ {local} @RequestMapping(value = \"/x\", method = {m}) void f() {{}} }}"
                ),
            )
        };
        let spring = IMPORTS;
        assert_eq!(with(spring, "", "RequestMethod.GET"), ["http::GET::/x"]);
        // Nowhere to be read, not a member, or hidden by a type of the file.
        assert!(with(spring, "", "Unknown.GET").is_empty());
        assert!(with(spring, "", "RequestMethod.SPELL").is_empty());
        assert!(with(spring, "enum RequestMethod { GET }", "RequestMethod.GET").is_empty());
        let other = IMPORTS.replace(
            "org.springframework.web.bind.annotation.RequestMethod",
            "org.acme.RequestMethod",
        );
        assert!(with(&other, "", "RequestMethod.GET").is_empty());
    }

    #[test]
    fn an_annotation_of_another_package_or_without_a_controller_serves_nothing() {
        let other = IMPORTS.replace(
            "org.springframework.web.bind.annotation.GetMapping",
            "org.acme.GetMapping",
        );
        assert!(
            served(
                &other,
                "@RestController class C { @GetMapping(\"/x\") void f() {} }"
            )
            .is_empty()
        );
        assert!(served(IMPORTS, "class C { @GetMapping(\"/x\") void f() {} }").is_empty());
        // A nested class's own annotations are not its methods'.
        let nested = "@RestController class C { @RequestMapping(\"/in\") @RestController static class D { @GetMapping(\"/x\") void f() {} } }";
        assert_eq!(served(IMPORTS, nested), ["http::GET::/in/x"]);
    }

    /// A controller whose base comes from outside the repository serves nothing: a member type
    /// of that base may be a method annotation's name (#354, "Opaque outside").
    #[test]
    fn a_controller_with_an_outside_base_serves_nothing() {
        let imports = format!("{IMPORTS}import com.corp.shared.BaseController;\n");
        let based = "@RestController @RequestMapping(\"/orders\") class C extends BaseController { \
            @GetMapping(\"/{id}\") void get() {} }";
        assert!(served(&imports, based).is_empty());
        let own = "@RestController @RequestMapping(\"/orders\") class C { @GetMapping(\"/{id}\") void get() {} }";
        assert_eq!(served(&imports, own), ["http::GET::/orders/{}"]);
    }

    #[test]
    fn what_cannot_be_read_with_certainty_serves_nothing() {
        // `value` and `path` together, a class-level `method`, a pattern with a `/` inside braces.
        let both = "@GetMapping(value = \"/a\", path = \"/b\") void f() {}";
        assert!(controller("", both).is_empty());
        let restricted = "@GetMapping(\"/a\") void f() {}";
        assert!(controller("@RequestMapping(method = RequestMethod.POST)", restricted).is_empty());
        let slash = "@GetMapping(\"/a/{id:x/y}\") void f() {}";
        assert!(controller("", slash).is_empty());
        let regex = "@GetMapping(\"/a/{id:[0-9]{3}}\") void f() {}";
        assert_eq!(controller("", regex), ["http::GET::/a/{}"]);
    }

    #[test]
    fn a_class_prefix_joins_each_path_of_each_array() {
        let found = controller(
            "@RequestMapping({\"/v1\", \"/v2/\"})",
            "@GetMapping({\"a\", \"/b/\"}) void f() {}",
        );
        assert_eq!(
            found,
            [
                "http::GET::/v1/a",
                "http::GET::/v1/b",
                "http::GET::/v2/a",
                "http::GET::/v2/b"
            ]
        );
    }

    const WILD: &str = "import org.springframework.web.bind.annotation.*;\n";
    const ROUTE: &str = "@RestController @RequestMapping(\"/p\") class C { \
        @GetMapping(\"/x\") void f() {} \
        @RequestMapping(value = \"/y\", method = RequestMethod.POST) void g() {} }";

    #[test]
    fn a_wildcard_import_from_spring_binds_its_annotations_and_enum() {
        assert_eq!(served(WILD, ROUTE), ["http::GET::/p/x", "http::POST::/p/y"]);
        // Another on-demand import, `java.lang` and the package's own do not bind them.
        let more = format!("import java.util.*;\nimport org.acme.*;\n{WILD}");
        assert_eq!(
            served(&more, ROUTE),
            ["http::GET::/p/x", "http::POST::/p/y"]
        );
    }

    #[test]
    fn a_wildcard_import_binds_nothing_another_binding_or_candidate_may_take() {
        // Only the `GetMapping` is hidden: `RequestMethod.POST` still binds.
        const POST: [&str; 1] = ["http::POST::/p/y"];
        // A class of the package, a single-type import, a class a repository wildcard brings,
        // and a static on-demand import of an outside class: any of them may be the name.
        let same = [("a/GetMapping.java", "package a; class GetMapping {}")];
        assert_eq!(served_with(WILD, ROUTE, &same), POST);
        // An annotation type of the package hides it too.
        let annotation = [(
            "a/GetMapping.java",
            "package a; @interface GetMapping { String[] value(); }",
        )];
        assert_eq!(served_with(WILD, ROUTE, &annotation), POST);
        let single = format!("{WILD}import org.acme.GetMapping;\n");
        assert_eq!(served(&single, ROUTE), POST);
        let other = [("b/GetMapping.java", "package b; public class GetMapping {}")];
        let repository = format!("{WILD}import b.*;\n");
        assert_eq!(served_with(&repository, ROUTE, &other), POST);
        // A repository wildcard that does not define the name leaves it to Spring's.
        let unrelated = [("b/Other.java", "package b; public class Other {}")];
        assert_eq!(served_with(&repository, ROUTE, &unrelated).len(), 2);
        let ambiguous = format!("{WILD}import static org.acme.Util.*;\nimport b.*;\n");
        assert_eq!(served_with(&ambiguous, ROUTE, &other), POST);
    }

    #[test]
    fn a_static_wildcard_import_of_the_enum_binds_its_members() {
        let imports = format!(
            "{IMPORTS}import static org.springframework.web.bind.annotation.RequestMethod.*;\n"
        );
        let found = controller(
            "",
            "@RequestMapping(value = \"/x\", method = GET) void f() {}",
        );
        assert!(found.is_empty(), "without the import nothing binds GET");
        let found = served(
            &imports,
            "@RestController class C { @RequestMapping(value = \"/x\", method = {GET, POST}) void f() {} }",
        );
        assert_eq!(found, ["http::GET::/x", "http::POST::/x"]);
        let other = format!("{IMPORTS}import static org.acme.Methods.*;\n");
        let body =
            "@RestController class C { @RequestMapping(value = \"/x\", method = GET) void f() {} }";
        assert!(served(&other, body).is_empty());
    }

    #[test]
    fn a_list_with_an_unreadable_element_serves_the_readable_ones() {
        let found = controller(
            "",
            "@RequestMapping(value = {\"/a\", Api.NOT_A_CONSTANT}, method = {RequestMethod.GET, Missing.X}) void f() {}",
        );
        assert_eq!(found, ["http::GET::/a"]);
    }

    const JAXRS: &str = "import javax.ws.rs.GET;\nimport javax.ws.rs.Path;\n";

    fn resource(imports: &str, body: &str) -> Vec<String> {
        served(imports, body)
    }

    #[test]
    fn a_jax_rs_resource_joins_the_class_path_and_the_method_path() {
        let body = "@Path(\"/items\") class C { @GET @Path(\"{id:[0-9]{3}}\") void f() {} @GET void g() {} }";
        assert_eq!(
            resource(JAXRS, body),
            ["http::GET::/items", "http::GET::/items/{}"]
        );
        let jakarta = JAXRS.replace("javax", "jakarta");
        assert_eq!(resource(&jakarta, body).len(), 2);
    }

    #[test]
    fn each_jax_rs_method_annotation_gives_its_method() {
        let verbs = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
        let imports: String = (verbs.iter())
            .map(|v| format!("import jakarta.ws.rs.{v};\n"))
            .collect::<String>()
            + "import jakarta.ws.rs.Path;\n";
        let methods: String = (verbs.iter())
            .map(|v| format!("@{v} @Path(\"/{v}\") void m{v}() {{}} "))
            .collect();
        let found = resource(&imports, &format!("@Path(\"/r\") class C {{ {methods} }}"));
        let mut want: Vec<String> = (verbs.iter())
            .map(|v| format!("http::{v}::/r/{v}"))
            .collect();
        want.sort();
        assert_eq!(found, want);
    }

    #[test]
    fn jax_rs_paths_fold_constants() {
        let body = "@Path(Api.BASE + \"/x\") class C { @GET @Path(Api.ID) void f() {} }";
        let api = [(
            "a/Api.java",
            "package a; class Api { static final String BASE = \"/b\"; static final String ID = \"{id}\"; }",
        )];
        assert_eq!(served_with(JAXRS, body, &api), ["http::GET::/b/x/{}"]);
    }

    #[test]
    fn jax_rs_names_count_only_through_an_import_and_a_resource_class() {
        let body = "@Path(\"/x\") class C { @GET void f() {} }";
        // A package that is no JAX-RS, a shadowing type, no class @Path, no import at all.
        let other = JAXRS.replace("javax.ws.rs.GET", "org.acme.GET");
        assert!(resource(&other, body).is_empty());
        let shadow = [("a/GET.java", "package a; class GET {}")];
        // A single-type import wins over the package's own class; an on-demand one does not.
        assert_eq!(served_with(JAXRS, body, &shadow), ["http::GET::/x"]);
        let wild = "import javax.ws.rs.*;\n";
        assert!(served_with(wild, body, &shadow).is_empty());
        let nested =
            "import javax.ws.rs.*;\n@Path(\"/x\") class C { class GET {} @GET void f() {} }";
        assert!(resource("", nested).is_empty());
        let single = format!("{JAXRS}import org.acme.GET;\n");
        assert!(resource(&single, body).is_empty());
        assert!(resource(JAXRS, "class C { @GET void f() {} }").is_empty());
        assert!(resource("", body).is_empty());
        // A method with only @Path is a sub-resource locator, not a route.
        assert!(resource(JAXRS, "@Path(\"/x\") class C { @Path(\"/y\") void f() {} }").is_empty());
    }

    #[test]
    fn a_jax_rs_interface_is_a_client_and_serves_nothing() {
        let client = "@Path(\"/users\") interface I { @GET void f(); }";
        assert!(resource(JAXRS, client).is_empty());
        let nested = "@Path(\"/x\") class C { @GET void g() {} @Path(\"/in\") interface I { @GET void f(); } }";
        assert_eq!(resource(JAXRS, nested), ["http::GET::/x"]);
    }

    #[test]
    fn a_jax_rs_wildcard_binds_through_one_package_only() {
        let body = "@Path(\"/x\") class C { @GET void f() {} }";
        assert_eq!(resource("import javax.ws.rs.*;\n", body), ["http::GET::/x"]);
        assert_eq!(
            resource("import jakarta.ws.rs.*;\n", body),
            ["http::GET::/x"]
        );
        let both = "import javax.ws.rs.*;\nimport jakarta.ws.rs.*;\n";
        assert!(resource(both, body).is_empty());
    }

    #[test]
    fn application_path_is_not_applied() {
        let imports = format!(
            "{JAXRS}import javax.ws.rs.ApplicationPath;\nimport javax.ws.rs.core.Application;\n"
        );
        let body = "@ApplicationPath(\"/api\") class App extends Application {} \
            @Path(\"/x\") class C { @GET void f() {} }";
        assert_eq!(resource(&imports, body), ["http::GET::/x"]);
        let nested =
            "class App { @Path(\"/app\") static class P { @GET @Path(\"/y\") void f() {} } }";
        assert_eq!(resource(&imports, nested), ["http::GET::/app/y"]);
    }
}
