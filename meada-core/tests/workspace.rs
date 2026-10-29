//! Workspace changes at the public deterministic core seam shared by CLI and browser.

use meada_core::workspace::{diff, entries_text, version_entries};
use serde_json::json;

#[test]
fn changes_name_entries_and_dotted_keys_in_manifest_order() {
    let before = "[[repository]]\nrepo = \"a.test/team/removed\"\npath = \"old\"\n\n\
                  [[repository]]\nrepo = \"a.test/team/web\"\npath = \"web\"\n\
                  [repository.serve]\nprefix = \"/old\"\nhosts = [\"old.test\"]\n";
    let after = "[[repository]]\nrepo = \"a.test/team/web\"\npath = \"new-web\"\n\
                 [repository.serve]\nhosts = [\"new.test\"]\nprefix = \"/new\"\n\n\
                 [[repository]]\nrepo = \"a.test/team/added\"\npath = \"added\"\n";
    let changes = diff("entries", before, after).unwrap();
    assert_eq!(changes.added, ["a.test/team/added"]);
    assert_eq!(changes.removed, ["a.test/team/removed"]);
    assert_eq!(changes.changed.len(), 1);
    assert_eq!(changes.changed[0].repo, "a.test/team/web");
    assert_eq!(
        changes.changed[0].keys,
        [
            "repository.path",
            "repository.serve.hosts",
            "repository.serve.prefix"
        ]
    );
}

#[test]
fn identity_and_nested_changes_ignore_formatting_and_keep_removed_keys() {
    let before = "[[repository]]\nrepo = 'a.test/team/web'\npath = 'web'\n\
                  [repository.serve]\nhosts = ['web.test']\nprefix = '/edge'\n\
                  [[repository.peer]]\nrepo = 'a.test/team/api'\nbase_url_env = ['API']\n";
    assert_eq!(diff("entries", before, before).unwrap(), Default::default());
    let after = "# a comment\n[[repository]]\npath = \"web\"\nrepo = \"a.test/team/web\"\n\
                 [[repository.peer]]\nrepo = \"a.test/team/api\"\nbase_url_env = [\"API_V2\"]\n\
                 [repository.serve]\nhosts = [\"web.test\"]\n";
    assert_eq!(
        diff("entries", before, after).unwrap().changed[0].keys,
        ["repository.peer.base_url_env", "repository.serve.prefix"]
    );
    assert_eq!(diff("entries", "", "").unwrap(), Default::default());
    assert_eq!(
        diff("entries", before, "[[repository]]\npath='x'\n").unwrap_err(),
        ["entries:1: `repository.repo` is missing"]
    );
}

#[test]
fn reordering_existing_entries_changes_the_repository_array_key() {
    let a = "[[repository]]\nrepo='a.test/team/a'\npath='a'\n";
    let b = "[[repository]]\nrepo='a.test/team/b'\npath='b'\n";
    let changes = diff("entries", &format!("{a}{b}"), &format!("{b}{a}")).unwrap();
    assert_eq!(
        changes
            .changed
            .iter()
            .map(|change| (change.repo.as_str(), change.keys.as_slice()))
            .collect::<Vec<_>>(),
        [
            ("a.test/team/b", ["repository".to_string()].as_slice()),
            ("a.test/team/a", ["repository".to_string()].as_slice())
        ]
    );
}

#[test]
fn a_peer_field_moved_between_rows_keeps_its_position_without_synthetic_key_indexes() {
    let before = "[[repository]]\nrepo='a.test/team/web'\npath='web'\n\
                  [[repository.peer]]\nrepo='a.test/team/a'\nbase_url_env=['API']\n\
                  [[repository.peer]]\nrepo='a.test/team/b'\n";
    let after = "[[repository]]\nrepo='a.test/team/web'\npath='web'\n\
                 [[repository.peer]]\nrepo='a.test/team/a'\n\
                 [[repository.peer]]\nrepo='a.test/team/b'\nbase_url_env=['API']\n";
    let changes = diff("entries", before, after).unwrap();
    assert_eq!(changes.changed.len(), 1);
    assert_eq!(changes.changed[0].keys, ["repository.peer.base_url_env"]);
}

#[test]
fn a_declared_empty_serve_table_keeps_its_presence_when_a_child_changes() {
    let entry = "[[repository]]\nrepo='a.test/team/web'\npath='web'\n";
    let empty = format!("{entry}[repository.serve]\n");
    let hosts = format!("{empty}hosts=['web.test']\n");
    for (before, after) in [(&empty, &hosts), (&hosts, &empty)] {
        assert_eq!(
            diff("entries", before, after).unwrap().changed[0].keys,
            ["repository.serve.hosts"]
        );
    }
    assert_eq!(
        diff("entries", entry, &empty).unwrap().changed[0].keys,
        ["repository.serve"]
    );
}

/// An entry's owner is printed right after its `path`, before its block tables, and the text
/// reads back to the same entries; entries with no owner print as they did before owners.
#[test]
fn entries_text_prints_the_owner_after_the_path() {
    let entries = [
        json!({"repo": "a.com/x", "path": "x", "owner": "team-x", "serve": {"prefix": "/x"}}),
        json!({"repo": "a.com/y", "path": "y"}),
    ];
    let text = entries_text(&entries).unwrap();
    assert_eq!(
        text,
        "[[repository]]\npath = \"x\"\nowner = \"team-x\"\nrepo = \"a.com/x\"\n\n\
         [repository.serve]\nprefix = \"/x\"\n\n\
         [[repository]]\npath = \"y\"\nrepo = \"a.com/y\"\n"
    );
    let ownerless = entries_text(&entries[1..]).unwrap();
    assert_eq!(
        ownerless,
        "[[repository]]\npath = \"y\"\nrepo = \"a.com/y\"\n"
    );
    let read = meada_core::manifest::validate("entries", &text).unwrap();
    assert_eq!(read[0].owner.as_deref(), Some("team-x"));
}

/// Two versions that differ only in an entry's owner list it as changed with the dotted key
/// `repository.owner`, set, cleared or replaced (ADR 0015, #325).
#[test]
fn an_owner_change_alone_is_the_dotted_key_repository_owner() {
    let entry = "[[repository]]\nrepo='a.test/team/web'\npath='web'\n";
    let owned = |owner: &str| {
        format!("[[repository]]\nrepo='a.test/team/web'\npath='web'\nowner='{owner}'\n")
    };
    for (before, after) in [
        (entry.to_string(), owned("team-a")),
        (owned("team-a"), entry.to_string()),
        (owned("team-a"), owned("team-b")),
    ] {
        let changes = diff("entries", &before, &after).unwrap();
        assert!(changes.added.is_empty() && changes.removed.is_empty());
        assert_eq!(changes.changed.len(), 1);
        assert_eq!(changes.changed[0].repo, "a.test/team/web");
        assert_eq!(changes.changed[0].keys, ["repository.owner"]);
    }
    assert_eq!(
        diff("entries", &owned("team-a"), &owned("team-a")).unwrap(),
        Default::default()
    );
}

/// The text read as a version document's entries: `repo`, `path`, the owner when there is one,
/// then the blocks, the form the authorization functions take.
#[test]
fn version_entries_reads_a_text_as_a_documents_entries() {
    let text = "[[repository]]\nrepo='a.test/x'\npath='x'\nowner='team-x'\n\
                [repository.serve]\nprefix='/x'\n\n\
                [[repository]]\nrepo='a.test/y'\npath='y'\n";
    assert_eq!(
        version_entries("entries", text).unwrap(),
        [
            json!({"repo": "a.test/x", "path": "x", "owner": "team-x", "serve": {"prefix": "/x"}}),
            json!({"repo": "a.test/y", "path": "y"}),
        ]
    );
    assert_eq!(
        version_entries("entries", "").unwrap(),
        Vec::<serde_json::Value>::new()
    );
    assert_eq!(
        version_entries("entries", "[[repository]]\npath='x'\n").unwrap_err(),
        ["entries:1: `repository.repo` is missing"]
    );
}
