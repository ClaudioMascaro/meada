//! The ids at the crate's public seam: the bundle and generation ids the `snapshot` module's
//! tests expected before the split, and the workspace version id.

use meada_core::ids::{bundle_id, entry_blocks, fence, generation_id, is_id, workspace_version_id};
use serde_json::{Value, json};

fn short(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex()[..16].to_string()
}

/// Ids of a fixed input, against the canonical texts written out here: any change to the
/// format shows.
#[test]
fn same_inputs_give_the_same_ids_on_any_machine() {
    let blocks = r#"{"serve":{"documents":["openapi.yaml"]}}"#;
    let head = "0123456789abcdef0123456789abcdef01234567";
    let bundle = bundle_id("example.com/acme/web", head, 7, "0.1.0", blocks);
    let text = "meada bundle\nidentity example.com/acme/web\n\
                head 0123456789abcdef0123456789abcdef01234567\nschema 7\nbinary 0.1.0\n\
                blocks {\"serve\":{\"documents\":[\"openapi.yaml\"]}}\n";
    assert_eq!(bundle, short(text));
    assert!(is_id(&bundle), "{bundle}");
    let api = ("example.com/acme/api", "0000000000000000");
    let generation = generation_id(&[("example.com/acme/web", &bundle), api]);
    let lines = format!("example.com/acme/api 0000000000000000\nexample.com/acme/web {bundle}\n");
    assert_eq!(generation, short(&lines));
    // The order the repositories come in is no input.
    assert_eq!(
        generation,
        generation_id(&[api, ("example.com/acme/web", &bundle)])
    );
}

fn base() -> String {
    bundle_id("example.com/acme/web", "aaaa", 7, "0.1.0", "{}")
}

#[test]
fn another_identity_changes_the_bundle_id() {
    assert_ne!(
        base(),
        bundle_id("example.com/acme/api", "aaaa", 7, "0.1.0", "{}")
    );
}

#[test]
fn another_head_changes_the_bundle_id() {
    assert_ne!(
        base(),
        bundle_id("example.com/acme/web", "bbbb", 7, "0.1.0", "{}")
    );
}

#[test]
fn another_schema_version_changes_the_bundle_id() {
    assert_ne!(
        base(),
        bundle_id("example.com/acme/web", "aaaa", 9, "0.1.0", "{}")
    );
}

#[test]
fn another_binary_version_changes_the_bundle_id() {
    assert_ne!(
        base(),
        bundle_id("example.com/acme/web", "aaaa", 7, "0.2.0", "{}")
    );
}

#[test]
fn other_entry_blocks_change_the_bundle_id() {
    let blocks = "{\"peer\":[{\"repo\":\"example.com/acme/api\"}]}";
    assert_ne!(
        base(),
        bundle_id("example.com/acme/web", "aaaa", 7, "0.1.0", blocks)
    );
}

#[test]
fn another_bundle_id_or_identity_changes_the_generation_id() {
    let generation = generation_id(&[("example.com/acme/web", "1111111111111111")]);
    assert_ne!(
        generation,
        generation_id(&[("example.com/acme/web", "2222222222222222")])
    );
    assert_ne!(
        generation,
        generation_id(&[("example.com/acme/api", "1111111111111111")])
    );
    let two = [
        ("example.com/acme/web", "1111111111111111"),
        ("example.com/acme/api", "1111111111111111"),
    ];
    assert_ne!(generation, generation_id(&two));
}

#[test]
fn entry_blocks_leave_out_repo_path_and_owner_and_sort_their_keys() {
    let a = "index_dir = \"i\"\n[[repository]]\nrepo = \"example.com/acme/web\"\npath = \"../web\"\n\
             [repository.serve]\ndomains = [\"web.example.com\"]\ndocuments = [\"openapi.yaml\"]\n\
             [[repository]]\nrepo = \"example.com/acme/api\"\npath = \"api\"\n";
    let b = "index_dir = \"j\"\n[[repository]]\npath = \"/srv/web\"\n\
             serve = { documents = ['openapi.yaml'], domains = ['web.example.com'] }\n\
             repo = \"example.com/acme/web\"\nowner = \"team-web\"\n\
             [[repository]]\nrepo = \"example.com/acme/api\"\npath = \"/srv/api\"\n";
    let (a, b) = (entry_blocks(a), entry_blocks(b));
    assert_eq!(a, b);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        "[{\"serve\":{\"documents\":[\"openapi.yaml\"],\"domains\":[\"web.example.com\"]}},{}]"
    );
}

#[test]
fn ids_have_the_shared_shape() {
    assert!(is_id("0123456789abcdef"));
    for bad in [
        "0123456789ABCDEF",
        "0123456789abcde",
        "../../../../etc/",
        "",
    ] {
        assert!(!is_id(bad), "{bad}");
    }
}

#[test]
fn a_fence_is_decimal_digits_within_a_u64() {
    assert_eq!(fence("0"), Some(0));
    assert_eq!(fence("18446744073709551615"), Some(u64::MAX));
    for bad in ["", "-1", "+1", " 1", "1.0", "0x10", "18446744073709551616"] {
        assert_eq!(fence(bad), None, "{bad}");
    }
}

fn web() -> Value {
    json!({"serve": {"documents": ["openapi.yaml"]}})
}

fn api() -> Value {
    json!({"peer": [{"repo": "example.com/acme/web"}]})
}

/// The id of a fixed input, against its canonical text written out here: nothing but the
/// entries is hashed (no author, time or message), so any change to the format shows.
#[test]
fn the_workspace_version_id_hashes_the_canonical_text_of_the_entries_alone() {
    let (web, api) = (web(), api());
    let entries = [
        ("example.com/acme/web", "../web", &web, None, None),
        ("example.com/acme/api", "/srv/api", &api, None, None),
    ];
    let id = workspace_version_id(&entries);
    let text = "[{\"blocks\":{\"serve\":{\"documents\":[\"openapi.yaml\"]}},\"path\":\"../web\",\
                \"repo\":\"example.com/acme/web\"},{\"blocks\":{\"peer\":[{\"repo\":\
                \"example.com/acme/web\"}]},\"path\":\"/srv/api\",\"repo\":\"example.com/acme/api\"}]";
    assert_eq!(id, short(text));
    assert!(is_id(&id), "{id}");
    assert_eq!(id, workspace_version_id(&entries));
}

#[test]
fn the_same_entries_written_another_way_give_the_same_workspace_version_id() {
    let of = |src: &str| {
        let blocks = entry_blocks(src);
        let entries: Vec<_> = (blocks.iter())
            .zip([
                ("example.com/acme/web", "../web"),
                ("example.com/acme/api", "api"),
            ])
            .map(|(blocks, (repo, path))| (repo, path, blocks, None, None))
            .collect();
        workspace_version_id(&entries)
    };
    let a = "[[repository]]\nrepo = \"example.com/acme/web\"\npath = \"../web\"\n\
             [repository.serve]\nhosts = [\"web.example.com\"]\ndocuments = [\"openapi.yaml\"]\n\
             [[repository]]\nrepo = \"example.com/acme/api\"\npath = \"api\"\n";
    let b = "[[repository]]\npath = \"../web\"\nrepo = \"example.com/acme/web\"\n\
             serve = { documents = ['openapi.yaml'], hosts = ['web.example.com'] }\n\
             [[repository]]\nrepo = \"example.com/acme/api\"\npath = \"api\"\n";
    assert_eq!(of(a), of(b));
}

#[test]
fn swapping_two_entries_or_changing_a_block_changes_the_workspace_version_id() {
    let (web, api) = (web(), api());
    let id = workspace_version_id(&[
        ("example.com/acme/web", "web", &web, None, None),
        ("example.com/acme/api", "api", &api, None, None),
    ]);
    let swapped = workspace_version_id(&[
        ("example.com/acme/api", "api", &api, None, None),
        ("example.com/acme/web", "web", &web, None, None),
    ]);
    assert_ne!(id, swapped);
    let other = json!({"serve": {"documents": ["openapi.json"]}});
    let changed = workspace_version_id(&[
        ("example.com/acme/web", "web", &other, None, None),
        ("example.com/acme/api", "api", &api, None, None),
    ]);
    assert_ne!(id, changed);
    let moved = workspace_version_id(&[
        ("example.com/acme/web", "../web", &web, None, None),
        ("example.com/acme/api", "api", &api, None, None),
    ]);
    assert_ne!(id, moved);
}

/// An entry without an owner has the canonical text it had before owners; an owner joins it
/// between `blocks` and `path`, and is compared exactly.
#[test]
fn an_owner_joins_the_canonical_text_and_changes_the_workspace_version_id() {
    let (web, api) = (web(), api());
    let of = |owner: Option<&str>| {
        workspace_version_id(&[
            ("example.com/acme/web", "../web", &web, owner, None),
            ("example.com/acme/api", "/srv/api", &api, None, None),
        ])
    };
    let ownerless = "[{\"blocks\":{\"serve\":{\"documents\":[\"openapi.yaml\"]}},\"path\":\"../web\",\
                     \"repo\":\"example.com/acme/web\"},{\"blocks\":{\"peer\":[{\"repo\":\
                     \"example.com/acme/web\"}]},\"path\":\"/srv/api\",\"repo\":\"example.com/acme/api\"}]";
    assert_eq!(of(None), short(ownerless));
    let owned = ownerless.replacen(
        "},\"path\":\"../web\"",
        "},\"owner\":\"team-web\",\"path\":\"../web\"",
        1,
    );
    assert_eq!(of(Some("team-web")), short(&owned));
    let (a, b) = (of(Some("Team-Web")), of(Some("team-web")));
    assert_ne!(a, b);
    assert_ne!(of(None), b);
    assert_ne!(of(None), of(Some("")));
}

/// An `added_by` joins the canonical text (keys sorted, so before `blocks`) and changes the
/// workspace version id; an entry without it keeps the text it had, and it is compared exactly.
#[test]
fn an_added_by_joins_the_canonical_text_and_changes_the_workspace_version_id() {
    let (web, api) = (web(), api());
    let of = |owner: Option<&str>, added_by: Option<&str>| {
        workspace_version_id(&[
            ("example.com/acme/web", "../web", &web, owner, added_by),
            ("example.com/acme/api", "/srv/api", &api, None, None),
        ])
    };
    let plain = "[{\"blocks\":{\"serve\":{\"documents\":[\"openapi.yaml\"]}},\"path\":\"../web\",\
                 \"repo\":\"example.com/acme/web\"},{\"blocks\":{\"peer\":[{\"repo\":\
                 \"example.com/acme/web\"}]},\"path\":\"/srv/api\",\"repo\":\"example.com/acme/api\"}]";
    assert_eq!(of(None, None), short(plain));
    let added = plain.replacen("[{\"blocks\"", "[{\"added_by\":\"ana-souza\",\"blocks\"", 1);
    assert_eq!(of(None, Some("ana-souza")), short(&added));
    let both = added.replacen(
        "},\"path\":\"../web\"",
        "},\"owner\":\"team-web\",\"path\":\"../web\"",
        1,
    );
    assert_eq!(of(Some("team-web"), Some("ana-souza")), short(&both));
    assert_ne!(of(None, Some("ana-souza")), of(None, Some("ana-souza2")));
    assert_ne!(of(None, Some("ana-souza")), of(Some("ana-souza"), None));
}

#[test]
fn named_workspaces_change_the_generation_id_and_their_absence_changes_nothing() {
    use meada_core::ids::{generation_id, generation_id_of};
    let bundles = [
        ("h/o/web", "0123456789abcdef"),
        ("h/o/api", "fedcba9876543210"),
    ];
    // Without members and contexts the id is the one every earlier generation has.
    assert_eq!(
        generation_id_of(&bundles, &[], &[]),
        generation_id(&bundles)
    );
    let member = [("h/o/lib", "1111111111111111")];
    let context = [("aaaaaaaaaaaaaaaa", "2222222222222222")];
    let with_member = generation_id_of(&bundles, &member, &[]);
    let with_context = generation_id_of(&bundles, &member, &context);
    assert_ne!(with_member, generation_id(&bundles));
    assert_ne!(with_context, with_member);
    // A repository listed as a member is not the same fact as one the deployment lists.
    assert_ne!(
        generation_id_of(&bundles, &member, &[]),
        generation_id_of(&[bundles[0], bundles[1], member[0]], &[], &[])
    );
    // Order never matters.
    assert_eq!(
        generation_id_of(&[bundles[1], bundles[0]], &member, &context),
        generation_id_of(&bundles, &member, &context)
    );
}

#[test]
fn a_workspace_id_hashes_its_creator_name_and_first_version_only() {
    use meada_core::ids::{is_id, workspace_id};
    let id = workspace_id("alice", "Payments", "0123456789abcdef");
    assert!(is_id(&id));
    assert_eq!(id, workspace_id("alice", "Payments", "0123456789abcdef"));
    for other in [
        workspace_id("bob", "Payments", "0123456789abcdef"),
        workspace_id("alice", "Payment", "0123456789abcdef"),
        workspace_id("alice", "Payments", "fedcba9876543210"),
    ] {
        assert_ne!(id, other);
    }
}
