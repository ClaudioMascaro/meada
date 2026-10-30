//! Item macros (#398): a `macro_rules!` of the repository whose first arm takes items
//! (`$($x:item)*`) and writes each one back behind outer attributes only
//! (`$( #[cfg(feature = "rt")] $x )*`). A braced invocation of one in a module is read as the
//! module's items, under the `cfg` attributes the arm writes on each. Any other macro, and the
//! bodies of the definitions themselves, stay unread.
//!
//! A definition is found by name from the call site, among every file of the repository
//! (`LanguagePack::extend`): a name qualifies only when all its definitions do, with the same
//! attributes. A file's setting carries the ones it invokes, so `parse` reads one file alone.

use std::collections::{BTreeMap, HashMap};

use tree_sitter::{Node, Parser, Range, Tree};

use super::field;
use crate::Result;
use crate::lang::{Facts, children, named, text};

/// How many item macros deep a body is read; a deeper invocation is skipped, as any other macro.
// ponytail: tokio nests three (`cfg_macros! { cfg_rt! { cfg_not_rt_multi_thread! { .. } } }`);
// each level parses its body again, so the limit bounds how often one byte is parsed.
pub(super) const ITEM_MACRO_DEPTH: usize = 8;

/// How many bytes the attributes one arm writes may take, as tokens; a definition whose
/// attributes take more is no item macro. Each file that invokes it stores them in its setting.
// ponytail: real arms write one `cfg` and one `cfg_attr` of a few dozen bytes; a crafted
// predicate of a megabyte must not be copied into the setting of every file that invokes it.
pub(super) const ATTRIBUTE_BYTES: usize = 1024;

/// The item macros a file's setting carries (`lang::extra`), by name: the predicate of each
/// `cfg` attribute the arm writes on every item, as tokens.
pub(super) type Table<'s> = HashMap<&'s str, Vec<&'s str>>;

/// The braced bodies of the item macros a file invokes in a module, each parsed on its own over
/// the file's bytes (so spans are the file's), by the start of the invocation. A body that does
/// not parse as items is none.
pub(super) type Bodies = Vec<(usize, Tree)>;

/// Reads what `LanguagePack::extend` needs of a file, whatever its setting: the `macro_rules!`
/// it defines (`Facts::macro_rules`) and the macros it invokes with braces (`Facts::invokes`),
/// inside macro tokens too. One pass over the tree.
pub(super) fn record(root: Node, src: &[u8], facts: &mut Facts) {
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        match node.kind() {
            "macro_definition" => {
                if let Some(name) = field(node, "name") {
                    put(facts, text(name, src), passes(node, src));
                }
            }
            "macro_invocation" if braced(node).is_some() => {
                if let Some(name) = field(node, "macro").and_then(|m| last_name(m, src)) {
                    facts.invokes.insert(name.to_string());
                }
            }
            "token_tree" => {
                let tokens: Vec<Node> = children(node).collect();
                for at in 0..tokens.len() {
                    let (Some(name), Some(bang), Some(next)) =
                        (tokens.get(at), tokens.get(at + 1), tokens.get(at + 2))
                    else {
                        break;
                    };
                    if name.kind() != "identifier" || bang.kind() != "!" {
                        continue;
                    }
                    match text(*name, src) {
                        // A definition written in tokens is one this module cannot read.
                        "macro_rules" if next.kind() == "identifier" => {
                            put(facts, text(*next, src), None)
                        }
                        name if next.kind() == "token_tree" && opens(*next, "{") => {
                            facts.invokes.insert(name.to_string());
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

/// Adds one definition of `name` to the file's.
fn put(facts: &mut Facts, name: &str, found: Option<Vec<String>>) {
    agree(&mut facts.macro_rules, name.to_string(), found);
}

/// Adds one definition of `name` to `defined`: two that disagree are no item macro.
fn agree<K: Ord, V: PartialEq>(defined: &mut BTreeMap<K, Option<V>>, name: K, found: Option<V>) {
    match defined.get(&name) {
        Some(known) if *known != found => {
            defined.insert(name, None);
        }
        Some(_) => {}
        None => {
            defined.insert(name, found);
        }
    }
}

/// The name a macro path ends with: `m` of `m!` and of `a::m!`.
fn last_name<'s>(path: Node, src: &'s [u8]) -> Option<&'s str> {
    match path.kind() {
        "identifier" => Some(text(path, src)),
        "scoped_identifier" => field(path, "name").map(|n| text(n, src)),
        _ => None,
    }
}

/// Whether a token tree opens with `delimiter`.
fn opens(tree: Node, delimiter: &str) -> bool {
    tree.child(0).is_some_and(|c| c.kind() == delimiter)
}

/// The token tree of an invocation written with braces.
fn braced(invocation: Node) -> Option<Node> {
    named(invocation).find(|n| n.kind() == "token_tree" && opens(*n, "{"))
}

/// The outer attributes the first arm of a `macro_rules!` writes on each item, when it is
/// `$($x:item)*` and writes back only `$( #[..] .. $x )*`; `None` for any other definition.
fn passes(definition: Node, src: &[u8]) -> Option<Vec<String>> {
    let rule = named(definition).find(|n| n.kind() == "macro_rule")?;
    let matcher = only(field(rule, "left")?, "token_repetition_pattern")?;
    let parts: Vec<Node> = children(matcher).collect();
    let kinds: Vec<&str> = parts.iter().map(|n| n.kind()).collect();
    if kinds != ["$", "(", "token_binding_pattern", ")", "*"] || separated(parts[3], parts[4], src)
    {
        return None;
    }
    let binding = parts[2];
    let fragment = field(binding, "type").map(|t| text(t, src));
    let bound = field(binding, "name").map(|n| text(n, src))?;
    if fragment != Some("item") {
        return None;
    }
    let transcriber = only(field(rule, "right")?, "token_repetition")?;
    let parts: Vec<Node> = children(transcriber).collect();
    let [dollar, open, between @ .., item, close, star] = &parts[..] else {
        return None;
    };
    let ends = [(dollar, "$"), (open, "("), (close, ")"), (star, "*")];
    if ends.iter().any(|(n, kind)| n.kind() != *kind)
        || item.kind() != "metavariable"
        || text(*item, src) != bound
        || between.len() % 2 != 0
        || separated(*close, *star, src)
    {
        return None;
    }
    let mut attributes = Vec::new();
    for pair in between.chunks(2) {
        let [hash, tree] = pair else {
            return None;
        };
        if hash.kind() != "#" || tree.kind() != "token_tree" || !opens(*tree, "[") {
            return None;
        }
        attributes.push(attribute(*tree, src)?);
    }
    let bytes: usize = attributes.iter().map(String::len).sum();
    (bytes <= ATTRIBUTE_BYTES).then_some(attributes)
}

/// Whether a repetition writes a separator between its `)` and its operator, which the grammar
/// leaves out of the tree (`$($i:item),*`).
fn separated(close: Node, operator: Node, src: &[u8]) -> bool {
    let between = &src[close.end_byte()..operator.start_byte()];
    !between.iter().all(u8::is_ascii_whitespace)
}

/// The one node of `kind` between a token tree's delimiters, and nothing else.
fn only<'t>(tree: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let parts: Vec<Node> = children(tree).collect();
    match parts[..] {
        [_, inner, _] if inner.kind() == kind => Some(inner),
        _ => None,
    }
}

/// An attribute's `[..]` as its tokens joined by spaces, without the brackets: `cfg ( feature =
/// "rt" )`. `None` for one the items would be read wrong under: `#[path]`, a metavariable (the
/// arm rewrites the attribute) or a control character (it would break the setting's lines).
fn attribute(tree: Node, src: &[u8]) -> Option<String> {
    let mut tokens = Vec::new();
    let mut stack: Vec<Node> = children(tree).collect();
    // Without the brackets themselves.
    if stack.len() < 2 {
        return None;
    }
    stack.pop();
    stack.remove(0);
    stack.reverse();
    while let Some(node) = stack.pop() {
        let whole = matches!(
            node.kind(),
            "string_literal" | "raw_string_literal" | "char_literal"
        );
        if whole || node.child_count() == 0 {
            tokens.push(text(node, src));
            continue;
        }
        stack.extend(children(node).collect::<Vec<_>>().into_iter().rev());
    }
    let unread = |t: &&str| t.starts_with('$') || t.chars().any(char::is_control);
    if tokens.first() == Some(&"path") || tokens.iter().any(unread) {
        return None;
    }
    Some(tokens.join(" "))
}

/// The predicate of a `cfg` attribute as `attribute` spells it: `( feature = "rt" )`.
fn predicate(attribute: &str) -> Option<&str> {
    attribute
        .strip_prefix("cfg ")
        .filter(|p| p.starts_with('('))
}

/// What each file's setting carries of the item macros it invokes (`LanguagePack::extend`): a
/// line per macro, by name, with the predicate of each `cfg` attribute its arm writes after a
/// tab. A name qualifies when every file that defines it agrees on the same attributes.
pub(super) fn extend(files: &BTreeMap<&str, &Facts>) -> BTreeMap<String, String> {
    let mut defined: BTreeMap<&str, Option<&[String]>> = BTreeMap::new();
    for (name, found) in files.values().flat_map(|f| &f.macro_rules) {
        agree(&mut defined, name.as_str(), found.as_deref());
    }
    let mut out = BTreeMap::new();
    for (path, facts) in files {
        let lines: Vec<String> = (facts.invokes.iter())
            .filter_map(|name| Some((name, defined.get(name.as_str()).copied()??)))
            .map(|(name, attributes)| {
                let predicates = attributes.iter().filter_map(|a| predicate(a));
                std::iter::once(name.as_str())
                    .chain(predicates)
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect();
        if !lines.is_empty() {
            out.insert(path.to_string(), lines.join("\n"));
        }
    }
    out
}

/// The item macros a setting's extra lines name (`extend`).
pub(super) fn table(extra: &str) -> Table<'_> {
    (extra.lines())
        .filter_map(|line| {
            let mut parts = line.split('\t');
            Some((parts.next().filter(|n| !n.is_empty())?, parts.collect()))
        })
        .collect()
}

/// The bodies of the item macros of `table` that the file at `root` invokes in a module, those
/// written in such bodies too, `ITEM_MACRO_DEPTH` deep. Each body is parsed again; one that does
/// not parse as items is left out, as is an empty one.
pub(super) fn bodies(root: Node, src: &[u8], table: &Table) -> Result<Bodies> {
    let mut out = Vec::new();
    if table.is_empty() {
        return Ok(out);
    }
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
    let mut pending: Vec<(usize, Range, usize)> = (invoked(root, src, table).into_iter())
        .map(|(at, range)| (at, range, 1))
        .collect();
    while let Some((at, range, depth)) = pending.pop() {
        if parser.set_included_ranges(&[range]).is_err() {
            continue;
        }
        let Some(tree) = parser.parse(src, None) else {
            continue;
        };
        if tree.root_node().has_error() {
            continue;
        }
        if depth < ITEM_MACRO_DEPTH {
            let deeper = invoked(tree.root_node(), src, table).into_iter();
            pending.extend(deeper.map(|(at, range)| (at, range, depth + 1)));
        }
        out.push((at, tree));
    }
    Ok(out)
}

/// The invocations of `table`'s macros written with braces among the items of the file or the
/// body at `root`, and of the modules written in place in it: each one's start and the range
/// between its braces.
fn invoked(root: Node, src: &[u8], table: &Table) -> Vec<(usize, Range)> {
    let mut out = Vec::new();
    let mut containers = vec![root];
    while let Some(container) = containers.pop() {
        for item in named(container) {
            match item.kind() {
                "mod_item" => containers.extend(field(item, "body")),
                "macro_invocation" => {
                    let name = field(item, "macro").filter(|m| m.kind() == "identifier");
                    if !name.is_some_and(|n| table.contains_key(text(n, src))) {
                        continue;
                    }
                    let Some(tree) = braced(item) else {
                        continue;
                    };
                    let (Some(open), Some(close)) =
                        (tree.child(0), tree.child(tree.child_count() - 1))
                    else {
                        continue;
                    };
                    if close.kind() != "}" || open.end_byte() >= close.start_byte() {
                        continue;
                    }
                    let range = Range {
                        start_byte: open.end_byte(),
                        end_byte: close.start_byte(),
                        start_point: open.end_position(),
                        end_point: close.start_position(),
                    };
                    out.push((item.start_byte(), range));
                }
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::rust::Rust;
    use crate::lang::{Imported, LanguagePack};

    const CFG_RT: &str = "macro_rules! cfg_rt { ($($i:item)*) => { $($i)* } }\n";

    /// Each file of `sources` parsed under `base` and what `extend` adds from all of them, each
    /// first parsed alone, as the index settles them.
    fn indexed(sources: &[(&str, &str)], base: &str) -> BTreeMap<String, Facts> {
        let alone: BTreeMap<&str, Facts> = (sources.iter())
            .map(|(path, src)| (*path, Rust.parse_with(path, src.as_bytes(), base).unwrap()))
            .collect();
        let extra = Rust.extend(&alone.iter().map(|(p, f)| (*p, f)).collect());
        (sources.iter())
            .map(|(path, src)| {
                let extra = extra.get(*path).map_or("", String::as_str);
                let setting = crate::lang::setting(base, extra);
                let facts = Rust.parse_with(path, src.as_bytes(), &setting).unwrap();
                (path.to_string(), facts)
            })
            .collect()
    }

    fn one(src: &str, base: &str) -> Facts {
        indexed(&[("src/lib.rs", src)], base)
            .remove("src/lib.rs")
            .unwrap()
    }

    fn names(facts: &Facts) -> Vec<&str> {
        facts.defs.iter().map(|d| d.name.as_str()).collect()
    }

    fn mods(facts: &Facts) -> Vec<&str> {
        (facts.imports.iter())
            .filter(|i| matches!(i.name, Imported::Namespace))
            .map(|i| i.source.as_str())
            .collect()
    }

    #[test]
    fn the_body_of_an_item_macro_is_read_as_items_of_its_module() {
        let src = format!(
            "{CFG_RT}cfg_rt! {{\n    pub fn spawn() {{}}\n    pub mod rt;\n}}\npub fn caller() {{ spawn(); }}\n"
        );
        let facts = one(&src, "");
        assert_eq!(names(&facts), ["spawn", "rt", "caller"]);
        assert_eq!(mods(&facts), ["mod rt"]);
        let call = facts.calls.iter().find(|c| c.callee == "spawn").unwrap();
        assert_eq!(call.scope, "caller");
        // Spans are the file's own.
        let spawn = &facts.defs[0];
        assert_eq!(&src[spawn.range.clone()], "pub fn spawn() {}");
        assert!(facts.star.takes("spawn") && facts.star.takes("rt"));
    }

    #[test]
    fn a_macro_another_file_defines_is_found_by_name() {
        let found = indexed(
            &[
                ("src/macros.rs", CFG_RT),
                ("src/lib.rs", "cfg_rt! { pub fn spawn() {} }\n"),
                ("src/other.rs", "cfg_net! { pub fn bind() {} }\n"),
            ],
            "",
        );
        assert_eq!(names(&found["src/lib.rs"]), ["spawn"]);
        // `cfg_net` is defined nowhere: a macro of another crate.
        assert!(found["src/other.rs"].defs.is_empty());
    }

    #[test]
    fn a_macro_that_does_not_pass_items_through_leaves_its_body_out() {
        let shapes = [
            // Another matcher, a separator, another repetition, another first arm.
            "macro_rules! m { ($($t:tt)*) => { $($t)* } }",
            "macro_rules! m { ($($i:item),*) => { $($i)* } }",
            "macro_rules! m { ($($i:item)+) => { $($i)* } }",
            "macro_rules! m { () => {}; ($($i:item)*) => { $($i)* } }",
            "macro_rules! m { ($($i:expr)*) => { $($i)* } }",
            // A transcriber that wraps, rewrites or adds to the items.
            "macro_rules! m { ($($i:item)*) => { mod w { $($i)* } } }",
            "macro_rules! m { ($($i:item)*) => { $($i)* fn more() {} } }",
            "macro_rules! m { ($($i:item)*) => { $( #[path = \"x.rs\"] $i )* } }",
            "macro_rules! m { ($($i:item)*) => { $( #![allow(x)] $i )* } }",
            "macro_rules! m { ($($i:item)*) => { $($i)+ } }",
            "macro_rules! m { ($($i:item)*) => { $( pub $i )* } }",
            "macro_rules! m { ($($i:item)*) => { $($j)* } }",
            // Two definitions that disagree, one of them in another macro's tokens.
            "macro_rules! m { ($($i:item)*) => { $($i)* } }\nmacro_rules! m { ($($i:item)*) => { $( #[cfg(unix)] $i )* } }",
            "macro_rules! m { ($($i:item)*) => { $($i)* } }\nother! { macro_rules! m { () => {} } }",
        ];
        for shape in shapes {
            let facts = one(
                &format!("{shape}\nm! {{ pub fn hidden() {{}} pub mod gone; }}\n"),
                "",
            );
            assert!(facts.defs.is_empty(), "{shape}: {:?}", names(&facts));
            assert!(mods(&facts).is_empty(), "{shape}");
        }
        // Parentheses, a path, and a body that is no list of items.
        for call in [
            "m!(pub fn hidden() {});",
            "crate::m! { pub fn hidden() {} }",
            "m! { pub fn hidden() {} + }",
        ] {
            let mut src = CFG_RT.replace("cfg_rt", "m");
            src += call;
            assert!(one(&src, "").defs.is_empty(), "{call}");
        }
    }

    #[test]
    fn the_attributes_on_an_invocation_gate_its_items_first() {
        let src = format!(
            "{CFG_RT}#[cfg(test)]\ncfg_rt! {{ mod checks; fn helper() {{}} }}\n#[cfg(windows)]\ncfg_rt! {{ pub fn win() {{}} }}\n#[cfg(not(test))]\ncfg_rt! {{ pub fn real() {{}} mod built; }}\n"
        );
        let facts = one(&src, "");
        // A test build compiles `checks`, as it would a `#[cfg(test)] mod checks;`.
        assert_eq!(names(&facts), ["checks", "real", "built"]);
        let not_test = format!("{CFG_RT}#[cfg(not(test))]\ncfg_rt! {{ #[cfg(test)] mod unit; }}\n");
        assert!(mods(&one(&not_test, "")).is_empty());
    }

    #[test]
    fn attributes_past_the_budget_are_no_item_macro() {
        let arm = |n: usize| {
            let names = vec!["a"; n].join(", ");
            format!(
                "macro_rules! m {{ ($($i:item)*) => {{ $( #[cfg(not(all({names}, unix)))] $i )* }} }}\nm! {{ pub fn f() {{}} }}\n"
            )
        };
        assert_eq!(names(&one(&arm(8), "")), ["f"]);
        assert!(one(&arm(ATTRIBUTE_BYTES), "").defs.is_empty());
    }

    #[test]
    fn the_definitions_themselves_stay_unread() {
        let src =
            format!("{CFG_RT}macro_rules! wrap {{ () => {{ cfg_rt! {{ fn inside() {{}} }} }} }}\n");
        assert!(one(&src, "").defs.is_empty());
    }

    #[test]
    fn the_attributes_the_arm_writes_gate_every_item() {
        let rt = "macro_rules! cfg_rt { ($($item:item)*) => { $( #[cfg(feature = \"rt\")] #[cfg_attr(docsrs, doc(cfg(feature = \"rt\")))] $item )* } }\n";
        let not_rt = "macro_rules! cfg_not_rt { ($($item:item)*) => { $( #[cfg(not(feature = \"rt\"))] $item )* } }\n";
        let src = format!(
            "{rt}{not_rt}cfg_rt! {{ pub fn on() {{}} #[cfg(windows)] pub fn win() {{}} pub mod runtime; }}\ncfg_not_rt! {{ pub fn off() {{}} }}\n"
        );
        let without = one(&src, "default");
        assert_eq!(names(&without), ["off"]);
        assert!(mods(&without).is_empty());
        let with = one(&src, "default,rt");
        assert_eq!(names(&with), ["on", "runtime"]);
        assert_eq!(mods(&with), ["mod runtime"]);
    }

    #[test]
    fn a_test_module_the_arm_gates_is_indexed_as_a_written_one() {
        let src = "macro_rules! cfg_test { ($($i:item)*) => { $( #[cfg(test)] $i )* } }\ncfg_test! { mod checks; fn helper() {} }\n";
        let facts = one(src, "");
        assert_eq!(mods(&facts), ["mod checks"]);
        assert_eq!(names(&facts), ["checks"]);
    }

    #[test]
    fn definitions_in_several_files_qualify_when_they_agree() {
        let gated = |attr: &str| {
            format!("macro_rules! cfg_rt {{ ($($i:item)*) => {{ $( #[{attr}] $i )* }} }}\n")
        };
        let read = |a: &str, b: &str| {
            let found = indexed(
                &[
                    ("a/src/macros.rs", a),
                    ("b/src/macros.rs", b),
                    ("a/src/lib.rs", "cfg_rt! { pub fn spawn() {} }\n"),
                ],
                "",
            );
            names(&found["a/src/lib.rs"]).len()
        };
        let unix = gated("cfg(unix)");
        // The same attributes, written with other spaces.
        assert_eq!(read(&unix, &gated("cfg( unix )")), 1);
        assert_eq!(read(&unix, &gated("cfg(windows)")), 0);
        assert_eq!(
            read(&unix, "macro_rules! cfg_rt { ($($t:tt)*) => {} }\n"),
            0
        );
    }

    #[test]
    fn nested_item_macros_are_read_to_the_depth_limit() {
        let nest = |n: usize| {
            let mut src = CFG_RT.to_string();
            for level in 0..n {
                src += &format!("cfg_rt! {{ pub fn f{level}() {{}} ");
            }
            src + &"}".repeat(n) + "\n"
        };
        let facts = one(&nest(ITEM_MACRO_DEPTH), "");
        assert_eq!(facts.defs.len(), ITEM_MACRO_DEPTH);
        let facts = one(&nest(ITEM_MACRO_DEPTH + 3), "");
        assert_eq!(facts.defs.len(), ITEM_MACRO_DEPTH);
    }

    #[test]
    fn a_module_written_in_place_reads_its_item_macros_too() {
        let src = format!("{CFG_RT}pub mod inner {{ cfg_rt! {{ pub fn spawn() {{}} }} }}\n");
        assert_eq!(names(&one(&src, "")), ["inner", "inner.spawn"]);
        // A function body is no module.
        let src = format!("{CFG_RT}fn outer() {{ cfg_rt! {{ fn spawn() {{}} }} }}\n");
        assert_eq!(names(&one(&src, "")), ["outer"]);
    }

    #[test]
    fn what_extend_reads_no_setting_changes() {
        let src = format!(
            "{CFG_RT}#[cfg(windows)]\nmacro_rules! hidden {{ ($($i:item)*) => {{ $($i)* }} }}\ncfg_rt! {{ cfg_net! {{ pub mod net; }} }}\n"
        );
        let alone = Rust.parse_with("src/lib.rs", src.as_bytes(), "").unwrap();
        let settled = one(&src, "default,net");
        assert_eq!(alone.macro_rules, settled.macro_rules);
        assert_eq!(alone.invokes, settled.invokes);
        assert!(alone.macro_rules["hidden"].is_some());
        let invoked: Vec<&str> = alone.invokes.iter().map(String::as_str).collect();
        assert_eq!(invoked, ["cfg_net", "cfg_rt"]);
    }
}
