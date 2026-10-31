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
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fetch_parse");
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
    let web = "[[repository]]\nrepo = \"github.com/a/web\"\npath = \"/data/mirrors/web\"\n";
    let want = "github.com/a/web /data/mirrors/web\n";
    assert_eq!(parse(web), want, "a plain entry");
    let issue = "[[repository]]\nrepo = \"github.com/a/web\"\npath = \"/data/mirrors/web\" # note\n\n\
                 [[repository.peer]]\nrepo = \"github.com/other/x\"\n";
    assert_eq!(parse(issue), want, "the entry of the issue");
    let peer =
        format!("{web}\n[[repository.peer]]\nrepo = \"github.com/other/x\"\npath = \"/p\"\n");
    assert_eq!(parse(&peer), want, "a peer table");
    let resource = format!("{web}\n[[repository.resource]]\nrepo = \"x\"\npath = \"/r\"\n");
    assert_eq!(parse(&resource), want, "a resource table");
    let other = format!("{web}\n[workspace]\nrepo = \"x\"\n\n[[other]]\npath = \"/o\"\n");
    assert_eq!(parse(&other), want, "any other table header");
    let comments = "# lead\n[[repository]] # entry\nrepo = \"github.com/a/web\" # why\n\
                    path = \"/data/mirrors/web\"   # where\n";
    assert_eq!(parse(comments), want, "a comment after a value or a header");
    assert_eq!(
        parse("[[repository]]\nrepo=\"github.com/a/web\"\npath=\"/data/mirrors/web\"\n"),
        want
    );
    let two = format!(
        "{web}\n[[repository.peer]]\nrepo = \"x\"\n\n[[repository]]\n\
                       repo = \"github.com/b/api\"\npath = \"/data/mirrors/api\"\n"
    );
    let both = format!("{want}github.com/b/api /data/mirrors/api\n");
    assert_eq!(parse(&two), both, "two entries with a peer table between");
    let rows = "[[repository]]\nrepo = \"github.com/a/web\"\npath = \"/data/mirrors/web\"\n\
                [[repository]]\nrepo = \"github.com/b/api\"\npath = \"/data/mirrors/api\"\n";
    assert_eq!(parse(rows), both, "two entries in a row");
    let before = "repo = \"github.com/x/y\"\n[[repository]]\nrepo = \"github.com/a/web\"\n";
    assert_eq!(
        parse(before),
        "github.com/a/web \n",
        "keys outside a repository table are not read"
    );
    assert_eq!(parse(""), "", "no entry");
}
