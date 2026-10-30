//! Context-specific configuration of named workspaces (#344, ADR 0021 contract 6), as public
//! processes: the console (`meada ui`) saves, applies and shows the effective configuration of
//! workspaces for the callers of the shared Y/A/B table (`tests/fixtures/named-workspaces.json`),
//! the job pulls, checks sponsors, indexes and publishes, and the console answers the context's
//! export. Two contexts share the repository Y (a mirror with no `meada.toml` of its own, so a
//! context's blocks apply) and each sees only its own configuration.

use super::*;

/// The id a creation answered.
fn id_of(reply: &Reply) -> String {
    assert!(matches!(reply.status, 200 | 201), "{}", reply.body);
    reply.json()["id"].as_str().unwrap().to_string()
}

fn code(reply: &Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

impl Case {
    /// Removes the `meada.toml` of the mirror of `short` and commits, before any refresh: the
    /// repository then declares nothing of its own and a context's blocks apply to it.
    fn bare(&self, short: &str) {
        let mirror = self.dir.join("mirrors").join(self.identity(short));
        crate::common::git(&mirror, &["rm", "-q", "meada.toml"]);
        crate::common::git(&mirror, &["commit", "-q", "-m", "no meada.toml"]);
    }

    /// The job's refresh to a published generation, pulled by the console: its id and document.
    fn publish(&self) -> (String, Value) {
        self.refresh();
        self.job_ok(&["index", "--workspace", MANIFEST]);
        let generation = self.job_ok(&["snapshot", "publish", "--workspace", MANIFEST]);
        let generation = generation.trim().to_string();
        let (stdout, stderr, code) = pull(&self.dir, MANIFEST, &[]);
        assert_eq!(code, Some(0), "{stdout} {stderr}");
        let document = fs::read(
            self.dir
                .join("console-generations")
                .join(&generation)
                .join("generation.json"),
        )
        .unwrap();
        (generation, serde_json::from_slice(&document).unwrap())
    }

    /// The manifest text of `entries`: each (short, extra TOML after its `repo` line).
    fn configured(&self, entries: &[(&str, &str)]) -> String {
        let one = |(short, extra): &(&str, &str)| {
            format!(
                "[[repository]]\nrepo = \"{}\"\n{extra}",
                self.identity(short)
            )
        };
        entries.iter().map(one).collect::<Vec<_>>().join("\n")
    }

    /// Saves and applies `text` to `workspace` as `caller`, at the base it reads.
    fn change(&self, caller: &str, workspace: &str, text: &str) -> Reply {
        let shown = self.get(
            &format!("/api/v1/workspaces/{workspace}"),
            &self.token(caller),
        );
        let base = shown.json()["base"].as_i64().unwrap();
        let saved = self.save(caller, workspace, text, base);
        assert!(matches!(saved.status, 200 | 201), "{}", saved.body);
        let version = saved.json()["version"].as_str().unwrap().to_string();
        self.apply(caller, workspace, &version, base)
    }

    fn effective(&self, caller: &str, workspace: &str, query: &str) -> Reply {
        let path = format!("/api/v1/workspaces/{workspace}/effective{query}");
        self.get(&path, &self.token(caller))
    }

    fn context_export(&self, caller: &str, workspace: &str, short: &str, query: &str) -> Reply {
        let identity = self.identity(short).replace('/', "%2F");
        let path = format!("/api/v1/workspaces/{workspace}/repositories/{identity}/export{query}");
        self.get(&path, &self.token(caller))
    }
}

/// The entry of `repo` in an effective view.
fn entry<'a>(view: &'a Value, repo: &str) -> &'a Value {
    (view["entries"].as_array().unwrap().iter())
        .find(|e| e["repo"] == json!(repo))
        .unwrap_or_else(|| panic!("{repo} is not in {view}"))
}

fn codes(entry: &Value) -> Vec<String> {
    (entry["diagnostics"].as_array().unwrap().iter())
        .map(|d| d["code"].as_str().unwrap().to_string())
        .collect()
}

const GATEWAY_PEER: &str =
    "[[repository.peer]]\nrepo = \"github.com/acme/a\"\nbase_url_env = [\"GATEWAY_URL\"]\n";

/// The links of the export in `reply`.
fn links(reply: &Reply) -> Vec<String> {
    assert_eq!(reply.status, 200, "{}", reply.body);
    (reply.json()["links"].as_array().unwrap().iter())
        .map(|l| l["reason"].as_str().unwrap().to_string())
        .collect()
}

/// The links of the export whose reason names a declared peer.
fn peer_links(reply: &Reply) -> usize {
    links(reply)
        .iter()
        .filter(|reason| reason.contains("a base_url_env of peer"))
        .count()
}

/// Two contexts share Y, a repository with no `meada.toml`: each declares its own peers and
/// serve hosts for it and the effective view, the generation and the context's export show only
/// that context's. A change of configuration is a new generation of the same bundles, a pinned
/// generation keeps the configuration it recorded, a stale base refuses, and the other context
/// does not move.
#[test]
fn two_contexts_sharing_y_read_only_their_own_configuration() {
    let Some(case) = Case::new("ws_cfg_contexts") else {
        return;
    };
    case.bare("Y");
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let (y, a, b) = (case.identity("Y"), case.identity("A"), case.identity("B"));

    // Each group declares its own configuration for the shared Y. A declares serve hosts too,
    // but its repository has a `meada.toml` of its own: those are ignored, and the answer to the
    // apply says so beside the success.
    let alice_text = case.configured(&[
        ("Y", GATEWAY_PEER),
        ("A", "[repository.serve]\nhosts = [\"a.example\"]\n"),
    ]);
    let applied = case.change("alice", &ya, &alice_text);
    assert_eq!(applied.status, 200, "{}", applied.body);
    // Nothing was ingested yet: the accepted apply says the context is stored, not in effect,
    // and that the repositories' configuration is not yet known to take hold.
    let said = applied.json();
    assert_eq!(said["state"], "applied");
    let findings: Vec<(&str, &str)> = (said["diagnostics"].as_array().unwrap().iter())
        .map(|d| (d["repo"].as_str().unwrap(), d["code"].as_str().unwrap()))
        .collect();
    assert_eq!(
        findings,
        [
            (y.as_str(), "unknown_until_ingested"),
            (a.as_str(), "unknown_until_ingested")
        ]
    );
    let bob_text = case.configured(&[
        ("Y", "[repository.serve]\nhosts = [\"bob.example\"]\n"),
        ("B", ""),
    ]);
    assert_eq!(case.change("bob", &yb, &bob_text).status, 200);

    // Before the job ingests anything, nothing is claimed to be in effect: the entries are
    // pending ingestion and the workspace is applied, not live.
    let pending = case.effective("alice", &ya, "").json();
    assert_eq!(pending["state"], "applied");
    assert_eq!(pending["recorded"], false);
    assert_eq!(pending["generation"], Value::Null);
    for entry in pending["entries"].as_array().unwrap() {
        assert_eq!(entry["coverage"], "pending_ingestion", "{pending}");
        assert_eq!(codes(entry), ["unknown_until_ingested"], "{pending}");
    }

    let (first, document) = case.publish();
    let alice = case.effective("alice", &ya, "").json();
    let bob = case.effective("bob", &yb, "").json();
    assert_eq!(
        (alice["state"].as_str(), bob["state"].as_str()),
        (Some("live"), Some("live"))
    );
    assert_eq!(alice["generation"], json!(first));
    assert_eq!(
        (alice["recorded"].clone(), bob["recorded"].clone()),
        (json!(true), json!(true))
    );
    // Y: the context's blocks apply, each its own.
    let (ay, by) = (entry(&alice, &y), entry(&bob, &y));
    assert_eq!(ay["source"], "context");
    assert_eq!(
        ay["blocks"],
        json!({"peer": [{"base_url_env": ["GATEWAY_URL"], "repo": a}]})
    );
    assert_eq!(
        (ay["coverage"].clone(), codes(ay)),
        (json!("indexed"), Vec::<String>::new())
    );
    assert_eq!(by["source"], "context");
    assert_eq!(by["blocks"], json!({"serve": {"hosts": ["bob.example"]}}));
    // Y has no documents indexed, so hosts declared for it do not come from any document.
    assert_eq!(codes(by), ["documents_not_indexed"]);
    // A: its own `meada.toml` wins whole, its context blocks are ignored and said so.
    let aa = entry(&alice, &a);
    assert_eq!(aa["source"], "repository");
    assert_eq!(aa["ignored"], json!(["[repository.serve]"]));
    assert_eq!(codes(aa), ["ignored_by_repository"]);
    assert_eq!(aa["blocks"], json!({}));
    // B: its own `meada.toml` and nothing of the context's to ignore.
    assert_eq!(entry(&bob, &b)["source"], "repository");
    assert_eq!(entry(&bob, &b)["ignored"], json!([]));
    // No contamination: neither view carries the other context's blocks or repositories.
    assert!(!alice.to_string().contains("bob.example") && !alice.to_string().contains(&b));
    assert!(!bob.to_string().contains("GATEWAY_URL") && !bob.to_string().contains(&a));

    // The same repository answers differently through each context: Y's calls to the gateway
    // link through alice's declared peer and through nothing in bob's.
    let through_alice = case.context_export("alice", &ya, "Y", "");
    assert_eq!(peer_links(&through_alice), 3, "{}", through_alice.body);
    assert_eq!(peer_links(&case.context_export("bob", &yb, "Y", "")), 0);

    // One bundle of Y serves both contexts, recorded with the ingestion blocks alone.
    let members: Vec<(&str, &str)> = (document["members"].as_array().unwrap().iter())
        .map(|m| {
            (
                m["identity"].as_str().unwrap(),
                m["bundle"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(members.iter().filter(|(i, _)| *i == y).count(), 1);
    assert!((document["members"].as_array().unwrap().iter()).all(|m| m["blocks"] == json!({})));
    let recorded = |document: &Value, id: &str| -> Value {
        (document["contexts"].as_array().unwrap().iter())
            .find(|c| c["id"] == json!(id))
            .unwrap()
            .clone()
    };
    assert_eq!(
        recorded(&document, &ya)["repositories"][0]["blocks"],
        ay["blocks"]
    );
    assert_eq!(
        recorded(&document, &yb)["repositories"][0]["blocks"],
        by["blocks"]
    );

    // Alice drops the peer. A stale base is refused, naming nothing changed; her change is a
    // new generation of the same bundles and bob's context does not move.
    let before = case
        .get(&format!("/api/v1/workspaces/{ya}"), &case.token("alice"))
        .json();
    let (old_version, old_base) = (
        before["version"].as_str().unwrap().to_string(),
        before["base"].as_i64().unwrap(),
    );
    let plain = case.configured(&[("Y", ""), ("A", "")]);
    assert_eq!(case.change("alice", &ya, &plain).status, 200);
    let stale = case.save("alice", &ya, &alice_text, old_base);
    assert_eq!(
        (stale.status, code(&stale)),
        (409, "stale_base".into()),
        "{}",
        stale.body
    );
    let bob_base = case
        .get(&format!("/api/v1/workspaces/{yb}"), &case.token("bob"))
        .json()["base"]
        .clone();
    assert_eq!(bob_base, json!(2));

    // Applied, not yet published: the view says the generation has not recorded it.
    let waiting = case.effective("alice", &ya, "").json();
    assert_eq!(
        (waiting["recorded"].clone(), waiting["state"].clone()),
        (json!(false), json!("applied"))
    );
    assert_eq!(
        case.effective("alice", &ya, &format!("?version={old_version}"))
            .json()["recorded"],
        true
    );
    let (second, next) = case.publish();
    assert_ne!(first, second);
    assert_eq!(
        next["members"], document["members"],
        "the bundles are the same"
    );
    assert_eq!(recorded(&next, &yb), recorded(&document, &yb));
    assert_ne!(recorded(&next, &ya), recorded(&document, &ya));
    let now = case.effective("alice", &ya, "").json();
    assert_eq!(entry(&now, &y)["source"], "none");
    assert_eq!(
        (now["recorded"].clone(), now["version"].clone()),
        (json!(true), json!(recorded(&next, &ya)["version"].clone()))
    );
    // The pinned generation keeps the configuration it recorded; the current one has none.
    assert_eq!(peer_links(&case.context_export("alice", &ya, "Y", "")), 0);
    let pinned = case.context_export("alice", &ya, "Y", &format!("?generation={first}"));
    assert_eq!(peer_links(&pinned), 3);
    // An earlier version is read as it was, without being the current one.
    let was = case
        .effective("alice", &ya, &format!("?version={old_version}"))
        .json();
    assert_eq!(was["current"], false);
    assert_eq!(entry(&was, &y)["source"], "context");

    // Edits and reads of a context are the operators' and readers' of that context alone: a
    // peer who reads it whole sees the view without the operators' extras, anyone else the one
    // 404 of an id that does not exist.
    let unknown = case.get(
        "/api/v1/workspaces/0123456789abcdef/effective",
        &case.token("root"),
    );
    assert_eq!(unknown.status, 404);
    for caller in ["bob", "yoda"] {
        let hidden = case.effective(caller, &ya, "");
        assert_eq!(
            (hidden.status, &hidden.body),
            (404, &unknown.body),
            "{caller}"
        );
    }
    let hidden_edit = case.save("bob", &ya, &alice_text, 2);
    assert_eq!(
        (hidden_edit.status, &hidden_edit.body),
        (404, &unknown.body)
    );
    let viewer = case.effective("erin", &ya, "");
    assert_eq!(viewer.status, 200, "{}", viewer.body);
    assert!(viewer.json().get("hidden").is_none());
    let erin_edit = case.save("erin", &ya, &alice_text, 3);
    assert_eq!(
        (erin_edit.status, code(&erin_edit)),
        (403, "not_operator".into())
    );
    case.github.clean(&case.console.stderr(), &[]);
}

/// A declaration that cannot take effect is refused or said, never silently applied: reserved
/// keys, values where a reference belongs and unknown keys are refused before anything is
/// stored; a block a repository's own file overrides is reported beside the accepted apply; a
/// preview of text shows the view before anything is saved and stores nothing.
#[test]
fn an_ignored_or_unsupported_declaration_never_looks_applied() {
    let Some(case) = Case::new("ws_cfg_refusals") else {
        return;
    };
    case.bare("Y");
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let base = |case: &Case| {
        let shown = case
            .get(&format!("/api/v1/workspaces/{ya}"), &case.token("alice"))
            .json();
        shown["base"].as_i64().unwrap()
    };
    assert_eq!(base(&case), 1);
    // Reserved keys, a value in place of a variable's name, a secret-looking reference: 422, and
    // nothing moves.
    for extra in [
        "[repository.serve]\ndocuments = [\"docs/api.yaml\"]\n",
        "path = \"/etc\"\n",
        "owner = \"team\"\n",
        "added_by = \"alice\"\n",
        "[[repository.peer]]\nrepo = \"github.com/acme/a\"\nbase_url_env = [\"GATEWAY_URL=https://x\"]\n",
        "[[repository.peer]]\nrepo = \"github.com/acme/a\"\nbase_url_env = [\"https://user:pw@x\"]\n",
        "[[repository.peer]]\nrepo = \"github.com/acme/a\"\npath = \"../a\"\n",
        "[repository.serve]\nmirror = \"/srv\"\n",
    ] {
        let text = case.configured(&[("Y", extra), ("A", "")]);
        let refused = case.save("alice", &ya, &text, 1);
        assert_eq!(
            (refused.status, code(&refused)),
            (422, "invalid_entries".into()),
            "{extra}: {}",
            refused.body
        );
        let preview = case.post(
            "/api/v1/workspaces/effective",
            &case.token("alice"),
            &json!({"entries": text}),
        );
        assert_eq!(
            (preview.status, code(&preview)),
            (422, "invalid_entries".into()),
            "{extra}"
        );
    }
    assert_eq!(base(&case), 1);

    // A published generation lets the view say what a repository's own file overrides.
    case.publish();
    let text = case.configured(&[
        ("Y", "[[repository.peer]]\nrepo = \"github.com/acme/b\"\n"),
        ("A", "[repository.serve]\nhosts = [\"a.example\"]\n"),
    ]);
    // Before saving: the preview computes the same view the saved version will show, with the
    // missing peer and the ignored block, and keeps nothing.
    let preview = case.post(
        "/api/v1/workspaces/effective",
        &case.token("alice"),
        &json!({"entries": text}),
    );
    assert_eq!(preview.status, 200, "{}", preview.body);
    let view = preview.json();
    let (y, a) = (case.identity("Y"), case.identity("A"));
    assert_eq!(codes(entry(&view, &y)), ["peer_missing"]);
    assert_eq!(codes(entry(&view, &a)), ["ignored_by_repository"]);
    assert_eq!(base(&case), 1);
    let saved = case.change("alice", &ya, &text);
    assert_eq!(saved.status, 200, "{}", saved.body);
    let said: Vec<(String, String)> = (saved.json()["diagnostics"].as_array().unwrap().iter())
        .map(|d| {
            (
                d["repo"].as_str().unwrap().into(),
                d["code"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            (y.clone(), "peer_missing".into()),
            (a.clone(), "ignored_by_repository".into())
        ]
    );
    let shown = case.effective("alice", &ya, "").json();
    assert_eq!(entry(&shown, &a)["ignored"], json!(["[repository.serve]"]));

    // A caller who reads only Y learns nothing of A from a preview: it is pending, as any
    // repository no generation holds is. A forged key and an anonymous call are refused.
    let yoda = case.post(
        "/api/v1/workspaces/effective",
        &case.token("yoda"),
        &json!({"entries": text}),
    );
    let seen = yoda.json();
    assert_eq!(entry(&seen, &y)["coverage"], "indexed");
    assert_eq!(entry(&seen, &a)["coverage"], "pending_ingestion");
    assert_eq!(codes(entry(&seen, &a)), ["unknown_until_ingested"]);
    let forged = case.post(
        "/api/v1/workspaces/effective",
        &case.token("alice"),
        &json!({"entries": text, "actor": "root", "role": "operator"}),
    );
    assert_eq!((forged.status, code(&forged)), (400, "bad_request".into()));
    let anonymous = send_body(
        &case.console,
        "POST",
        "/api/v1/workspaces/effective",
        &[("Origin", PUBLIC)],
        "{}",
    );
    assert_eq!(anonymous.status, 401);
    case.github.clean(&case.console.stderr(), &[]);
}
