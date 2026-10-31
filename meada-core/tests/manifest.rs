//! A workspace text's validation at the crate's public seam: the errors the CLI prints for the
//! same text, naming the file, the line and the dotted key, or the entry.

use meada_core::manifest::{validate, validate_entries, validate_named};

const FILE: &str = "ops/meada.workspace.toml";

fn problems(src: &str) -> Vec<String> {
    validate(FILE, src).err().unwrap_or_default()
}

#[test]
fn a_valid_workspace_gives_its_entries_in_order() {
    let src = "[[repository]]\nrepo = \"example.com/acme/web\"\npath = \"../web\"\n\
               [repository.serve]\nprefix = \"/shop/\"\n\
               [[repository]]\nrepo = \"example.com/acme/api\"\npath = \"api\"\n\
               [[repository.peer]]\nrepo = \"example.com/acme/web\"\nbase_url_env = [\"WEB\"]\n";
    let entries = validate(FILE, src).unwrap_or_else(|e| panic!("{e:?}"));
    let repos: Vec<&str> = entries.iter().map(|e| e.repo.as_str()).collect();
    assert_eq!(repos, ["example.com/acme/web", "example.com/acme/api"]);
    assert_eq!(entries[0].serve.prefix.as_deref(), Some("/shop"));
    assert_eq!(
        entries[1].blocks(),
        ["[[repository.peer]] example.com/acme/web"]
    );
    assert!(validate(FILE, "").unwrap().is_empty());
}

#[test]
fn a_text_error_names_the_file_the_line_and_the_key() {
    for (src, error) in [
        (
            "[[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\nbranch = \"main\"\n",
            "ops/meada.workspace.toml:4: `repository.branch` is not a meada.workspace.toml key",
        ),
        (
            "[[repository]]\npath = \"x\"\n",
            "ops/meada.workspace.toml:1: `repository.repo` is missing",
        ),
        (
            "[[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\n[[repository.peer]]\n\
             repo = \"a.com/y\"\npath = \"../y\"\n",
            "ops/meada.workspace.toml:6: `repository.peer.path` is not a meada.workspace.toml key",
        ),
        (
            "[[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\n[repository.serve]\n\
             prefix = \"edge\"\n",
            "ops/meada.workspace.toml:5: `repository.serve.prefix`: must start with `/`",
        ),
        (
            "[[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\n[[repository.resource]]\n\
             kind = \"Postgres\"\nid = \"orders\"\nrole = \"read\"\n",
            "ops/meada.workspace.toml:5: `repository.resource.kind`: must be a \
             lowercase identifier (ASCII letters, digits and hyphens, starting with a letter)",
        ),
        // A workspace's text holds its entries alone.
        (
            "index_dir = \"/srv\"\n",
            "ops/meada.workspace.toml:1: `index_dir` is not a meada.workspace.toml key",
        ),
    ] {
        assert_eq!(problems(src), [error], "{src}");
    }
}

#[test]
fn every_entry_whose_identity_cannot_be_served_is_named() {
    let src = "[[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\n\
               [[repository]]\nrepo = \"local/y\"\npath = \"y\"\n\
               [[repository]]\nrepo = \"a.com/../z\"\npath = \"z\"\n\
               [[repository]]\nrepo = \"a.com/x\"\npath = \"x2\"\n";
    assert_eq!(
        problems(src),
        [
            "local/y: a local identity: server mode serves repositories by their remote",
            "a.com/../z: not a repository identity: a segment is empty, `.` or `..`",
            "a.com/x: listed twice",
        ]
    );
}

/// A whole manifest's `[[repository]]` entries, its other keys and sections unread, checked as a
/// workspace's text is (`meada workspace import`).
#[test]
fn a_manifest_s_entries_are_validated_alone() {
    let src = "index_dir = \"/srv\"\n\n[snapshot]\nbucket = \"b\"\n\n\
               [[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\n";
    let entries = validate_entries(FILE, src).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(entries.len(), 1);
    let src = "index_dir = \"/srv\"\n\n[[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\n\
               [[repository.peer]]\nrepo = \"a.com/y\"\npath = \"../y\"\n";
    assert_eq!(
        validate_entries(FILE, src).err().unwrap_or_default(),
        ["ops/meada.workspace.toml:8: `repository.peer.path` is not a meada.workspace.toml key"]
    );
    let src = "index_dir = \"/srv\"\n\n[[repository]]\nrepo = \"local/x\"\npath = \"x\"\n";
    assert_eq!(
        validate_entries(FILE, src).err().unwrap_or_default(),
        ["local/x: a local identity: server mode serves repositories by their remote"]
    );
}

const OWNED: &str = "[[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\n";

/// Each `owner` error names the line and the dotted key; the accepted names are compared as
/// they are written. (The pt-BR texts are the CLI's `cli` cases: the language is process-wide.)
#[test]
fn an_owner_is_one_string_of_a_group_name_and_each_error_names_its_line_and_key() {
    let long = "g".repeat(257);
    for (value, error) in [
        ("1".to_string(), "must be one string, the name of one group"),
        (
            "[\"a\", \"b\"]".into(),
            "must be one string, the name of one group",
        ),
        ("\"\"".into(), "is empty"),
        (format!("\"{long}\""), "is longer than 256 characters"),
        ("\"te\\u0007am\"".into(), "holds a control character"),
        ("\" team\"".into(), "has whitespace at either end"),
        ("\"team \"".into(), "has whitespace at either end"),
    ] {
        let src = format!("{OWNED}owner = {value}\n");
        assert_eq!(
            problems(&src),
            [format!("{FILE}:4: `repository.owner`: {error}")],
            "{value}"
        );
    }
    for owner in [
        "g".repeat(256),
        "g".to_string(),
        "Equipe de Cobrança, São Paulo".to_string(),
        "a b,c".to_string(),
    ] {
        let entries = validate(FILE, &format!("{OWNED}owner = \"{owner}\"\n")).unwrap();
        assert_eq!(entries[0].owner.as_deref(), Some(owner.as_str()));
    }
    assert_eq!(validate(FILE, OWNED).unwrap()[0].owner, None);
}

/// Each `added_by` error names the line and the dotted key; a login of 1 to 39 lowercase ASCII
/// letters, digits, `-` and `_` is accepted as written (#330).
#[test]
fn an_added_by_is_one_lowercase_login_and_each_error_names_its_line_and_key() {
    let long = "a".repeat(40);
    for (value, error) in [
        ("1".to_string(), "must be one string, a GitHub login"),
        ("\"\"".into(), "is empty"),
        (format!("\"{long}\""), "is longer than 39 characters"),
        (
            "\"Ana\"".into(),
            "holds a character other than lowercase ASCII letters, digits, `-` and `_`",
        ),
        (
            "\"ana.souza\"".into(),
            "holds a character other than lowercase ASCII letters, digits, `-` and `_`",
        ),
        (
            "\"ana souza\"".into(),
            "holds a character other than lowercase ASCII letters, digits, `-` and `_`",
        ),
    ] {
        let src = format!("{OWNED}added_by = {value}\n");
        assert_eq!(
            problems(&src),
            [format!("{FILE}:4: `repository.added_by`: {error}")],
            "{value}"
        );
    }
    for login in [
        "a".repeat(39),
        "a".into(),
        "ana-souza".into(),
        "a1_b-2".into(),
        "ana_acme".into(),
    ] {
        let entries = validate(FILE, &format!("{OWNED}added_by = \"{login}\"\n")).unwrap();
        assert_eq!(entries[0].added_by.as_deref(), Some(login.as_str()));
    }
    assert_eq!(validate(FILE, OWNED).unwrap()[0].added_by, None);
}

#[test]
fn a_named_peer_takes_an_environment_name_of_at_most_128_bytes() {
    let text = |name: &str| {
        format!(
            "[[repository]]\nrepo = \"example.com/acme/api\"\n\
             [[repository.peer]]\nrepo = \"example.com/acme/web\"\nbase_url_env = [\"{name}\"]\n"
        )
    };
    let longest = format!("W{}", "_".repeat(127));
    assert!(validate_named(FILE, &text(&longest), 10).is_ok());
    let past = format!("{longest}_");
    let problems = validate_named(FILE, &text(&past), 10)
        .err()
        .unwrap_or_default();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains(&past), "{problems:?}");
}

#[test]
fn a_value_of_the_wrong_type_is_named_by_key_and_kind_never_echoed() {
    let secret = "s3cr3t-token";
    // A string where a table is expected.
    let table = format!("[[repository]]\nrepo = \"example.com/acme/api\"\nserve = \"{secret}\"\n");
    // An integer where a string is expected.
    let string = "[[repository]]\nrepo = 12345678\n".to_string();
    let a = problems(&table).join("\n");
    assert!(
        a.contains("`repository.serve`: has the wrong type (a table was expected)"),
        "{a}"
    );
    assert!(!a.contains(secret), "{a}");
    let b = problems(&string).join("\n");
    assert!(
        b.contains("`repository.repo`: has the wrong type (a string was expected)"),
        "{b}"
    );
    assert!(!b.contains("12345678"), "{b}");
}

#[test]
fn every_kind_and_an_invalid_value_are_worded_without_the_value() {
    let kinds = [
        (
            "[[repository]]\nrepo = \"a.b/c/d\"\npeer = 9\n",
            "(an array was expected)",
        ),
        (
            "[[repository]]\nrepo = \"a.b/c/d\"\npath = true\n",
            "(a string was expected)",
        ),
    ];
    for (src, want) in kinds {
        let got = problems(src).join("\n");
        assert!(got.contains(want), "{got}");
    }
}

/// `unknown variant` is worded from the schema's own variants: the written value, even one that
/// imitates serde's text, never reaches the message (#545).
#[test]
fn an_unknown_variant_names_the_key_and_the_values_it_accepts() {
    let src = "[[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\n[[repository.resource]]\n\
               kind = \"pg\"\nid = \"a\"\nrole = \"x`, expected `y\"\n";
    let problems = validate(FILE, src).err().unwrap_or_default();
    assert_eq!(
        problems,
        [format!(
            "{FILE}:7: `repository.resource.role`: is not one of the values it accepts \
             (`read`, `write`, `read-write`)"
        )]
    );
}
