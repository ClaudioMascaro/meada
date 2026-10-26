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
