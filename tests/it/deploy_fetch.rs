//! The awk program the deploy fetch scripts share (#544): `deploy/compose/fetch-pulled.sh` and the
//! chart's `workspace.fetch` read the entries of `meada workspace show --pulled` with it. Needs no
//! services, so it runs in the default `cargo test`.

use std::fs;
use std::path::Path;
use std::process::Command;

/// The awk program between `awk '` and `' "$list"` in a deploy fetch script, one line per line
/// with its indentation dropped, so the copy in a shell script and the one in a YAML block compare.
fn fetch_awk(path: &str) -> String {
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap();
    let (_, rest) = text
        .split_once("pairs=$(awk '\n")
        .expect("the fetch parses its entries");
    let (program, _) = rest
        .split_once("' \"$list\")")
        .expect("the awk program ends");
    let lines: Vec<&str> = program.trim_end().lines().map(str::trim).collect();
    lines.join("\n")
}

/// The Compose fetch script and the chart's `workspace.fetch` carry one awk program (#544), and it
/// reads the `repo` and `path` of each `[[repository]]` table itself: a peer or resource table
/// after an entry replaces nothing, a comment after a closing quote is not part of the value.
#[test]
fn the_deploy_fetch_reads_each_repository_table_by_its_own_keys() {
    let compose = fetch_awk("deploy/compose/fetch-pulled.sh");
    let chart = fetch_awk("deploy/helm/meada/values.yaml");
    assert_eq!(compose, chart, "the two copies of the fetch parse drifted");
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fetch_parse-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("parse.awk"), &compose).unwrap();
    let parse = |input: &str| {
        fs::write(dir.join("in.toml"), input).unwrap();
        let out = Command::new("awk")
            .current_dir(&dir)
            .args(["-f", "parse.awk", "in.toml"])
            .output()
            .expect("awk runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    // The oracle: the `repository` array of tables as the toml crate reads the input.
    let expected = |input: &str| -> String {
        let doc: toml::Table = toml::from_str(input).expect("the case is valid TOML");
        let get =
            |t: &toml::Table, k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        doc.get("repository")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_table())
            .map(|t| format!("{} {}\n", get(t, "repo"), get(t, "path")))
            .collect()
    };
    let web = "[[repository]]\nrepo = \"github.com/a/web\"\npath = \"/data/mirrors/web\"\n";
    let api = "[[repository]]\nrepo = \"github.com/b/api\"\npath = \"/data/mirrors/api\"\n";
    let cases: Vec<(&str, String)> = vec![
        ("a plain entry", web.into()),
        (
            "the entry of the issue",
            "[[repository]]\nrepo = \"github.com/a/web\"\npath = \"/data/mirrors/web\" # note\n\n\
             [[repository.peer]]\nrepo = \"github.com/other/x\"\n"
                .into(),
        ),
        (
            "a peer table",
            format!("{web}\n[[repository.peer]]\nrepo = \"x\"\npath = \"/p\"\n"),
        ),
        (
            "a resource table",
            format!("{web}\n[[repository.resource]]\nrepo = \"x\"\npath = \"/r\"\n"),
        ),
        (
            "any other table header",
            format!("{web}\n[workspace]\nrepo = \"x\"\n\n[[other]]\npath = \"/o\"\n"),
        ),
        (
            "a comment after a value or a header",
            "# lead\n[[repository]] # entry\nrepo = \"github.com/a/web\" # why\n\
             path = \"/data/mirrors/web\"   # where\n"
                .into(),
        ),
        (
            "no spaces around the equals sign",
            "[[repository]]\nrepo=\"github.com/a/web\"\npath=\"/data/mirrors/web\"\n".into(),
        ),
        (
            "two entries with a peer table between",
            format!("{web}\n[[repository.peer]]\nrepo = \"x\"\n\n{api}"),
        ),
        ("two entries in a row", format!("{web}{api}")),
        (
            "spaces inside the brackets of a header",
            "[[ repository ]]\nrepo = \"github.com/a/web\"\npath = \"/data/mirrors/web\"\n\
             [[ repository.peer ]]\nrepo = \"x\"\n"
                .into(),
        ),
        (
            "keys in either order",
            "[[repository]]\npath = \"/data/mirrors/web\"\nrepo = \"github.com/a/web\"\n".into(),
        ),
        (
            "keys outside a repository table are not read",
            "repo = \"github.com/x/y\"\n[[repository]]\nrepo = \"github.com/a/web\"\n".into(),
        ),
        (
            "a table between entries and tabs around the header",
            format!(
                "{web}\n[workspace]\nname = \"w\"\n\n\t[[\trepository\t]]\t# e\n\
                     repo = \"github.com/b/api\"\npath = \"/data/mirrors/api\"\n"
            ),
        ),
        (
            "quoted keys",
            "[[repository]]\n\"repo\" = \"github.com/a/web\"\n\"path\" = \"/data/mirrors/web\"\n"
                .into(),
        ),
        ("no entry", String::new()),
    ];
    for (name, input) in &cases {
        assert_eq!(parse(input), expected(input), "{name}");
    }
}
