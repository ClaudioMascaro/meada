//! The language conformance suite (ADR 0007): a language pack is done when the export of its
//! conformance fixture holds exactly the expected definitions, every expected call at its
//! expected level, and matches its golden file. A new pack adds a fixture under
//! `tests/fixtures/conformance-<language>/` and one test calling `conform`.

mod common;

use common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

const ORIGIN: &str = "https://example.com/acme/conformance.git";
const REPO: &str = "example.com/acme/conformance";

/// An expected call site: its id and level, and its targets (one for `exact` and `inferred`,
/// every candidate for `ambiguous`, none when the call stays unresolved). Ids are relative to
/// the repository.
type ExpectedCall<'a> = (&'a str, Option<&'a str>, &'a [&'a str]);

const EXACT: Option<&str> = Some("exact");
const INFERRED: Option<&str> = Some("inferred");
const AMBIGUOUS: Option<&str> = Some("ambiguous");

fn conform(fixture: &str, symbols: &[&str], calls: &[ExpectedCall]) {
    let repo = fixture_repo(&format!("conformance_{fixture}"), fixture, Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let id = |rel: &str| format!("{REPO}::{rel}");

    let actual: Vec<&str> = json["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    let mut expected: Vec<String> = symbols.iter().map(|s| id(s)).collect();
    expected.sort();
    assert_eq!(actual, expected, "definitions of {fixture}");

    for (call, level, targets) in calls {
        let site = json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id(call))
            .unwrap_or_else(|| panic!("no call {call} in {fixture}"));
        let mut found: Vec<&str> = site["target"].as_str().into_iter().collect();
        if let Some(candidates) = site["candidates"].as_array() {
            found.extend(candidates.iter().map(|c| c.as_str().unwrap()));
        }
        let wanted: Vec<String> = targets.iter().map(|t| id(t)).collect();
        assert_eq!(
            (site["level"].as_str(), found),
            (*level, wanted.iter().map(String::as_str).collect()),
            "call {call} in {fixture}"
        );
    }

    assert_golden(&format!("{fixture}.export.json"), &export);
}

#[test]
fn typescript_and_javascript() {
    conform(
        "conformance-ts-js",
        &[
            "src/app.ts::main",
            "src/legacy.cjs::legacy",
            "src/math.ts::add",
            "src/math.ts::double",
            "src/math.ts::scale",
            "src/math.ts::scale.times",
            "src/math.ts::square",
            "src/shapes/circle.ts::Circle",
            "src/shapes/circle.ts::Circle.area",
            "src/shapes/circle.ts::Circle.constructor",
            "src/shapes/shape.ts::Shape",
            "src/shapes/shape.ts::Shape.area",
            "src/shapes/shape.ts::Shape.describe",
            "src/shapes/shape.ts::Shape.unit",
            "src/util/format.mjs::format",
            "src/util/log.js::default",
            "src/view.tsx::render",
            "src/widget.jsx::render",
        ],
        &[
            // In-file scope: top level, nested function, `this.method` inside a class.
            ("src/math.ts::double::add#0", EXACT, &["src/math.ts::add"]),
            (
                "src/math.ts::scale::double#0",
                EXACT,
                &["src/math.ts::double"],
            ),
            (
                "src/math.ts::scale::times#0",
                EXACT,
                &["src/math.ts::scale.times"],
            ),
            (
                "src/shapes/shape.ts::Shape.describe::this.area#0",
                EXACT,
                &["src/shapes/shape.ts::Shape.area"],
            ),
            // Imports: named through `../` without extension, named with alias, default.
            (
                "src/shapes/circle.ts::Circle.area::square#0",
                EXACT,
                &["src/math.ts::square"],
            ),
            ("src/app.ts::main::plus#0", EXACT, &["src/math.ts::add"]),
            (
                "src/app.ts::main::double#0",
                EXACT,
                &["src/math.ts::double"],
            ),
            ("src/app.ts::main::scale#0", EXACT, &["src/math.ts::scale"]),
            // A namespace import whose `.js` extension names the `.ts` file.
            (
                "src/app.ts::main::numbers.square#0",
                EXACT,
                &["src/math.ts::square"],
            ),
            // Re-exports through `index` resolution: a namespace, a named one with its
            // extension, a default renamed.
            ("src/app.ts::main::math.add#0", EXACT, &["src/math.ts::add"]),
            (
                "src/app.ts::main::util.format#0",
                EXACT,
                &["src/util/format.mjs::format"],
            ),
            (
                "src/app.ts::main::log#0",
                EXACT,
                &["src/util/log.js::default"],
            ),
            // The `@/*` alias of `tsconfig.json`, to an `index` file re-exporting a class, then
            // its static method; and from a TSX file.
            (
                "src/app.ts::main::Shape.unit#0",
                EXACT,
                &["src/shapes/shape.ts::Shape.unit"],
            ),
            (
                "src/view.tsx::render::format#0",
                EXACT,
                &["src/util/format.mjs::format"],
            ),
            // A package outside the repository stays unresolved.
            ("src/app.ts::main::useState#0", None, &[]),
            // Only a unique name match finds these: an inherited static method, a method on
            // a parameter, a function no import names.
            (
                "src/app.ts::main::Circle.unit#0",
                INFERRED,
                &["src/shapes/shape.ts::Shape.unit"],
            ),
            (
                "src/app.ts::main::shape.describe#0",
                INFERRED,
                &["src/shapes/shape.ts::Shape.describe"],
            ),
            (
                "src/app.ts::main::legacy#0",
                INFERRED,
                &["src/legacy.cjs::legacy"],
            ),
            // Several definitions fit: all are listed.
            (
                "src/app.ts::main::shape.area#0",
                AMBIGUOUS,
                &[
                    "src/shapes/circle.ts::Circle.area",
                    "src/shapes/shape.ts::Shape.area",
                ],
            ),
            (
                "src/app.ts::main::render#0",
                AMBIGUOUS,
                &["src/view.tsx::render", "src/widget.jsx::render"],
            ),
            // No definition has the name.
            ("src/util/log.js::default::console.log#0", None, &[]),
        ],
    );
}

#[test]
fn python() {
    conform(
        "conformance-python",
        &[
            "families.py::F",
            "families.py::E",
            "families.py::E.pick",
            "families.py::D",
            "families.py::D.pick",
            "families.py::C",
            "families.py::B",
            "families.py::A",
            "families.py::A.go",
            "families.py::Tangled",
            "families.py::Tangled.go",
            "families.py::Stored",
            "families.py::Stored.save",
            "families.py::Model",
            "families.py::Model.go",
            "families.py::Client",
            "families.py::Remote",
            "families.py::Remote.go",
            "families.py::Plain",
            "families.py::Plain.keep",
            "families.py::Plain.go",
            "families.py::Lone",
            "families.py::Lone.go",
            "families.py::Lone.make",
            "receivers.py::Engine",
            "receivers.py::Engine.start",
            "receivers.py::Untyped",
            "receivers.py::Untyped.__init__",
            "receivers.py::Untyped.run",
            "receivers.py::Typed",
            "receivers.py::Typed.__init__",
            "receivers.py::Typed.run",
            "receivers.py::Runner",
            "receivers.py::Runner.stride",
            "receivers.py::Runner.each",
            "receivers.py::module_read",
            "receivers.py::with_as",
            "receivers.py::comprehension",
            "receivers.py::walrus",
            "receivers.py::except_as",
            "receivers.py::lambdas",
            "receivers.py::param_reassigned",
            "receivers.py::swap",
            "receivers.py::spare_read",
            "receivers.py::fallback",
            "receivers.py::lacking",
            "receivers.py::optional",
            "cycles.py::Ping",
            "cycles.py::Ping.go",
            "cycles.py::Pong",
            "cycles.py::Pong.ring",
            "cycles.py::Knot",
            "cycles.py::Knot.go",
            "cycles.py::Loop",
            "cycles.py::Loop.Inner",
            "cycles.py::Deep",
            "cycles.py::Deep.go",
            "cycles.py::crossed",
            "cycle_b.py::use",
            "bindings.py::Line",
            "bindings.py::Line.dial",
            "bindings.py::Unpacked",
            "bindings.py::Unpacked.__init__",
            "bindings.py::Unpacked.call",
            "bindings.py::Looped",
            "bindings.py::Looped.__init__",
            "bindings.py::Looped.call",
            "bindings.py::Opened",
            "bindings.py::Opened.__init__",
            "bindings.py::Opened.call",
            "bindings.py::Reset",
            "bindings.py::Reset.call",
            "bindings.py::Reset.call.reset",
            "bindings.py::Outside",
            "bindings.py::Outside.call",
            "bindings.py::poke",
            "bindings.py::nested",
            "bindings.py::nested.mid",
            "bindings.py::nested.mid.inner",
            "bindings.py::factory",
            "bindings.py::factory.Inner",
            "bindings.py::factory.Inner.reset",
            "bindings.py::matched",
            "bindings.py::imported",
            "bindings.py::lazy",
            "bindings.py::Left",
            "bindings.py::Left.left",
            "bindings.py::Right",
            "bindings.py::Right.right",
            "bindings.py::Both",
            "bindings.py::Both.go",
            "bindings.py::ObjectLeft",
            "bindings.py::ObjectRight",
            "bindings.py::ObjectRight.side",
            "bindings.py::ObjectBoth",
            "bindings.py::ObjectBoth.go",
            "bindings.py::mixin",
            "bindings.py::Store",
            "bindings.py::Store.stash",
            "bindings.py::Mixed",
            "bindings.py::Mixed.go",
            "defs.py::Registry",
            "defs.py::Registry.Meta",
            "defs.py::Registry.Meta.label",
            "defs.py::Registry.add",
            "defs.py::Registry.build",
            "defs.py::Registry.load",
            "defs.py::Registry.tagged",
            "defs.py::Registry.unit",
            "defs.py::decorated",
            "defs.py::fetch_all",
            "defs.py::outer",
            "defs.py::outer.inner",
            "defs.py::plain",
            "defs.py::register",
            "defs.py::register.wrapper",
            "defs.py::sorted_names",
            "explicit/core.py::default",
            "explicit/early.py::kept",
            "explicit/early.py::swept",
            "explicit/early.py::taken",
            "explicit/first.py::pick",
            "explicit/letters.py::alpha",
            "explicit/letters.py::beta",
            "explicit/mod.py::_hidden",
            "explicit/mod.py::outside",
            "explicit/mod.py::public",
            "explicit/order.py::order",
            "explicit/second.py::pick",
            "explicit/sweeper.py::swept",
            "explicit/use.py::report",
            "explicit/util.py::get",
            "geometry/extra/measure.py::perimeter",
            "geometry/shapes.py::Circle",
            "geometry/shapes.py::Circle.__init__",
            "geometry/shapes.py::Circle.area",
            "geometry/shapes.py::Circle.describe",
            "geometry/shapes.py::Shape",
            "geometry/shapes.py::Shape.area",
            "geometry/shapes.py::Shape.unit",
            "geometry/shapes.py::area_of",
            "geometry/shapes.py::scale",
            "geometry/units.py::to_cm",
            "helpers.py::clamp",
            "helpers.py::format_name",
            "legacy.py::legacy",
            "main.py::main",
            "methods.py::Base",
            "methods.py::Base.ping",
            "methods.py::Base.shared",
            "methods.py::Dial",
            "methods.py::Dial.spin",
            "methods.py::Leaf",
            "methods.py::Leaf.run",
            "methods.py::Left",
            "methods.py::Mixin",
            "methods.py::Mixin.mixed",
            "methods.py::Mixin.shared",
            "methods.py::Panel",
            "methods.py::Panel.__init__",
            "methods.py::Panel.build",
            "methods.py::Panel.check",
            "methods.py::Panel.create",
            "methods.py::Panel.draw",
            "methods.py::Panel.draw.later",
            "methods.py::Panel.render",
            "methods.py::Right",
            "methods.py::Right.hook",
            "methods.py::Root",
            "methods.py::Root.hook",
            "methods.py::make",
            "methods.py::module_level",
            "methods.py::outside",
            "methods.py::shadowed",
            "methods.py::use",
            "scopes.py::Helper",
            "scopes.py::Outer",
            "scopes.py::Outer.Helper",
            "scopes.py::Outer.m",
            "shadow.py::from_module",
            "shadow/__init__.py::from_package",
            "shop/catalog/urls.py::products",
            "shop/catalog/urls.py::upload",
            "shop/orders/items.py::stock",
            "shop/orders/views.py::OrderList",
            "shop/orders/views.py::OrderList.get",
            "shop/orders/views.py::OrderList.post",
            "shop/orders/views.py::OrderList.total",
            "shop/orders/views.py::order",
            "shop/views.py::archive",
            "shop/views.py::health",
            "shop/views.py::vintage",
            "src/billing/invoice.py::total",
            "src/billing/tax.py::fee",
            "src/billing/tax.py::vat",
            "stars/computed.py::named",
            "stars/computed.py::unnamed",
            "stars/literal.py::Shown",
            "stars/literal.py::listed",
            "stars/literal.py::unlisted",
            "stars/underscore.py::_internal",
            "stars/underscore.py::visible",
            "web/accounts.py::update",
            "web/api.py::echo",
            "web/api.py::health",
            "web/api.py::ping",
            "web/factory.py::build_admin",
            "web/factory.py::build_admin.remove_user",
            "web/factory.py::mount",
            "web/factory.py::relay",
            "web/items.py::create_item",
            "web/items.py::read_item",
            "web/pages.py::daily",
            "web/pages.py::fetch",
            "web/pages.py::index",
            "web/pages.py::order",
            "web/pages.py::shortcut",
        ],
        &[
            // In-file scope: a comprehension and a nested function, a nested function called
            // from its parent, a class body, methods, and lambdas, whose calls belong to the
            // enclosing symbol (the module, here, or `sorted_names`).
            ("defs.py::fetch_all::plain#0", EXACT, &["defs.py::plain"]),
            ("defs.py::outer.inner::plain#0", EXACT, &["defs.py::plain"]),
            ("defs.py::outer::inner#0", EXACT, &["defs.py::outer.inner"]),
            ("defs.py::decorated::plain#0", EXACT, &["defs.py::plain"]),
            ("defs.py::Registry::plain#0", EXACT, &["defs.py::plain"]),
            (
                "defs.py::Registry.load::fetch_all#0",
                EXACT,
                &["defs.py::fetch_all"],
            ),
            (
                "defs.py::Registry.unit::plain#0",
                EXACT,
                &["defs.py::plain"],
            ),
            (
                "defs.py::Registry.tagged::plain#0",
                EXACT,
                &["defs.py::plain"],
            ),
            (
                "defs.py::Registry.Meta.label::plain#0",
                EXACT,
                &["defs.py::plain"],
            ),
            ("defs.py::plain#0", EXACT, &["defs.py::plain"]),
            ("defs.py::sorted_names::plain#0", EXACT, &["defs.py::plain"]),
            // Built-ins, a decorator from an imported package, parameters and `self`: no
            // definition fits.
            ("defs.py::register::functools.wraps#0", None, &[]),
            ("defs.py::register.wrapper::func#0", None, &[]),
            ("defs.py::Registry.add::self.items.append#0", None, &[]),
            ("defs.py::Registry.build::cls#0", None, &[]),
            ("defs.py::sorted_names::sorted#0", None, &[]),
            // Relative imports: `from .m import f`, and `from . import m` and `from .. import
            // m`, where `m` is a module of the package.
            (
                "geometry/shapes.py::scale::to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            (
                "geometry/shapes.py::Circle.area::scale#0",
                EXACT,
                &["geometry/shapes.py::scale"],
            ),
            (
                "geometry/shapes.py::Circle.describe::to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            (
                "geometry/extra/measure.py::perimeter::units.to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            (
                "geometry/extra/measure.py::perimeter::area_of#0",
                EXACT,
                &["geometry/shapes.py::area_of"],
            ),
            // `import a.b` binds `a.b` as a whole.
            (
                "geometry/extra/measure.py::perimeter::geometry.shapes.scale#0",
                EXACT,
                &["geometry/shapes.py::scale"],
            ),
            // The `src/` layout, with `from . import tax` and `from .tax import vat`.
            (
                "src/billing/invoice.py::total::vat#0",
                EXACT,
                &["src/billing/tax.py::vat"],
            ),
            (
                "src/billing/invoice.py::total::tax.fee#0",
                EXACT,
                &["src/billing/tax.py::fee"],
            ),
            // Imports through a package: a re-export of a class and of a function, the
            // function as an attribute of the package, a submodule `__init__.py` imports, a
            // deeper module named in full, and an alias of a module.
            (
                "main.py::main::Round#0",
                EXACT,
                &["geometry/shapes.py::Circle"],
            ),
            (
                "main.py::main::area_of#0",
                EXACT,
                &["geometry/shapes.py::area_of"],
            ),
            (
                "main.py::main::geometry.area_of#0",
                EXACT,
                &["geometry/shapes.py::area_of"],
            ),
            (
                "main.py::main::geometry.units.to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            (
                "main.py::main::geometry.extra.measure.perimeter#0",
                EXACT,
                &["geometry/extra/measure.py::perimeter"],
            ),
            (
                "main.py::main::gu.to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            // `from a.b import c as d`, a package re-export from the `src/` layout and a
            // module alias in it.
            (
                "main.py::main::grow#0",
                EXACT,
                &["geometry/shapes.py::scale"],
            ),
            (
                "main.py::main::total#0",
                EXACT,
                &["src/billing/invoice.py::total"],
            ),
            (
                "main.py::main::tax.vat#0",
                EXACT,
                &["src/billing/tax.py::vat"],
            ),
            // `import *` brings in the exports: all public names, only `__all__` when it is
            // a literal, and every public name when it is computed. What it leaves out is
            // found by name alone.
            (
                "main.py::main::format_name#0",
                EXACT,
                &["helpers.py::format_name"],
            ),
            ("main.py::main::clamp#0", EXACT, &["helpers.py::clamp"]),
            (
                "main.py::main::listed#0",
                EXACT,
                &["stars/literal.py::listed"],
            ),
            (
                "main.py::main::Shown#0",
                EXACT,
                &["stars/literal.py::Shown"],
            ),
            (
                "main.py::main::unlisted#0",
                INFERRED,
                &["stars/literal.py::unlisted"],
            ),
            (
                "main.py::main::visible#0",
                EXACT,
                &["stars/underscore.py::visible"],
            ),
            (
                "main.py::main::_internal#0",
                INFERRED,
                &["stars/underscore.py::_internal"],
            ),
            (
                "main.py::main::named#0",
                EXACT,
                &["stars/computed.py::named"],
            ),
            (
                "main.py::main::unnamed#0",
                EXACT,
                &["stars/computed.py::unnamed"],
            ),
            // An explicit import or an attribute of a module sees every name, whatever
            // `__all__` and the underscore prefix say, and a re-export `__all__` leaves out; a
            // star import in a package is found by name too, `default` included.
            (
                "explicit/use.py::report::outside#0",
                EXACT,
                &["explicit/mod.py::outside"],
            ),
            (
                "explicit/use.py::report::_hidden#0",
                EXACT,
                &["explicit/mod.py::_hidden"],
            ),
            (
                "explicit/use.py::report::m.outside#0",
                EXACT,
                &["explicit/mod.py::outside"],
            ),
            (
                "explicit/use.py::report::m._hidden#0",
                EXACT,
                &["explicit/mod.py::_hidden"],
            ),
            (
                "explicit/use.py::report::beta#0",
                EXACT,
                &["explicit/letters.py::beta"],
            ),
            (
                "explicit/use.py::report::default#0",
                EXACT,
                &["explicit/core.py::default"],
            ),
            // Of two imports of one name the later wins, and an explicit import of a package
            // outside the repository is not overridden by a star import.
            (
                "explicit/use.py::report::pick#0",
                EXACT,
                &["explicit/second.py::pick"],
            ),
            ("explicit/use.py::report::get#0", None, &[]),
            // The latest import that binds a name decides: an external one after a resolving
            // one leaves it unresolved, a star import after an explicit one takes the name
            // over when it provides it, and only then.
            ("explicit/order.py::order::taken#0", None, &[]),
            (
                "explicit/order.py::order::swept#0",
                EXACT,
                &["explicit/sweeper.py::swept"],
            ),
            (
                "explicit/order.py::order::kept#0",
                EXACT,
                &["explicit/early.py::kept"],
            ),
            // A package comes before a module of the same name.
            (
                "explicit/use.py::report::from_package#0",
                EXACT,
                &["shadow/__init__.py::from_package"],
            ),
            (
                "explicit/use.py::report::shadow.from_package#0",
                EXACT,
                &["shadow/__init__.py::from_package"],
            ),
            // A package outside the repository stays unresolved.
            ("main.py::main::np.array#0", None, &[]),
            // Only a unique name match finds a function no import names.
            ("main.py::main::legacy#0", INFERRED, &["legacy.py::legacy"]),
            // A local assigned from a constructor call, here through an aliased import.
            (
                "main.py::main::circle.describe#0",
                EXACT,
                &["geometry/shapes.py::Circle.describe"],
            ),
            // `self` and `cls` reach the class, then its bases in the order written, in
            // Python's method resolution order, also from a nested function.
            (
                "methods.py::Panel.draw::self.ping#0",
                EXACT,
                &["methods.py::Base.ping"],
            ),
            (
                "methods.py::Panel.draw::self.shared#0",
                EXACT,
                &["methods.py::Base.shared"],
            ),
            (
                "methods.py::Panel.draw::self.mixed#0",
                EXACT,
                &["methods.py::Mixin.mixed"],
            ),
            (
                "methods.py::Panel.draw::self.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::Panel.draw.later::self.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::Panel.build::cls.create#0",
                EXACT,
                &["methods.py::Panel.create"],
            ),
            (
                "methods.py::Leaf.run::self.hook#0",
                EXACT,
                &["methods.py::Right.hook"],
            ),
            // Attribute annotations, in a method and in the class body.
            (
                "methods.py::Panel.draw::self.shape.area#0",
                EXACT,
                &["geometry/shapes.py::Shape.area"],
            ),
            (
                "methods.py::Panel.draw::self.frame.describe#0",
                EXACT,
                &["geometry/shapes.py::Circle.describe"],
            ),
            // Parameter annotations: a name, `Optional[...]` and a string; a constructor
            // call in the same scope, and at the module level.
            (
                "methods.py::use::panel.draw#0",
                EXACT,
                &["methods.py::Panel.draw"],
            ),
            (
                "methods.py::use::maybe.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::use::ahead.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::use::built.ping#0",
                EXACT,
                &["methods.py::Base.ping"],
            ),
            (
                "methods.py::module_level::current.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            // By name: a method the class family lacks, a static method's parameter, what a
            // function returns, and a local that shadows a typed module-level name.
            (
                "methods.py::Panel.draw::self.spin#0",
                INFERRED,
                &["methods.py::Dial.spin"],
            ),
            (
                "methods.py::Panel.check::value.ping#0",
                INFERRED,
                &["methods.py::Base.ping"],
            ),
            (
                "methods.py::use::made.render#0",
                INFERRED,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::shadowed::current.render#0",
                INFERRED,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::use::made.area#0",
                AMBIGUOUS,
                &[
                    "geometry/shapes.py::Circle.area",
                    "geometry/shapes.py::Shape.area",
                ],
            ),
            // A receiver whose class, or whose value's callee, is outside the repository:
            // what its methods are is not known, and no name match is tried.
            ("methods.py::outside::session.render#0", None, &[]),
            ("methods.py::outside::client.render#0", None, &[]),
            ("methods.py::outside::log.render#0", None, &[]),
            // Python's method resolution order (C3), a hierarchy it rejects, bases outside the
            // repository that come before the class defining a method, and `object`.
            (
                "families.py::A.go::self.pick#0",
                EXACT,
                &["families.py::D.pick"],
            ),
            (
                "families.py::Tangled.go::self.pick#0",
                AMBIGUOUS,
                &["families.py::D.pick", "families.py::E.pick"],
            ),
            (
                "families.py::Model.go::self.save#0",
                INFERRED,
                &["families.py::Stored.save"],
            ),
            (
                "families.py::Remote.go::self.save#0",
                INFERRED,
                &["families.py::Stored.save"],
            ),
            (
                "families.py::Plain.keep::self.go#0",
                EXACT,
                &["families.py::Plain.go"],
            ),
            // `self` and `cls` whose family lacks the method: a name match.
            (
                "families.py::Lone.go::self.pick#0",
                AMBIGUOUS,
                &["families.py::D.pick", "families.py::E.pick"],
            ),
            (
                "families.py::Lone.make::cls.pick#0",
                AMBIGUOUS,
                &["families.py::D.pick", "families.py::E.pick"],
            ),
            // Bindings that hide an outer one: `self.x = ...` without an annotation, a loop
            // variable named `this`, `with`, comprehensions, `:=`, `except`, lambdas, a
            // reassigned parameter, `global` and an import's fallback; an annotation stays
            // stronger, and a single module-level binding is read from a function.
            (
                "receivers.py::Untyped.run::self.engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::Typed.run::self.engine.start#0",
                EXACT,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::Runner.each::this.stride#0",
                INFERRED,
                &["receivers.py::Runner.stride"],
            ),
            (
                "receivers.py::module_read::engine.start#0",
                EXACT,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::with_as::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::comprehension::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::walrus::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::except_as::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::lambdas::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::param_reassigned::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::spare_read::spare.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::fallback::Circle#0",
                INFERRED,
                &["geometry/shapes.py::Circle"],
            ),
            // An annotated receiver whose family lacks the method, and `X | None` both ways.
            (
                "receivers.py::lacking::engine.stride#0",
                INFERRED,
                &["receivers.py::Runner.stride"],
            ),
            (
                "receivers.py::optional::first.start#0",
                EXACT,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::optional::second.start#0",
                EXACT,
                &["receivers.py::Engine.start"],
            ),
            // Bases and annotations that resolve through themselves, in one module or two.
            (
                "cycles.py::Ping.go::self.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycles.py::Knot.go::self.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycles.py::Deep.go::self.x.y.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycles.py::crossed::a.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycles.py::crossed::b.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycle_b.py::use::other.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            // A receiver attribute bound in a compound target or from a closure, a `nonlocal` that
            // skips a class scope, a `case` capture and a function-local import hide an outer
            // binding; a write from outside the class is not seen; a function-local import
            // still resolves.
            (
                "bindings.py::Unpacked.call::self.line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::Looped.call::self.line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::Opened.call::self.line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::Reset.call::self.line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::Outside.call::self.line.dial#0",
                EXACT,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::nested::line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::factory::line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::matched::line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::imported::line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::lazy::scale#0",
                EXACT,
                &["geometry/shapes.py::scale"],
            ),
            // One outside base shared by two classes (`ABC`, `object`) has one place in C3; a base
            // that is not a name hides what comes after it.
            (
                "bindings.py::Both.go::self.right#0",
                EXACT,
                &["bindings.py::Right.right"],
            ),
            (
                "bindings.py::ObjectBoth.go::self.side#0",
                EXACT,
                &["bindings.py::ObjectRight.side"],
            ),
            (
                "bindings.py::Mixed.go::self.stash#0",
                INFERRED,
                &["bindings.py::Store.stash"],
            ),
            // A class body's names are in scope for its own statements, not for its methods.
            (
                "scopes.py::Outer::Helper#0",
                EXACT,
                &["scopes.py::Outer.Helper"],
            ),
            (
                "scopes.py::Outer.m::Helper#0",
                EXACT,
                &["scopes.py::Helper"],
            ),
            // Several definitions fit: a method on a parameter.
            (
                "geometry/shapes.py::area_of::shape.area#0",
                AMBIGUOUS,
                &[
                    "geometry/shapes.py::Circle.area",
                    "geometry/shapes.py::Shape.area",
                ],
            ),
        ],
    );
}

/// The Python conformance fixture's export, and its providers (contract, site) whose site is
/// under `dir`, sorted.
fn python_providers(case: &str, dir: &str) -> (Value, Vec<(String, String)>) {
    let repo = fixture_repo(case, "conformance-python", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let mut providers: Vec<(String, String)> = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let site = p["site"].as_str().unwrap();
            let site = site.strip_prefix(&format!("{REPO}::")).unwrap();
            (p["contract"].as_str().unwrap().into(), site.into())
        })
        .filter(|(_, site): &(String, String)| site.starts_with(dir))
        .collect();
    providers.sort();
    (json, providers)
}

/// `(contract, site)` pairs as owned strings, sorted.
fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = (pairs.iter())
        .map(|(c, s)| (c.to_string(), s.to_string()))
        .collect();
    out.sort();
    out
}

#[test]
fn python_fastapi_and_flask_routes_are_providers_under_every_prefix() {
    let (json, actual) = python_providers("python_http_providers", "web/");
    let expected = [
        // FastAPI: method decorators and `api_route` on the app.
        ("http::GET::/health", "web/api.py::health"),
        ("http::GET::/ping", "web/api.py::ping"),
        ("http::POST::/ping", "web/api.py::ping"),
        ("http::GET::/echo", "web/api.py::echo"),
        // One router under three prefixes: nested in `v1` (its own prefix after the one given
        // when including it), directly, and through a function that returns its argument.
        (
            "http::GET::/api/v1/items/catalog/{}",
            "web/items.py::read_item",
        ),
        (
            "http::POST::/api/v1/items/catalog",
            "web/items.py::create_item",
        ),
        ("http::GET::/legacy/catalog/{}", "web/items.py::read_item"),
        ("http::POST::/legacy/catalog", "web/items.py::create_item"),
        ("http::GET::/relay/catalog/{}", "web/items.py::read_item"),
        ("http::POST::/relay/catalog", "web/items.py::create_item"),
        // A router returned by a function and included by another one, as arguments.
        (
            "http::DELETE::/internal/admin/users/{}",
            "web/factory.py::build_admin.remove_user",
        ),
        // Flask: `route` without methods serves GET; a Blueprint's own prefix, and one replaced
        // on registration. The `get` shortcut yields nothing.
        ("http::GET::/", "web/pages.py::index"),
        ("http::GET::/orders/{}", "web/pages.py::order"),
        ("http::DELETE::/orders/{}", "web/pages.py::order"),
        ("http::GET::/reports/daily/{}", "web/pages.py::daily"),
        ("http::PUT::/members/{}", "web/accounts.py::update"),
    ];
    assert_eq!(actual, pairs(&expected));
    // A Python function named `fetch` is no HTTP consumer.
    assert_eq!(json["consumers"], serde_json::json!([]));
}

#[test]
fn python_django_urlconfs_are_providers_with_the_methods_django_allows() {
    let (_, actual) = python_providers("python_django_providers", "shop/");
    let expected = [
        // Unrestricted function views serve every method.
        ("http::ANY::/health", "shop/views.py::health"),
        // A `re_path()` with a simple named group; the one beyond them yields nothing.
        ("http::ANY::/vintage/{}", "shop/views.py::vintage"),
        // `include()` by dotted name, two levels deep: `require_http_methods`, a class-based
        // view's own handler methods, and `require_safe` reached through its module.
        ("http::GET::/shop/orders/{}", "shop/orders/views.py::order"),
        (
            "http::DELETE::/shop/orders/{}",
            "shop/orders/views.py::order",
        ),
        (
            "http::GET::/shop/orders",
            "shop/orders/views.py::OrderList.get",
        ),
        (
            "http::POST::/shop/orders",
            "shop/orders/views.py::OrderList.post",
        ),
        (
            "http::GET::/shop/items/{}/stock",
            "shop/orders/items.py::stock",
        ),
        (
            "http::HEAD::/shop/items/{}/stock",
            "shop/orders/items.py::stock",
        ),
        // `include()` of an imported module: `require_GET` and `require_POST`.
        (
            "http::GET::/catalog/products",
            "shop/catalog/urls.py::products",
        ),
        (
            "http::POST::/catalog/upload",
            "shop/catalog/urls.py::upload",
        ),
    ];
    assert_eq!(actual, pairs(&expected));
}

/// The levels of `checkout`'s two calls in `python-roots` under a given `pyproject.toml`: the
/// call into `lib/shop` and the call into `src/other`.
fn python_roots(case: &str, pyproject: &str) -> [Option<String>; 2] {
    let repo = fixture_repo(case, "python-roots", Some(ORIGIN));
    std::fs::write(repo.join("pyproject.toml"), pyproject).unwrap();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    ["total", "helper"].map(|callee| {
        let id = format!("{REPO}::app.py::checkout::{callee}#0");
        let calls = json["calls"].as_array().unwrap();
        let call = calls.iter().find(|c| c["id"] == id).unwrap();
        call["level"].as_str().map(String::from)
    })
}

#[test]
fn python_source_roots_come_from_pyproject() {
    let exact = Some("exact".to_string());
    // Nothing declared: `src` is a root, `lib` is not.
    assert_eq!(python_roots("python_roots_none", ""), [None, exact.clone()]);
    // A `package-dir` whose directory is not named after the package, even by its last
    // letters, says nothing about a root.
    assert_eq!(
        python_roots(
            "python_roots_package_dir_suffix",
            "[tool.setuptools]\npackage-dir = { shop = \"libshop\" }\n",
        ),
        [None, exact.clone()]
    );
    // A declared root replaces `src`, in each of the forms.
    for (case, pyproject) in [
        (
            "python_roots_package_dir_dotted",
            "[tool.setuptools.package-dir]\n\"shop.cart\" = \"lib/shop/cart\"\n",
        ),
        (
            "python_roots_package_dir",
            "[tool.setuptools.package-dir]\n\"\" = \"lib\"\n",
        ),
        (
            "python_roots_package_dir_named",
            "[tool.setuptools]\npackage-dir = { shop = \"./lib/shop\" }\n",
        ),
        (
            "python_roots_find",
            "[tool.setuptools.packages.find]\nwhere = [\"lib\"]\n",
        ),
        (
            "python_roots_poetry",
            "[tool.poetry]\npackages = [{ include = \"shop\", from = \"lib\" }]\n",
        ),
    ] {
        assert_eq!(
            python_roots(case, pyproject),
            [exact.clone(), None],
            "{case}"
        );
    }
}

#[test]
fn python_class_families_are_linearized_once() {
    // A ladder of diamonds: walking every path through it would take 2^24 steps.
    let repo = fixture_repo("python_ladder", "python-roots", Some(ORIGIN));
    let mut src =
        String::from("class L0A:\n    def m(self):\n        return 0\n\n\nclass L0B:\n    pass\n");
    for n in 1..=24 {
        for side in ["A", "B"] {
            let p = n - 1;
            src += &format!("\n\nclass L{n}{side}(L{p}A, L{p}B):\n    pass\n");
        }
    }
    src += "\n\nclass Top(L24A, L24B):\n    def go(self):\n        return self.m()\n";
    std::fs::write(repo.join("ladder.py"), src).unwrap();

    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();

    let id = format!("{REPO}::ladder.py::Top.go::self.m#0");
    let call = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap();
    assert_eq!(
        (call["level"].as_str(), call["target"].as_str()),
        (
            Some("exact"),
            Some(format!("{REPO}::ladder.py::L0A.m").as_str())
        )
    );
}

#[test]
fn python_pyproject_that_does_not_parse_names_the_file() {
    let repo = fixture_repo("python_roots_invalid", "python-roots", Some(ORIGIN));
    std::fs::write(repo.join("pyproject.toml"), "[tool\n").unwrap();

    let error = Engine::open(&repo).and_then(|e| e.export()).unwrap_err();

    assert!(error.to_string().contains("pyproject.toml"), "{error}");
}
