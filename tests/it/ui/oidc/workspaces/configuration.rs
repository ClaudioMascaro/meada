use super::*;

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
    pub(super) fn bare(&self, short: &str) {
        let mirror = self.dir.join("mirrors").join(self.identity(short));
        crate::common::git(&mirror, &["rm", "-q", "meada.toml"]);
        crate::common::git(&mirror, &["commit", "-q", "-m", "no meada.toml"]);
    }

    pub(super) fn publish(&self) -> (String, Value) {
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

    fn configured(&self, entries: &[(&str, &str)]) -> String {
        let one = |(short, extra): &(&str, &str)| {
            format!(
                "[[repository]]\nrepo = \"{}\"\n{extra}",
                self.identity(short)
            )
        };
        entries.iter().map(one).collect::<Vec<_>>().join("\n")
    }

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

fn links(reply: &Reply) -> Vec<String> {
    assert_eq!(reply.status, 200, "{}", reply.body);
    (reply.json()["links"].as_array().unwrap().iter())
        .map(|l| l["reason"].as_str().unwrap().to_string())
        .collect()
}

fn peer_links(reply: &Reply) -> usize {
    links(reply)
        .iter()
        .filter(|reason| reason.contains("a base_url_env of peer"))
        .count()
}

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

    let alice_text = case.configured(&[
        ("Y", GATEWAY_PEER),
        ("A", "[repository.serve]\nhosts = [\"a.example\"]\n"),
    ]);
    let applied = case.change("alice", &ya, &alice_text);
    assert_eq!(applied.status, 200, "{}", applied.body);
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
    assert_eq!(codes(by), ["documents_not_indexed"]);
    let aa = entry(&alice, &a);
    assert_eq!(aa["source"], "repository");
    assert_eq!(aa["ignored"], json!(["[repository.serve]"]));
    assert_eq!(codes(aa), ["ignored_by_repository"]);
    assert_eq!(aa["blocks"], json!({}));
    assert_eq!(entry(&bob, &b)["source"], "repository");
    assert_eq!(entry(&bob, &b)["ignored"], json!([]));
    assert!(!alice.to_string().contains("bob.example") && !alice.to_string().contains(&b));
    assert!(!bob.to_string().contains("GATEWAY_URL") && !bob.to_string().contains(&a));

    let through_alice = case.context_export("alice", &ya, "Y", "");
    assert_eq!(peer_links(&through_alice), 3, "{}", through_alice.body);
    assert_eq!(peer_links(&case.context_export("bob", &yb, "Y", "")), 0);

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
    assert_eq!(peer_links(&case.context_export("alice", &ya, "Y", "")), 0);
    let pinned = case.context_export("alice", &ya, "Y", &format!("?generation={first}"));
    assert_eq!(peer_links(&pinned), 3);
    let was = case
        .effective("alice", &ya, &format!("?version={old_version}"))
        .json();
    assert_eq!(was["current"], false);
    assert_eq!(entry(&was, &y)["source"], "context");

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

#[test]
fn an_ignored_or_unsupported_declaration_never_looks_applied() {
    if let Some(case) = Case::new("ws_cfg_refusals") {
        an_ignored_or_unsupported_declaration_never_looks_applied_as(case);
    }
}

#[cfg(feature = "accounts")]
#[test]
fn local_accounts_an_ignored_or_unsupported_declaration_never_looks_applied() {
    if let Some(case) = Case::local("ws_cfg_refusals_local") {
        an_ignored_or_unsupported_declaration_never_looks_applied_as(case);
    }
}

fn an_ignored_or_unsupported_declaration_never_looks_applied_as(case: Case) {
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

    case.publish();
    let text = case.configured(&[
        ("Y", "[[repository.peer]]\nrepo = \"github.com/acme/b\"\n"),
        ("A", "[repository.serve]\nhosts = [\"a.example\"]\n"),
    ]);
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
