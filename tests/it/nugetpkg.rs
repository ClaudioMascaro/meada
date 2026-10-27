//! NuGet packages and project references link through literal `.csproj` files, without MSBuild
//! or a feed (spec #28, "Packages").

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{age, assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

fn workspace(case: &str) -> [PathBuf; 2] {
    ["lib", "app"].map(|role| {
        fixture_repo(
            case,
            &format!("dotnet-{role}"),
            Some(&format!("https://example.com/dotnet/{role}.git")),
        )
    })
}

fn export(repo: &Path) -> (String, Value) {
    let export = Engine::open(repo).unwrap().export().unwrap();
    let json = serde_json::from_str(&export).unwrap();
    (export, json)
}

/// The links of `json` a project file declares: a reference, not a call through a `using`.
fn declared(json: &Value) -> Vec<&Value> {
    (json["links"].as_array().into_iter().flatten())
        .filter(|l| !l["consumer"].as_str().unwrap().contains(".cs::"))
        .collect()
}

#[test]
fn package_and_project_references_link_to_workspace_projects() {
    let [lib, app] = workspace("nuget_packages_literals");
    let (lib_export, lib_json) = export(&lib);
    let names: Vec<_> = lib_json["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    // The id is the `PackageId`, else the `AssemblyName`.
    assert_eq!(names, ["Example.Kit", "Example.Money"]);
    assert_eq!(lib_json["packages"][0]["ecosystem"], "nuget");
    assert_golden("dotnet-lib.export.json", &lib_export);

    let (app_export, app_json) = export(&app);
    let links = declared(&app_json);
    let contracts: Vec<_> = links
        .iter()
        .map(|l| {
            (
                l["contract"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
            )
        })
        .collect();
    // Newtonsoft.Json is outside the workspace, and Shared is a project of the app's own
    // repository: neither is a package link.
    assert_eq!(
        contracts,
        [
            ("package::nuget::Example.Money", "exact"),
            ("package::nuget::Example.Kit", "exact"),
        ],
        "{app_export}"
    );
    assert_eq!(
        links[1]["reason"],
        "package::nuget::Example.Kit [1.0,2.0) (local 1.2.0), range unchecked"
    );
    assert_eq!(
        links[0]["provider"],
        "example.com/dotnet/lib::src/Money/Money.csproj"
    );
    assert_golden("dotnet-app.export.json", &app_export);
}

const CHECKOUT_TOTAL: &str =
    "example.com/dotnet/app::src/Web/Checkout.cs::Example.App.Web.Checkout.Pay(int)::order.Total#0";
const ORDER: &str = "example.com/dotnet/lib::src/Kit/Pricing/Order.cs::Example.Kit.Pricing.Order";

#[test]
fn a_using_reaches_the_type_a_depended_on_project_declares() {
    let [_, app] = workspace("nuget_packages_imports");
    let (export, json) = export(&app);
    let imports: Vec<_> = json["links"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["consumer"].as_str().unwrap().contains(".cs::"))
        .map(|l| {
            let get = |k: &str| l[k].as_str().unwrap().to_string();
            (
                get("consumer"),
                get("provider"),
                get("level"),
                get("reason"),
            )
        })
        .collect();
    let kit =
        "package::nuget::Example.Kit [1.0,2.0) (local 1.2.0), range unchecked; import namespace";
    let money =
        "package::nuget::Example.Money none (local none), range unchecked; import namespace";
    let lib = "example.com/dotnet/lib::src";
    let app = "example.com/dotnet/app::src";
    let expected = [
        (
            format!("{app}/Jobs/Report.cs::Example.App.Jobs.Report.Line(int)::Currency.Cents#0"),
            format!("{lib}/Money/Currency.cs::Example.Money.Currency.Cents(int)"),
            money,
        ),
        (
            format!("{app}/Jobs/Report.cs::Example.App.Jobs.Report.Short(int)::Cents#0"),
            format!("{lib}/Money/Currency.cs::Example.Money.Currency.Cents(int)"),
            money,
        ),
        (
            format!("{app}/Web/Checkout.cs::Example.App.Web.Checkout.Create()::Order#0"),
            format!("{lib}/Kit/Pricing/Order.cs::Example.Kit.Pricing.Order.Order(int)"),
            kit,
        ),
        (
            format!("{app}/Web/Checkout.cs::Example.App.Web.Checkout.Pay(int)::Order.Of#0"),
            format!("{lib}/Kit/Pricing/Order.cs::Example.Kit.Pricing.Order.Of(int)"),
            kit,
        ),
        (
            format!("{app}/Web/Checkout.cs::Example.App.Web.Checkout.Pay(int)::order.Total#0"),
            format!("{lib}/Kit/Pricing/Order.cs::Example.Kit.Pricing.Order.Total()"),
            kit,
        ),
    ];
    let expected: Vec<_> = expected
        .into_iter()
        .map(|(c, p, r)| (c, p, "exact".to_string(), r.to_string()))
        .collect();
    assert_eq!(imports, expected, "{export}");
}

#[test]
fn context_on_a_public_type_lists_its_consumers_in_other_repositories() {
    let [lib, _] = workspace("nuget_packages_context");
    let engine = Engine::open(&lib).unwrap();
    let found = engine.context(ORDER, None).unwrap();
    assert!(
        found.contains("example.com/dotnet/app src/Web/Checkout.cs"),
        "{found}"
    );
    assert_golden("dotnet-lib.context-order.txt", &found);
    let total = engine.context(&format!("{ORDER}.Total()"), None).unwrap();
    assert_golden("dotnet-lib.context-total.txt", &total);
}

#[test]
fn trace_down_crosses_the_package_hop_and_onward() {
    let [_, app] = workspace("nuget_packages_trace");
    let down = Engine::open(&app)
        .unwrap()
        .trace(Start::From(CHECKOUT_TOTAL), Direction::Down, None)
        .unwrap();
    assert_golden("dotnet-app.trace-down.txt", &down);
}

#[test]
fn several_providers_declaring_the_type_make_the_hop_ambiguous() {
    let [_, app] = workspace("nuget_packages_ambiguous");
    fixture_repo(
        "nuget_packages_ambiguous",
        "dotnet-twin",
        Some("https://example.com/dotnet/twin.git"),
    );
    let toml = app.join("meada.toml");
    let peers = fs::read_to_string(&toml).unwrap();
    let peer = "[[peer]]\nrepo = \"example.com/dotnet/twin\"\npath = \"../dotnet-twin\"\n";
    fs::write(&toml, format!("{peers}{peer}")).unwrap();
    let web = app.join("src/Web/Web.csproj");
    let text = fs::read_to_string(&web).unwrap();
    let more = "<PackageReference Include=\"Example.Twin\" Version=\"1\" />\n  </ItemGroup>";
    fs::write(&web, text.replace("</ItemGroup>", more)).unwrap();
    let (export, json) = export(&app);
    let total: Vec<_> = (json["links"].as_array().unwrap().iter())
        .filter(|l| l["consumer"].as_str().unwrap().ends_with("order.Total#0"))
        .map(|l| {
            (
                l["provider"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
            )
        })
        .collect();
    let (kit, twin) = (
        "example.com/dotnet/lib::src/Kit",
        "example.com/dotnet/twin::src",
    );
    assert_eq!(
        total,
        [
            (
                &format!("{kit}/Pricing/Order.cs::Example.Kit.Pricing.Order.Total()")[..],
                "ambiguous"
            ),
            (
                &format!("{twin}/Order.cs::Example.Kit.Pricing.Order.Total()")[..],
                "ambiguous"
            ),
        ],
        "{export}"
    );
}

fn edit(path: PathBuf, old: &str, new: &str) {
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(old), "{} lacks {old}", path.display());
    fs::write(&path, text.replace(old, new)).unwrap();
}

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// The twin repository, a peer of `app`.
fn twin(case: &str, app: &Path) -> PathBuf {
    let twin = fixture_repo(
        case,
        "dotnet-twin",
        Some("https://example.com/dotnet/twin.git"),
    );
    let toml = app.join("meada.toml");
    let peers = fs::read_to_string(&toml).unwrap();
    let peer = "[[peer]]\nrepo = \"example.com/dotnet/twin\"\npath = \"../dotnet-twin\"\n";
    fs::write(&toml, format!("{peers}{peer}")).unwrap();
    twin
}

/// The `(provider, level)` of each link whose consumer ends with `suffix`.
fn calls<'a>(json: &'a Value, suffix: &str) -> Vec<(&'a str, &'a str)> {
    (json["links"].as_array().unwrap().iter())
        .filter(|l| l["consumer"].as_str().unwrap().ends_with(suffix))
        .map(|l| {
            (
                l["provider"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
            )
        })
        .collect()
}

const KIT_ORDER: &str =
    "example.com/dotnet/lib::src/Kit/Pricing/Order.cs::Example.Kit.Pricing.Order";

#[test]
fn a_project_reference_pins_its_project_over_a_twin_with_the_same_package_id() {
    let [_, app] = workspace("nuget_packages_pinned");
    let twin = twin("nuget_packages_pinned", &app);
    edit(twin.join("src/Twin.csproj"), "Example.Twin", "Example.Kit");
    let web = app.join("src/Web/Web.csproj");
    let old = "<PackageReference Include=\"Example.Kit\" Version=\"[1.0,2.0)\" />";
    let new = "<ProjectReference Include=\"..\\..\\..\\dotnet-lib\\src\\Kit\\Kit.csproj\" />";
    edit(web, old, new);
    let (export, json) = export(&app);
    for (suffix, member) in [
        ("Order.Of#0", "Of(int)"),
        ("order.Total#0", "Total()"),
        ("Create()::Order#0", "Order(int)"),
    ] {
        let member = format!("{KIT_ORDER}.{member}");
        assert_eq!(calls(&json, suffix), [(&member[..], "exact")], "{export}");
    }
    let imports = json["imports"].as_array().unwrap();
    assert!(
        !imports
            .iter()
            .any(|i| i["target"].as_str().unwrap().contains("dotnet/twin")),
        "{export}"
    );
}

#[test]
fn a_project_of_the_same_repository_is_ambiguous_with_no_project_of_another() {
    let [_, app] = workspace("nuget_packages_own_project");
    let twin = twin("nuget_packages_own_project", &app);
    // Another repository's project is named like the app's own `Shared` and declares a type the
    // app's `using` also reaches in the project it really references.
    edit(twin.join("src/Twin.csproj"), "Example.Twin", "Shared");
    let order = twin.join("src/Order.cs");
    fs::write(&order, "namespace Example.Money;\n\npublic class Currency\n{\n    public static string Cents(int amount) => \"\";\n}\n").unwrap();
    let (export, json) = export(&app);
    let money = "example.com/dotnet/lib::src/Money/Currency.cs::Example.Money.Currency.Cents(int)";
    for suffix in ["Currency.Cents#0", "Short(int)::Cents#0"] {
        assert_eq!(calls(&json, suffix), [(money, "exact")], "{export}");
    }
    let imports = json["imports"].as_array().unwrap();
    assert!(
        !imports
            .iter()
            .any(|i| i["target"].as_str().unwrap().contains("dotnet/twin")),
        "{export}"
    );
}

#[test]
fn a_project_with_a_name_a_package_id_cannot_have_is_still_referenced() {
    let [lib, app] = workspace("nuget_packages_spaced");
    let (from, to) = (lib.join("src/Money"), lib.join("src/My Lib"));
    fs::rename(from, &to).unwrap();
    fs::rename(to.join("Money.csproj"), to.join("My Lib.csproj")).unwrap();
    edit(
        app.join("src/Jobs/Jobs.csproj"),
        "Money\\Money.csproj",
        "My Lib\\My Lib.csproj",
    );
    let (export, json) = export(&app);
    let links: Vec<_> = declared(&json)
        .iter()
        .map(|l| l["contract"].as_str().unwrap())
        .collect();
    assert_eq!(
        links,
        [
            "package::nuget::Example.Money",
            "package::nuget::Example.Kit"
        ],
        "{export}"
    );
}

#[test]
fn a_package_id_written_in_another_case_names_the_provider() {
    let [_, app] = workspace("nuget_packages_cased");
    edit(
        app.join("src/Web/Web.csproj"),
        "Include=\"Example.Kit\"",
        "Include=\"example.KIT\"",
    );
    let (export, json) = export(&app);
    let kit = declared(&json)
        .into_iter()
        .find(|l| l["contract"] == "package::nuget::Example.Kit");
    assert_eq!(
        kit.map(|l| l["level"].as_str().unwrap()),
        Some("exact"),
        "{export}"
    );
    let member = format!("{KIT_ORDER}.Total()");
    assert_eq!(
        calls(&json, "order.Total#0"),
        [(&member[..], "exact")],
        "{export}"
    );
}

#[test]
fn a_file_under_a_project_with_no_sdk_belongs_to_the_project_above() {
    let [lib, app] = workspace("nuget_packages_legacy");
    let legacy = "<Project><PropertyGroup><TargetFramework>net48</TargetFramework></PropertyGroup></Project>";
    write(&lib, "src/Kit/Legacy/Legacy.csproj", legacy);
    let extra = "namespace Example.Kit.Legacy;\n\npublic class Extra\n{\n    public static int Go() => 1;\n}\n";
    write(&lib, "src/Kit/Legacy/Extra.cs", extra);
    let user = "using Example.Kit.Legacy;\n\nnamespace Example.App.Web;\n\npublic class Uses\n{\n    public int Run() => Extra.Go();\n}\n";
    write(&app, "src/Web/Uses.cs", user);
    let (export, json) = export(&app);
    let go = "example.com/dotnet/lib::src/Kit/Legacy/Extra.cs::Example.Kit.Legacy.Extra.Go()";
    assert_eq!(calls(&json, "Extra.Go#0"), [(go, "exact")], "{export}");
}

#[test]
fn a_project_reference_beyond_the_workspace_links_nothing() {
    let [_, app] = workspace("nuget_packages_beyond");
    let more =
        "<ProjectReference Include=\"..\\..\\..\\elsewhere\\Gone\\Gone.csproj\" />\n  </ItemGroup>";
    edit(app.join("src/Web/Web.csproj"), "</ItemGroup>", more);
    let (export, json) = export(&app);
    let links: Vec<_> = declared(&json)
        .iter()
        .map(|l| l["contract"].as_str().unwrap())
        .collect();
    assert_eq!(
        links,
        [
            "package::nuget::Example.Money",
            "package::nuget::Example.Kit"
        ],
        "{export}"
    );
}

#[test]
fn a_file_counts_for_the_ecosystem_that_imports_its_extension_only() {
    let [lib, app] = workspace("nuget_packages_extensions");
    // A Java class under a `.csproj` and a polyfill in `System.*` are no types a `using` brings.
    let gizmo = "package Example.Kit.Pricing;\n\npublic class Gizmo {\n    public static int make() { return 1; }\n}\n";
    write(&lib, "src/Kit/Pricing/Gizmo.java", gizmo);
    let poly =
        "namespace System.Kit;\n\npublic class Poly\n{\n    public static int Make() => 1;\n}\n";
    write(&lib, "src/Kit/Poly.cs", poly);
    let user = "using Example.Kit.Pricing;\nusing System.Kit;\n\nnamespace Example.App.Web;\n\npublic class Uses\n{\n    public int Run() => Gizmo.make() + Poly.Make();\n}\n";
    write(&app, "src/Web/Uses.cs", user);
    let (export, json) = export(&app);
    assert!(calls(&json, "Gizmo.make#0").is_empty(), "{export}");
    assert!(calls(&json, "Poly.Make#0").is_empty(), "{export}");
}

#[test]
fn a_type_with_a_base_the_workspace_does_not_hold_is_inferred() {
    let [lib, app] = workspace("nuget_packages_opaque");
    let bundle = "namespace Example.Kit.Pricing;\n\npublic class Bundle : Outside.Base\n{\n    public static Bundle Of(int n) => new Bundle();\n\n    public int Total() => 1;\n}\n";
    write(&lib, "src/Kit/Pricing/Bundle.cs", bundle);
    let user = "using Example.Kit.Pricing;\n\nnamespace Example.App.Web;\n\npublic class Uses\n{\n    public int Run()\n    {\n        Bundle bundle = Bundle.Of(1);\n        return bundle.Total();\n    }\n}\n";
    write(&app, "src/Web/Uses.cs", user);
    let (export, json) = export(&app);
    let total =
        "example.com/dotnet/lib::src/Kit/Pricing/Bundle.cs::Example.Kit.Pricing.Bundle.Total()";
    assert_eq!(
        calls(&json, "bundle.Total#0"),
        [(total, "inferred")],
        "{export}"
    );
}

/// Edits the `.csproj` files of the workspace: a package renamed, a reference dropped and a
/// project added.
fn change_projects([lib, app]: &[PathBuf; 2]) {
    let replace = |path: PathBuf, old: &str, new: &str| {
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(old), "{} lacks {old}", path.display());
        fs::write(&path, text.replace(old, new)).unwrap();
    };
    replace(
        lib.join("src/Kit/Kit.csproj"),
        "<PackageId>Example.Kit</PackageId>",
        "<PackageId>Example.Pricing</PackageId>",
    );
    replace(
        app.join("src/Jobs/Jobs.csproj"),
        "    <ProjectReference Include=\"..\\Shared\\Shared.csproj\" />\n",
        "",
    );
    fs::create_dir_all(lib.join("src/Added")).unwrap();
    fs::write(
        lib.join("src/Added/Added.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>",
    )
    .unwrap();
}

#[test]
fn edited_and_added_projects_refresh_like_a_cold_workspace() {
    let warm = workspace("nuget_packages_refresh");
    age(warm[0].parent().unwrap());
    let exports = |repos: &[PathBuf; 2]| {
        repos
            .each_ref()
            .map(|p| Engine::open(p).unwrap().export().unwrap())
    };
    let before = exports(&warm);
    assert_eq!(before, exports(&warm), "byte-identical repeated exports");
    change_projects(&warm);
    let refreshed = exports(&warm);
    assert_ne!(before, refreshed);
    let cold = workspace("nuget_packages_cold");
    change_projects(&cold);
    assert_eq!(refreshed, exports(&cold));
}

#[test]
fn a_file_sees_the_references_its_project_references_pass_on() {
    let [_, app] = workspace("nuget_packages_transitive");
    // App -> Lib, of the app's repository; Lib -> Money, and Kit, privately.
    let lib = "<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup>\n\
               <PackageReference Include=\"Example.Money\" />\n\
               <PackageReference Include=\"Example.Kit\" PrivateAssets=\"all\" />\n\
               </ItemGroup></Project>";
    write(&app, "src/Lib/Lib.csproj", lib);
    let project = "<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup>\n\
                   <ProjectReference Include=\"..\\Lib\\Lib.csproj\" />\n\
                   </ItemGroup></Project>";
    write(&app, "src/App/App.csproj", project);
    let user = |ns: &str| {
        format!(
            "using Example.Money;\nusing Example.Kit.Pricing;\n\nnamespace Example.{ns};\n\npublic class Uses\n{{\n    public void Go()\n    {{\n        Currency.Cents(1);\n        Order.Of(1);\n    }}\n}}\n"
        )
    };
    write(&app, "src/Lib/Uses.cs", &user("Lib"));
    write(&app, "src/App/Uses.cs", &user("App"));
    let (export, json) = export(&app);
    let cents = "example.com/dotnet/lib::src/Money/Currency.cs::Example.Money.Currency.Cents(int)";
    let of = format!("{KIT_ORDER}.Of(int)");
    let at = |ns: &str, call: &str| calls(&json, &format!("Example.{ns}.Uses.Go()::{call}#0"));
    assert_eq!(at("Lib", "Currency.Cents"), [(cents, "exact")], "{export}");
    assert_eq!(at("Lib", "Order.Of"), [(&of[..], "exact")], "{export}");
    // Through `import namespace`, as from Lib; the private reference stays Lib's.
    assert_eq!(at("App", "Currency.Cents"), [(cents, "exact")], "{export}");
    assert!(at("App", "Order.Of").is_empty(), "{export}");
    let reason = (json["links"].as_array().unwrap().iter())
        .find(|l| {
            l["consumer"]
                .as_str()
                .unwrap()
                .ends_with("App.Uses.Go()::Currency.Cents#0")
        })
        .map(|l| l["reason"].as_str().unwrap());
    assert!(
        reason.is_some_and(|r| r.ends_with("; import namespace")),
        "{export}"
    );
}

/// A project of the app that names no reference, and a call in it on `Money`'s type.
fn tool(app: &Path) {
    write(
        app,
        "src/Tool/Tool.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>",
    );
    let run = "using Example.Money;\n\nnamespace Example.App.Tool;\n\npublic class Run\n{\n    public string Go() => Currency.Cents(1);\n}\n";
    write(app, "src/Tool/Run.cs", run);
}

const CENTS: &str =
    "example.com/dotnet/lib::src/Money/Currency.cs::Example.Money.Currency.Cents(int)";

fn tool_calls(json: &Value) -> Vec<(&str, &str)> {
    calls(json, "Example.App.Tool.Run.Go()::Currency.Cents#0")
}

/// A `Directory.Build.props` gives the projects below it its `PackageReference`s and
/// `ProjectReference`s (relative to each project), the nearest hiding the ones above unless it
/// imports them; the props file itself declares none.
#[test]
fn a_directory_build_props_gives_its_references_to_the_projects_below_it() {
    let [_, app] = workspace("nuget_packages_props");
    tool(&app);
    let (text, json) = export(&app);
    assert!(tool_calls(&json).is_empty(), "{text}");
    let package =
        "<Project><ItemGroup><PackageReference Include=\"Example.Money\" /></ItemGroup></Project>";
    write(&app, "Directory.Build.props", package);
    let (text, json) = export(&app);
    assert_eq!(tool_calls(&json), [(CENTS, "exact")], "{text}");
    let declared: Vec<_> = declared(&json)
        .iter()
        .map(|l| l["consumer"].as_str().unwrap())
        .collect();
    assert!(
        declared
            .iter()
            .all(|c| !c.contains("Directory.Build.props")),
        "{text}"
    );
    // A project reference, written relative to the project, not to the props file.
    let project = "<Project><ItemGroup><ProjectReference Include=\"..\\..\\..\\dotnet-lib\\src\\Money\\Money.csproj\" /></ItemGroup></Project>";
    write(&app, "Directory.Build.props", project);
    let (text, json) = export(&app);
    assert_eq!(tool_calls(&json), [(CENTS, "exact")], "{text}");
    // The nearest hides it; with the import, both apply.
    write(&app, "src/Directory.Build.props", "<Project></Project>");
    let (text, json) = export(&app);
    assert!(tool_calls(&json).is_empty(), "{text}");
    let import = "<Project><Import Project=\"$([MSBuild]::GetPathOfFileAbove('Directory.Build.props', '$(MSBuildThisFileDirectory)../'))\" /></Project>";
    write(&app, "src/Directory.Build.props", import);
    let (text, json) = export(&app);
    assert_eq!(tool_calls(&json), [(CENTS, "exact")], "{text}");
}

/// Editing only a `Directory.Build.props` changes the next answer, as a cold index gives it.
#[test]
fn an_edited_directory_build_props_refreshes_like_a_cold_workspace() {
    let package =
        "<Project><ItemGroup><PackageReference Include=\"Example.Money\" /></ItemGroup></Project>";
    let setup = |repos: &[PathBuf; 2]| {
        tool(&repos[1]);
        write(&repos[1], "Directory.Build.props", package);
    };
    let edit = |repos: &[PathBuf; 2]| {
        edit(
            repos[1].join("Directory.Build.props"),
            "Example.Money",
            "Example.Kit",
        );
    };
    let warm = workspace("nuget_packages_props_refresh");
    setup(&warm);
    age(warm[0].parent().unwrap());
    let before = export(&warm[1]);
    assert_eq!(tool_calls(&before.1), [(CENTS, "exact")], "{}", before.0);
    edit(&warm);
    let refreshed = export(&warm[1]);
    assert!(tool_calls(&refreshed.1).is_empty(), "{}", refreshed.0);
    let cold = workspace("nuget_packages_props_cold");
    setup(&cold);
    edit(&cold);
    assert_eq!(refreshed.0, export(&cold[1]).0);
}
