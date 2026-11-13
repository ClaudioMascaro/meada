//! The command table (#831, ADR 0028): the `run` script of a CI step, read under one gate.
//! `pip install` and its forms make the CI step a consumer of the package they name, at the
//! level the package kind gives a dependency by name.

use std::fs;
use std::path::{Path, PathBuf};

use meada::engine::Engine;
use serde_json::Value;

use crate::common::fixture_repo;

const WEB: &str = "example.com/pkg/pyweb";
const LIB: &str = "example.com/pkg/pylib::pyproject.toml";
const TOOLS: &str = "example.com/pkg/pylib::tools/pyproject.toml";
const FILE: &str = ".github/workflows/ci.yml";
const SHARED: &str = "package::pypi::shared-lib";

/// The PyPI fixtures: `pylib` provides `shared-lib` 1.2.0 and `shared-tools` 0.3.0, and `pyweb`
/// names it as its peer.
fn pair(case: &str) -> (PathBuf, PathBuf) {
    let repo = |name: &str| {
        let origin = format!("https://example.com/pkg/{name}.git");
        fixture_repo(case, &format!("pkg-{name}"), Some(&origin))
    };
    (repo("pylib"), repo("pyweb"))
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A pipeline of one CI job, `build`, whose CI steps are `steps` (each line indented under
/// `steps:`), with `head` between `on` and `jobs`.
fn pipeline(head: &str, steps: &str) -> String {
    let steps: String = steps.lines().map(|l| format!("      {l}\n")).collect();
    format!(
        "name: ci\non: push\n{head}jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n{steps}"
    )
}

/// The CI step `index` of the job `build`.
fn step(index: usize) -> String {
    format!("{WEB}::{FILE}::jobs.build.steps[{index}]")
}

fn exported(root: &Path) -> Value {
    let text = Engine::open(root).unwrap().export().unwrap();
    serde_json::from_str(&text).unwrap()
}

/// What the CI nodes of the pipeline hold outside the CI kind: `(address, contract, via)` of
/// each consumer row, in the export's order.
fn sides(export: &Value) -> Vec<(String, Option<String>, Option<String>)> {
    let own = format!("{WEB}::{FILE}::");
    let text = |v: &Value| v.as_str().map(str::to_string);
    let rows = export["consumers"].as_array().into_iter().flatten();
    rows.filter_map(|row| {
        let address = row["site"].as_str()?.strip_prefix(&own)?;
        Some((
            address.to_string(),
            text(&row["contract"]),
            text(&row["via"]),
        ))
    })
    .collect()
}

/// The `(consumer address, provider, level, reason)` of every link that leaves a CI node of the
/// pipeline.
fn links(export: &Value) -> Vec<(String, String, String, String)> {
    let own = format!("{WEB}::{FILE}::");
    let rows = export["links"].as_array().into_iter().flatten();
    rows.filter_map(|l| {
        let at = |k: &str| l[k].as_str().unwrap().to_string();
        let address = l["consumer"].as_str()?.strip_prefix(&own)?;
        Some((
            address.to_string(),
            at("provider"),
            at("level"),
            at("reason"),
        ))
    })
    .collect()
}

fn side(address: &str, contract: &str) -> (String, Option<String>, Option<String>) {
    (address.into(), Some(contract.into()), None)
}

#[test]
fn an_install_by_name_makes_the_ci_step_a_consumer_of_the_package() {
    let (_, web) = pair("ci_run_install");
    write(
        &web,
        FILE,
        &pipeline("", "- run: pip install \"shared-lib>=1.0,<2\""),
    );
    let export = exported(&web);
    assert_eq!(sides(&export), [side("jobs.build.steps[0]", SHARED)]);
    // One provider of the name in the workspace is `exact`, the range as written
    // (`python_dependencies_and_imports_link_to_the_distribution` shows the manifest's).
    let reason = "package::pypi::shared-lib >=1.0,<2 (local 1.2.0)";
    assert_eq!(
        links(&export),
        [(
            "jobs.build.steps[0]".to_string(),
            LIB.to_string(),
            "exact".to_string(),
            reason.to_string()
        )]
    );
    let nodes = export["iac_nodes"].as_array().unwrap();
    assert!(nodes.iter().any(|n| n["identity"] == step(0)), "{nodes:#?}");
}

/// The `(address, contract)` of the consumer rows a name gives, a CI step per entry of
/// `steps` in the job `build`.
fn named(case: &str, head: &str, steps: &[&str]) -> (Value, Vec<(String, String)>) {
    let (_, web) = pair(case);
    let text: String = steps.iter().map(|s| format!("- run: {s}\n")).collect();
    write(&web, FILE, &pipeline(head, &text));
    let export = exported(&web);
    let rows = sides(&export).into_iter();
    let rows = rows.filter_map(|(address, contract, _)| Some((address, contract?)));
    (export, rows.collect())
}

fn at(index: usize, contract: &str) -> (String, String) {
    (format!("jobs.build.steps[{index}]"), contract.into())
}

const TOOLS_PACKAGE: &str = "package::pypi::shared-tools";

#[test]
fn each_install_command_of_the_table_reads_the_names_it_is_given() {
    let (export, rows) = named(
        "ci_run_commands",
        "",
        &[
            "pip install shared-lib",
            "pip3 install shared-lib==1.2.0",
            "python -m pip install 'shared-lib>=2'",
            "uv pip install --system shared_lib[fast]",
            "pipx install shared-tools",
            "uv tool install Shared.Tools",
            "pip install --upgrade shared-lib shared-tools 'shared-lib<1'",
            // A package no repository of the workspace provides, as a manifest's entry.
            "pip install requests",
        ],
    );
    assert_eq!(
        rows,
        [
            at(0, SHARED),
            at(1, SHARED),
            at(2, SHARED),
            at(3, SHARED),
            at(4, TOOLS_PACKAGE),
            at(5, TOOLS_PACKAGE),
            at(6, SHARED),
            at(6, TOOLS_PACKAGE),
        ]
    );
    // Every link is the package kind's `exact`, with the range as written and its flag; the
    // first install of a name by a CI step gives the range.
    let said = |range: &str, flag: &str| format!("{SHARED} {range} (local 1.2.0){flag}");
    let tools = |n: usize| {
        let reason = format!("{TOOLS_PACKAGE} none (local 0.3.0), range unchecked");
        (
            format!("jobs.build.steps[{n}]"),
            TOOLS.to_string(),
            "exact".to_string(),
            reason,
        )
    };
    let lib = |n: usize, range: &str, flag: &str| {
        let reason = said(range, flag);
        (
            format!("jobs.build.steps[{n}]"),
            LIB.to_string(),
            "exact".to_string(),
            reason,
        )
    };
    assert_eq!(
        links(&export),
        [
            lib(0, "none", ", range unchecked"),
            lib(1, "==1.2.0", ""),
            lib(2, ">=2", ", range mismatch"),
            lib(3, "none", ", range unchecked"),
            tools(4),
            tools(5),
            lib(6, "none", ", range unchecked"),
            tools(6),
        ]
    );
}

#[test]
fn an_argument_that_is_no_name_and_a_command_outside_the_table_give_no_side() {
    let (_, rows) = named(
        "ci_run_arguments",
        "",
        &[
            "pip install -e .",
            "pip install .",
            "pip install ./shared-lib",
            "pip install shared-lib/",
            "pip install -r requirements.txt",
            "pip install https://example.com/shared-lib.tar.gz",
            "pip install 'shared-lib @ https://example.com/shared-lib.whl'",
            "pip install git+https://example.com/pkg/pylib.git",
            // 8: the value of an option is never a name, whatever it looks like.
            "pip install --index-url https://example.com/simple --no-binary shared-lib shared-tools",
            "pip install --target shared-lib -c shared-lib -U shared-tools",
            "pip install --find-links=shared-lib -- shared-tools",
            "pip install --no-binary shared-lib",
            // 12: commands outside the table.
            "pip download shared-lib",
            "pip uninstall shared-lib",
            "pip --quiet install shared-lib",
            "python3 -m pip install shared-lib",
            "conda install shared-lib",
            "sudo pip install shared-lib",
            "echo pip install shared-lib",
        ],
    );
    // In the export's order, by site.
    assert_eq!(
        rows,
        [
            at(10, TOOLS_PACKAGE),
            at(8, TOOLS_PACKAGE),
            at(9, TOOLS_PACKAGE)
        ]
    );
}

#[test]
fn the_gate_decides_which_lines_of_a_script_are_commands() {
    let (_, web) = pair("ci_run_gate");
    let steps = "\
- run: |
    echo \"pip install shared-lib\"
    # pip install shared-lib
    true || pip install shared-lib
    cat names | xargs pip install shared-lib
- run: |
    if [ -n \"$CI\" ]; then pip install shared-lib; fi
    for n in 1 2; do
      pip install shared-tools
    done
- run: |
    cd tools && pip install \\
      shared-tools
- run: |
    pip install shared-lib
    cat <<EOF
    pip install shared-tools
    EOF
- run: pip install shared-lib > log; pip install shared-tools 2>&1 | tee log
- name: no script
- uses: actions/setup-python@v5
  with:
    run: pip install shared-lib
";
    write(&web, FILE, &pipeline("", steps));
    let rows: Vec<_> = sides(&exported(&web))
        .into_iter()
        .map(|(address, contract, _)| (address, contract.unwrap()))
        .collect();
    assert_eq!(
        rows,
        [
            at(1, SHARED),
            at(1, TOOLS_PACKAGE),
            at(2, TOOLS_PACKAGE),
            at(4, SHARED),
            at(4, TOOLS_PACKAGE),
        ]
    );
}

#[test]
fn a_script_is_read_under_bash_sh_or_no_declared_shell() {
    let (_, web) = pair("ci_run_shell");
    let job = |name: &str, defaults: &str, steps: &[(&str, &str)]| {
        let steps: String = (steps.iter())
            .map(|(shell, package)| format!("      - run: pip install {package}\n{shell}"))
            .collect();
        format!("  {name}:\n    runs-on: ubuntu-latest\n{defaults}    steps:\n{steps}")
    };
    let shell = |name: &str| format!("        shell: {name}\n");
    let defaults = |name: &str| format!("    defaults:\n      run:\n        shell: {name}\n");
    let (bash, sh, pwsh) = (shell("bash"), shell("sh"), shell("pwsh"));
    let (python, template, expression) = (
        shell("python"),
        shell("bash -e {0}"),
        shell("${{ matrix.shell }}"),
    );
    let lib = "shared-lib";
    let text = format!(
        "name: ci\non: push\ndefaults:\n  run:\n    shell: pwsh\njobs:\n{}{}{}",
        // The pipeline's default is `pwsh`: only a CI step that declares its own is read.
        job(
            "inherits",
            "",
            &[("", lib), (&bash, lib), (&sh, lib), (&pwsh, lib)]
        ),
        // The CI job's default wins over the pipeline's, and the CI step's over both.
        job(
            "declares",
            &defaults("bash"),
            &[
                ("", lib),
                (&pwsh, lib),
                (&python, lib),
                (&template, lib),
                (&expression, lib)
            ]
        ),
        job("windows", &defaults("cmd"), &[("", lib), (&sh, lib)]),
    );
    write(&web, FILE, &text);
    let rows: Vec<String> = sides(&exported(&web))
        .into_iter()
        .map(|row| row.0)
        .collect();
    assert_eq!(
        rows,
        [
            "jobs.declares.steps[0]",
            "jobs.inherits.steps[1]",
            "jobs.inherits.steps[2]",
            "jobs.windows.steps[1]",
        ]
    );
    // None declared anywhere is read too, also in a composite action, where the CI step
    // declares its shell.
    write(&web, FILE, &pipeline("", "- run: pip install shared-lib"));
    let action = "name: setup\nruns:\n  using: composite\n  steps:\n    - run: pip install shared-lib\n      shell: bash\n    - run: pip install shared-lib\n      shell: pwsh\n";
    write(&web, "tools/setup/action.yml", action);
    let export = exported(&web);
    let sites: Vec<&str> = (export["consumers"].as_array().unwrap().iter())
        .filter(|row| row["contract"] == SHARED)
        .filter_map(|row| row["site"].as_str()?.strip_prefix(&format!("{WEB}::")))
        .filter(|site| site.contains(".yml::"))
        .collect();
    assert_eq!(
        sites,
        [
            ".github/workflows/ci.yml::jobs.build.steps[0]",
            "tools/setup/action.yml::runs.steps[0]",
        ]
    );
}

#[test]
fn a_name_is_a_literal_a_pattern_or_left_open() {
    let (_, web) = pair("ci_run_names");
    let head = "env:\n  PKG: shared-lib\n  TOOL: other\n  SPACED: shared-lib shared-tools\n  DYNAMIC: ${{ vars.PACKAGE }}\n";
    let steps = "\
- run: pip install ${{ env.PKG }} \"$TOOL\"
  env:
    TOOL: shared-tools
- run: pip install $PKG==1.2.0 ${TOOL}
- run: pip install ${{ vars.PACKAGE }}
- run: pip install \"${{ secrets.PACKAGE }}>=1\" $UNKNOWN $UNKNOWN
- run: pip install $(cat name) `cat name` ${{ matrix.package }} ${{ github.event.inputs.package }}
- run: pip install $SPACED $DYNAMIC ${PKG:-shared-lib}
- run: pip install shared-${{ vars.PART }}
- run: pip install \"${{ vars.PREFIX }}_Lib<2\"
- run: pip install other-${{ vars.PART }} sh${{ vars.PART }} ${{ vars.A }}-${{ vars.B }} ./${{ vars.PART }} $HOME/shared-lib
- run: pip install --index-url ${{ secrets.INDEX }} -r $FILE --no-binary=$UNKNOWN shared-lib
- run: $INSTALLER install shared-lib
";
    write(&web, FILE, &pipeline(head, steps));
    let export = exported(&web);
    let open = |n: usize, written: &str| {
        (
            format!("jobs.build.steps[{n}]"),
            None,
            Some(written.to_string()),
        )
    };
    let pattern = |n: usize, name: &str| {
        let contract = Some(format!("package::pypi::{name}"));
        (
            format!("jobs.build.steps[{n}]"),
            contract,
            Some("pattern".to_string()),
        )
    };
    assert_eq!(
        sides(&export),
        [
            // The nearest `env` that defines a name decides.
            side("jobs.build.steps[0]", SHARED),
            side("jobs.build.steps[0]", TOOLS_PACKAGE),
            side("jobs.build.steps[1]", SHARED),
            // A name left open whole is one unresolved consumer of its CI step, as written.
            open(2, "${{ vars.PACKAGE }}"),
            open(3, "${{ secrets.PACKAGE }}, $UNKNOWN"),
            open(
                4,
                "$(cat name), `cat name`, ${{ matrix.package }}, ${{ github.event.inputs.package }}"
            ),
            open(5, "$SPACED, $DYNAMIC, ${PKG:-shared-lib}"),
            // An open part of a name makes it a pattern.
            pattern(6, "shared-*"),
            pattern(7, "*-lib"),
            // No side: a pattern that fits no package or leaves too little of a name, a path,
            // the value of an option, a command left open.
            side("jobs.build.steps[9]", SHARED),
        ]
    );
    let found = links(&export);
    let of = |n: usize| -> Vec<(&str, &str, &str)> {
        let own = format!("jobs.build.steps[{n}]");
        let rows = found.iter().filter(|l| l.0 == own);
        rows.map(|l| (l.1.as_str(), l.2.as_str(), l.3.as_str()))
            .collect()
    };
    assert_eq!(
        of(1),
        [(
            LIB,
            "exact",
            "package::pypi::shared-lib ==1.2.0 (local 1.2.0)"
        )]
    );
    assert_eq!(
        of(6),
        [
            (
                LIB,
                "ambiguous",
                "2 providers fit; a pattern of the name; package::pypi::shared-* none (local 1.2.0), range unchecked"
            ),
            (
                TOOLS,
                "ambiguous",
                "2 providers fit; a pattern of the name; package::pypi::shared-* none (local 0.3.0), range unchecked"
            ),
        ]
    );
    assert_eq!(
        of(7),
        [(
            LIB,
            "inferred",
            "a pattern of the name; package::pypi::*-lib <2 (local 1.2.0)"
        )]
    );
    assert!(of(2).is_empty() && of(8).is_empty(), "{found:#?}");
}

#[test]
fn a_name_a_script_of_the_file_rebinds_keeps_no_literal() {
    let (_, web) = pair("ci_run_rebound");
    let head = "env:\n  PKG: shared-lib\n  TOOL: shared-tools\n";
    // A name written to `GITHUB_ENV` reaches the CI steps after it; one a script assigns for
    // itself stays in that script.
    let steps = "\
- run: echo \"PKG=shared-tools\" >> \"$GITHUB_ENV\"
- run: pip install $PKG ${{ env.PKG }} $TOOL
- run: |
    TOOL=shared-lib
    pip install $TOOL
";
    write(&web, FILE, &pipeline(head, steps));
    let step = |n: usize| format!("jobs.build.steps[{n}]");
    assert_eq!(
        sides(&exported(&web)),
        [
            (step(1), None, Some("$PKG, ${{ env.PKG }}".to_string())),
            (step(1), Some(TOOLS_PACKAGE.to_string()), None),
            (step(2), None, Some("$TOOL".to_string())),
        ]
    );
}

#[test]
fn a_script_past_the_value_length_ceiling_is_not_read() {
    let (_, web) = pair("ci_run_long");
    let script = |lines: usize| {
        let filler = "    echo 0123456789012345678901234567890123456789\n".repeat(lines);
        format!("- run: |\n{filler}    pip install shared-lib\n")
    };
    // 20 lines of 46 bytes and the install stay under 1024 bytes; 22 go past it.
    write(&web, FILE, &pipeline("", &(script(20) + &script(22))));
    assert_eq!(
        sides(&exported(&web)),
        [side("jobs.build.steps[0]", SHARED)]
    );
}

#[test]
fn the_answers_show_a_ci_step_that_installs_a_package() {
    let (lib, web) = pair("ci_run_answers");
    let steps = "- run: pip install 'shared-lib>=1.0'\n- run: pip install ${{ vars.PACKAGE }}\n";
    write(&web, FILE, &pipeline("", steps));
    // In the package's own repository, the kind's rule for a name with no source holds.
    write(&lib, FILE, &pipeline("", "- run: pip install shared-lib"));
    let engine = Engine::open(&web).unwrap();
    let listed = |answer: &str, start: &str| -> Vec<String> {
        let lines = answer.lines().filter_map(|l| l.strip_prefix(start));
        lines.map(str::to_string).collect()
    };
    // `context` on the package lists the CI step among its consumers.
    let package = engine.context(SHARED, None).unwrap();
    let steps: Vec<&str> = (package.lines()).filter(|l| l.contains(FILE)).collect();
    assert_eq!(
        steps,
        [
            "  example.com/pkg/pylib dependent .github/workflows/ci.yml:7 jobs.build.steps[0]",
            "  example.com/pkg/pyweb dependent .github/workflows/ci.yml:7 jobs.build.steps[0]",
        ],
        "{package}"
    );
    // `context` on the CI step shows the side, its provider, the level and the reason.
    let step = engine.context(&step(0), None).unwrap();
    assert_eq!(listed(&step, "consumes "), [SHARED], "{step}");
    assert_eq!(
        listed(&step, "  provider: "),
        [
            "example.com/pkg/pylib pyproject.toml [exact: package::pypi::shared-lib >=1.0 (local 1.2.0)]"
        ],
        "{step}"
    );
    let own = Engine::open(&lib).unwrap();
    let own = own
        .context(
            "example.com/pkg/pylib::.github/workflows/ci.yml::jobs.build.steps[0]",
            None,
        )
        .unwrap();
    assert_eq!(
        listed(&own, "  provider: "),
        [
            "example.com/pkg/pylib pyproject.toml [inferred: no uv source names the local package, the registry package is the likely provider; package::pypi::shared-lib none (local 1.2.0), range unchecked]"
        ],
        "{own}"
    );
    // A name left open is an open item no decision closes: it says why.
    let cause = "the package name is not a literal, and no literal `env` gives it";
    let open = engine.context(&self::step(1), None).unwrap();
    assert_eq!(
        listed(&open, "consumes "),
        [format!("(unresolved) ${{{{ vars.PACKAGE }}}}: {cause}")],
        "{open}"
    );
    let answer = engine.search("PACKAGE", Some("unresolved"), None).unwrap();
    assert_eq!(
        answer.lines().collect::<Vec<_>>(),
        [format!(
            "{WEB} {FILE}:8 (unresolved) candidates: 0; {cause} (`{}`)",
            self::step(1)
        )]
    );
}
