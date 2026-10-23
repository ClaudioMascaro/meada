//! Workspace changes at the public deterministic core seam shared by CLI and browser.

use meada_core::workspace::diff;

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
