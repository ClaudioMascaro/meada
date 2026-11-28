use super::*;

fn id_of(reply: &Reply) -> String {
    assert!(matches!(reply.status, 200 | 201), "{}", reply.body);
    reply.json()["id"].as_str().unwrap().to_string()
}

impl Case {
    fn hosted(&self, entries: &[(&str, Option<&str>)]) -> String {
        let one = |(short, host): &(&str, Option<&str>)| {
            let mut text = format!("[[repository]]\nrepo = \"{}\"\n", self.identity(short));
            if let Some(host) = host {
                text += &format!("[repository.serve]\nhosts = [\"{host}\"]\n");
            }
            text
        };
        entries.iter().map(one).collect::<Vec<_>>().join("\n")
    }

    pub(super) fn save(&self, caller: &str, workspace: &str, text: &str, base: i64) -> Reply {
        let body = json!({"entries": text, "message": "edit", "base": base});
        let path = format!("/api/v1/workspaces/{workspace}/versions");
        self.post(&path, &self.token(caller), &body)
    }

    pub(super) fn apply(&self, caller: &str, workspace: &str, version: &str, base: i64) -> Reply {
        let body = json!({"version": version, "base": base});
        let path = format!("/api/v1/workspaces/{workspace}/apply");
        self.post(&path, &self.token(caller), &body)
    }

    fn show(&self, caller: &str, workspace: &str) -> Reply {
        self.get(
            &format!("/api/v1/workspaces/{workspace}"),
            &self.token(caller),
        )
    }

    fn at(&self, caller: &str, workspace: &str) -> (String, i64) {
        let shown = self.show(caller, workspace);
        assert_eq!(shown.status, 200, "{}", shown.body);
        let shown = shown.json();
        (
            shown["version"].as_str().unwrap().to_string(),
            shown["base"].as_i64().unwrap(),
        )
    }

    pub(super) fn revoke(&self, revoked: &[(&str, &str)]) {
        let (caller, short) = revoked.last().copied().unwrap();
        self.grant_all_but(revoked);
        self.roles();
        self.await_check(caller, short, "unavailable");
    }

    pub(super) fn grant_all_but(&self, revoked: &[(&str, &str)]) {
        for (short, identity) in self.table["identities"].as_object().unwrap() {
            let repo = identity
                .as_str()
                .unwrap()
                .strip_prefix("github.com/")
                .unwrap();
            let rows: Vec<(&str, &str)> = self.table["callers"]
                .as_object()
                .unwrap()
                .iter()
                .flat_map(|(name, caller)| {
                    let login = caller["login"].as_str();
                    let rows = caller["rows"].as_array().unwrap().iter();
                    let revoked = revoked.to_vec();
                    rows.filter(move |row| row["identity"] == short.as_str())
                        .filter(move |_| !revoked.contains(&(name.as_str(), short.as_str())))
                        .map(move |row| (login.unwrap(), row["role"].as_str().unwrap()))
                })
                .collect();
            self.github.list(repo, &rows);
        }
    }

    pub(super) fn await_check(&self, caller: &str, short: &str, state: &str) {
        let token = self.token(caller);
        let identity = self.identity(short);
        let start = Instant::now();
        loop {
            let reply = self.post(
                "/api/v1/catalog/check",
                &token,
                &json!({"identities": [identity]}),
            );
            if reply.json()["results"][0]["state"] == json!(state) {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "{}", reply.body);
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn audited(&self, id: &str) -> Vec<Value> {
        let reply = self.get("/api/v1/audit?limit=100", &self.token("root"));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let mut records: Vec<Value> = (reply.json()["records"].as_array().unwrap().iter())
            .filter(|r| r["arguments"]["workspace"] == json!(id))
            .cloned()
            .collect();
        records.reverse();
        records
    }
}

fn code(reply: &Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

#[test]
fn versions_are_saved_applied_and_rolled_back_along_the_base() {
    if let Some(case) = Case::new("ws_life") {
        versions_are_saved_applied_and_rolled_back_along_the_base_as(case);
    }
}

#[cfg(feature = "accounts")]
#[test]
fn local_accounts_versions_are_saved_applied_and_rolled_back_along_the_base() {
    if let Some(case) = Case::local("ws_life_local") {
        versions_are_saved_applied_and_rolled_back_along_the_base_as(case);
    }
}

#[cfg(feature = "accounts")]
#[test]
fn local_accounts_a_suspended_operator_operates_no_workspace() {
    let Some(case) = Case::local("ws_suspended_local") else {
        return;
    };
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let admin = case.token("admin");
    let set = |action: &str| {
        let body = json!({"action": action});
        let reply = case.post("/api/v1/accounts/alice", &admin, &body);
        assert_eq!(reply.status, 200, "{action}: {}", reply.body);
    };
    let rename = |name: &str, base: i64| {
        let body = json!({"name": name, "base": base});
        let path = format!("/api/v1/workspaces/{ya}/rename");
        case.post(&path, &case.token("alice"), &body)
    };
    set("suspend");
    assert_eq!(case.show("alice", &ya).status, 200);
    let refused = rename("Suspended", 1);
    assert_eq!(
        (refused.status, code(&refused)),
        (404, "not_found".into()),
        "{}",
        refused.body
    );
    set("reactivate");
    let renamed = rename("Reactivated", 1);
    assert_eq!(renamed.status, 200, "{}", renamed.body);
}

fn versions_are_saved_applied_and_rolled_back_along_the_base_as(case: Case) {
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let (v1, base) = case.at("alice", &ya);
    assert_eq!(base, 1);

    let text = case.hosted(&[("Y", Some("y.example")), ("A", None)]);
    let saved = case.save("alice", &ya, &text, 1);
    assert_eq!(saved.status, 201, "{}", saved.body);
    let v2 = saved.json()["version"].as_str().unwrap().to_string();
    assert_ne!(v1, v2);
    assert_eq!(saved.json()["base"], 1);
    assert_eq!(case.save("alice", &ya, &text, 1).status, 200);
    assert_eq!(case.at("alice", &ya), (v1.clone(), 1));
    assert_eq!(case.at("bob", &yb).1, 1);

    let listed = case.get(
        &format!("/api/v1/workspaces/{ya}/versions"),
        &case.token("alice"),
    );
    assert_eq!(listed.status, 200, "{}", listed.body);
    let versions = listed.json()["versions"].as_array().unwrap().clone();
    let ids: Vec<&str> = versions
        .iter()
        .map(|v| v["version"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&v1.as_str()) && ids.contains(&v2.as_str()));
    let current: Vec<&Value> = versions.iter().filter(|v| v["current"] == true).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["version"], json!(v1));
    assert_eq!(versions[0]["author"], json!(case.subject("alice")));
    let peer = case.get(
        &format!("/api/v1/workspaces/{ya}/versions"),
        &case.token("dave"),
    );
    assert_eq!(peer.status, 200, "{}", peer.body);
    assert!(
        peer.json()["versions"][0].get("author").is_none(),
        "{}",
        peer.body
    );

    let shown = case.get(
        &format!("/api/v1/workspaces/{ya}/versions/{v2}?against=current"),
        &case.token("alice"),
    );
    assert_eq!(shown.status, 200, "{}", shown.body);
    let shown = shown.json();
    assert_eq!(shown["diff"]["against"], json!(v1));
    assert_eq!(
        shown["diff"]["changed"][0]["repo"],
        json!(case.identity("Y"))
    );
    assert_eq!(shown["diff"]["changed"][0]["keys"], json!(["serve"]));
    assert!(shown["text"].as_str().unwrap().contains("y.example"));
    let other = case.show("bob", &yb).json()["version"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        other,
        case.show("bob", &yb).json()["version"].as_str().unwrap()
    );
    let apply = case.apply("alice", &ya, "0123456789abcdef", 1);
    assert_eq!(
        (apply.status, code(&apply)),
        (404, "unknown_version".into())
    );

    let applied = case.apply("alice", &ya, &v2, 1);
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["action"], "apply");
    assert_eq!(applied.json()["base"], 2);
    assert_eq!(case.at("alice", &ya), (v2.clone(), 2));
    let again = case.apply("alice", &ya, &v2, 2);
    assert_eq!(
        (again.status, code(&again)),
        (409, "already_current".into())
    );

    let back = case.apply("alice", &ya, &v1, 2);
    assert_eq!(back.status, 200, "{}", back.body);
    assert_eq!(
        (back.json()["action"].clone(), back.json()["base"].clone()),
        (json!("rollback"), json!(3))
    );
    assert_eq!(case.at("alice", &ya), (v1.clone(), 3));

    let stale = case.apply("alice", &ya, &v2, 2);
    assert_eq!((stale.status, code(&stale)), (409, "stale_base".into()));
    assert_eq!(stale.json()["error"]["base"], 3);
    assert_eq!(case.at("alice", &ya), (v1.clone(), 3));
    let stale = case.save("alice", &ya, &text, 1);
    assert_eq!((stale.status, code(&stale)), (409, "stale_base".into()));
    assert_eq!(stale.json()["error"]["base"], 3);

    let rename = |caller: &str, name: &str, base: i64| {
        let body = json!({"name": name, "base": base});
        let path = format!("/api/v1/workspaces/{ya}/rename");
        case.post(&path, &case.token(caller), &body)
    };
    let renamed = rename("alice", "Payments", 3);
    assert_eq!(renamed.status, 200, "{}", renamed.body);
    assert_eq!(renamed.json()["base"], 4);
    let shown = case.show("alice", &ya).json();
    assert_eq!(
        (shown["name"].clone(), shown["id"].clone()),
        (json!("Payments"), json!(ya))
    );
    assert_eq!(rename("alice", " bad", 4).status, 422);
    assert_eq!(rename("dave", "Mine", 4).status, 403);

    let history = case.get(
        &format!("/api/v1/workspaces/{ya}/history"),
        &case.token("alice"),
    );
    assert_eq!(history.status, 200, "{}", history.body);
    let actions: Vec<String> = (history.json()["history"].as_array().unwrap().iter())
        .map(|h| h["action"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(actions, ["rename", "rollback", "apply", "create"]);
    let actors: Vec<String> = (history.json()["history"].as_array().unwrap().iter())
        .map(|h| h["actor"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(actors, vec![case.subject("alice"); 4]);
    let denied = case.get(
        &format!("/api/v1/workspaces/{ya}/history"),
        &case.token("dave"),
    );
    assert_eq!(denied.status, 403);

    let operators = |caller: &str, body: Value| {
        let path = format!("/api/v1/workspaces/{ya}/operators");
        case.post(&path, &case.token(caller), &body)
    };
    let add = json!({"add": [{"subject": case.subject("dave"), "login": "dave"}], "base": 4});
    let denied = operators("alice", add.clone());
    assert_eq!(
        (denied.status, code(&denied)),
        (403, "not_administrator".into())
    );
    let delegated = operators("root", add);
    assert_eq!(delegated.status, 200, "{}", delegated.body);
    assert_eq!(delegated.json()["base"], 5);
    let shown = case.show("dave", &ya).json();
    assert_eq!(shown["operate"], true);
    let mut listed: Vec<Value> = shown["operators"].as_array().unwrap().clone();
    listed.sort_by_key(|o| o["subject"].as_str().unwrap().to_string());
    assert_eq!(
        listed,
        [
            json!({"subject": case.subject("alice"), "login": "alice"}),
            json!({"subject": case.subject("dave"), "login": "dave"}),
        ]
    );
    let both = [case.subject("alice"), case.subject("dave")];
    let last = operators("root", json!({"remove": both, "base": 5}));
    assert_eq!((last.status, code(&last)), (422, "last_operator".into()));
    let dropped = operators(
        "root",
        json!({"remove": [case.subject("alice")], "base": 5}),
    );
    assert_eq!(dropped.status, 200, "{}", dropped.body);
    let refused = case.apply("alice", &ya, &v2, 6);
    assert_eq!(
        (refused.status, code(&refused)),
        (403, "not_operator".into())
    );
    let delegate = case.apply("dave", &ya, &v2, 6);
    assert_eq!(delegate.status, 200, "{}", delegate.body);
    assert_eq!(delegate.json()["action"], "rollback");

    let unknown = case.get("/api/v1/workspaces/0123456789abcdef", &case.token("root"));
    let deleted = case.post(
        &format!("/api/v1/workspaces/{ya}/delete"),
        &case.token("dave"),
        &json!({"base": 7}),
    );
    assert_eq!(deleted.status, 200, "{}", deleted.body);
    for caller in ["dave", "alice", "root"] {
        let gone = case.show(caller, &ya);
        assert_eq!((gone.status, &gone.body), (404, &unknown.body), "{caller}");
    }
    assert!(!case.listed("root").contains(&ya));
    assert_eq!(case.at("bob", &yb).1, 1);
    let again = case.create("alice", "Y and A", &["Y", "A"]);
    assert_eq!(
        (again.status, code(&again)),
        (409, "workspace_deleted".into())
    );

    let records = case.audited(&ya);
    let line = |r: &Value| {
        format!(
            "{} {} {}",
            r["action"].as_str().unwrap(),
            r["subject"].as_str().unwrap_or("-"),
            r["outcome"].as_str().unwrap()
        )
    };
    let lines: Vec<String> = records.iter().map(line).collect();
    let count = |needle: &str| lines.iter().filter(|l| l.as_str() == needle).count();
    assert_eq!(
        count(&format!(
            "workspaces.create {} accepted",
            case.subject("alice")
        )),
        1,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.save {} accepted",
            case.subject("alice")
        )),
        2,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.apply {} accepted",
            case.subject("alice")
        )),
        1,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.rollback {} accepted",
            case.subject("alice")
        )),
        1,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.rollback {} accepted",
            case.subject("dave")
        )),
        1,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.rename {} accepted",
            case.subject("alice")
        )),
        1,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.rename {} refused",
            case.subject("dave")
        )),
        1,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.operators {} accepted",
            case.subject("root")
        )),
        2,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.operators {} refused",
            case.subject("root")
        )),
        1,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.operators {} refused",
            case.subject("alice")
        )),
        1,
        "{lines:?}"
    );
    assert_eq!(
        count(&format!(
            "workspaces.delete {} accepted",
            case.subject("dave")
        )),
        1,
        "{lines:?}"
    );
    assert!(records.iter().all(|r| r["via"] == "console"));
    let accepted = records
        .iter()
        .find(|r| r["action"] == "workspaces.delete")
        .unwrap();
    assert_eq!(accepted["arguments"]["authority"], "workspace_operator");
    assert_eq!(accepted["arguments"]["base"], "7");
    case.github.clean(&case.console.stderr(), &[]);
}

#[test]
fn concurrent_edits_of_one_base_let_exactly_one_win() {
    let Some(case) = Case::new("ws_race") else {
        return;
    };
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let (_, base) = case.at("alice", &ya);
    let mut versions = Vec::new();
    for host in ["one.example", "two.example", "three.example"] {
        let text = case.hosted(&[("Y", Some(host)), ("A", None)]);
        let saved = case.save("alice", &ya, &text, base);
        assert_eq!(saved.status, 201, "{}", saved.body);
        versions.push(saved.json()["version"].as_str().unwrap().to_string());
    }
    for _ in 0..4 {
        let (current, base) = case.at("alice", &ya);
        let others: Vec<&String> = versions.iter().filter(|v| **v != current).collect();
        let replies: Vec<Reply> = std::thread::scope(|scope| {
            let a = scope.spawn(|| case.apply("alice", &ya, others[0], base));
            let b = scope.spawn(|| case.apply("root", &ya, others[1], base));
            vec![a.join().unwrap(), b.join().unwrap()]
        });
        let bodies: Vec<&str> = replies.iter().map(|r| r.body.as_str()).collect();
        let won: Vec<&Reply> = replies.iter().filter(|r| r.status == 200).collect();
        let lost: Vec<&Reply> = replies.iter().filter(|r| r.status == 409).collect();
        assert_eq!((won.len(), lost.len()), (1, 1), "{bodies:?}");
        assert_eq!(code(lost[0]), "stale_base", "{bodies:?}");
        assert_eq!(lost[0].json()["error"]["base"], base + 1);
        let (now, seq) = case.at("alice", &ya);
        assert_eq!(seq, base + 1);
        assert_eq!(now, won[0].json()["version"].as_str().unwrap());
    }
    let history = case.get(
        &format!("/api/v1/workspaces/{ya}/history"),
        &case.token("root"),
    );
    assert_eq!(history.json()["history"].as_array().unwrap().len(), 5);
    case.github.clean(&case.console.stderr(), &[]);
}

#[test]
fn a_partial_revocation_hides_for_peers_and_never_drops_entries_for_the_operator() {
    let Some(case) = Case::new("ws_loss") else {
        return;
    };
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let v1 = case.at("alice", &ya).0;
    let unknown = case.get("/api/v1/workspaces/0123456789abcdef", &case.token("root"));
    let y_hosted = case.hosted(&[("Y", Some("y.example")), ("A", None)]);
    let waiting = case.save("alice", &ya, &y_hosted, 1);
    assert_eq!(waiting.status, 201, "{}", waiting.body);
    let waiting = waiting.json()["version"].as_str().unwrap().to_string();

    case.revoke(&[("alice", "A")]);
    let shown = case.show("alice", &ya).json();
    assert_eq!(shown["entries"].as_array().unwrap().len(), 1, "{shown}");
    assert_eq!(
        (shown["operate"].clone(), shown["hidden"].clone()),
        (json!(true), json!(1))
    );
    assert_eq!(
        case.show("erin", &ya).json()["entries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(case.listed("alice"), vec![ya.clone()]);
    let version = case.get(
        &format!("/api/v1/workspaces/{ya}/versions/{v1}"),
        &case.token("alice"),
    );
    assert_eq!(version.status, 200, "{}", version.body);
    let version = version.json();
    assert_eq!(
        (
            version["entries"].as_array().unwrap().len(),
            version["hidden"].clone()
        ),
        (1, json!(1))
    );
    assert!(
        !version["text"]
            .as_str()
            .unwrap()
            .contains(&case.identity("A"))
    );
    let diff = case.get(
        &format!("/api/v1/workspaces/{ya}/versions/{waiting}?against={v1}"),
        &case.token("alice"),
    );
    assert!(!diff.body.contains(&case.identity("A")), "{}", diff.body);
    assert_eq!(
        diff.json()["diff"]["changed"][0]["repo"],
        json!(case.identity("Y"))
    );

    let only_y = case.hosted(&[("Y", Some("mine.example"))]);
    let saved = case.save("alice", &ya, &only_y, 1);
    assert_eq!(saved.status, 201, "{}", saved.body);
    let stored = saved.json()["version"].as_str().unwrap().to_string();
    let whole = case.get(
        &format!("/api/v1/workspaces/{ya}/versions/{stored}"),
        &case.token("root"),
    );
    let entries = whole.json()["entries"].as_array().unwrap().clone();
    let repos: Vec<&str> = entries
        .iter()
        .map(|e| e["repo"].as_str().unwrap())
        .collect();
    assert_eq!(repos, [case.identity("Y"), case.identity("A")]);
    assert_eq!(entries[1]["blocks"], json!({}));
    let applied = case.apply("alice", &ya, &stored, 1);
    assert_eq!(applied.status, 200, "{}", applied.body);
    let shown = case.show("alice", &ya).json();
    assert_eq!(
        (
            shown["entries"].as_array().unwrap().len(),
            shown["hidden"].clone()
        ),
        (1, json!(1))
    );
    let before = case.at("alice", &ya);
    let with_b = case.hosted(&[("Y", Some("mine.example")), ("B", None)]);
    let refused = case.save("alice", &ya, &with_b, before.1);
    assert_eq!(
        (refused.status, code(&refused)),
        (403, "entries_refused".into())
    );
    assert_eq!(refused.json()["error"]["hidden"], 1);
    assert_eq!(refused.json()["error"]["refused"], json!([]));
    let mut forged = with_b.clone();
    forged.push_str(&format!(
        "\n[[repository]]\nrepo = \"{}\"\n",
        case.identity("A")
    ));
    assert_eq!(case.save("alice", &ya, &forged, before.1).status, 403);
    assert_eq!(case.at("alice", &ya), before);

    case.revoke(&[("alice", "A"), ("alice", "Y")]);
    let refused = case.apply("alice", &ya, &waiting, before.1);
    assert_eq!(
        (refused.status, code(&refused)),
        (403, "entries_refused".into()),
        "{}",
        refused.body
    );
    assert_eq!(case.at("alice", &ya), before);
    let shown = case.show("alice", &ya).json();
    assert_eq!(shown["entries"], json!([]));
    assert_eq!(shown["hidden"], 2);
    let admin = case.save(
        "root",
        &ya,
        &case.hosted(&[("Y", Some("admin.example"))]),
        before.1,
    );
    assert_eq!(admin.status, 201, "{}", admin.body);

    let ab = id_of(&case.create("carol", "Y and A again", &["Y", "A"]));
    assert_eq!(case.show("erin", &ab).status, 200);
    case.revoke(&[("alice", "A"), ("alice", "Y"), ("erin", "A")]);
    for path in [
        format!("/api/v1/workspaces/{ab}"),
        format!("/api/v1/workspaces/{ab}/versions"),
        format!("/api/v1/workspaces/{ab}/history"),
    ] {
        let hidden = case.get(&path, &case.token("erin"));
        assert_eq!(
            (hidden.status, &hidden.body),
            (404, &unknown.body),
            "{path}"
        );
    }
    let write = case.save("erin", &ab, &case.hosted(&[("Y", None)]), 1);
    assert_eq!((write.status, &write.body), (404, &unknown.body));
    assert!(!case.listed("erin").contains(&ab));
    case.github.clean(&case.console.stderr(), &[]);
}

#[test]
fn overlapping_workspaces_live_and_end_independently_over_one_index() {
    let Some(case) = Case::new("ws_overlap") else {
        return;
    };
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let (y, a, b) = (case.identity("Y"), case.identity("A"), case.identity("B"));

    let edit = case.hosted(&[("Y", Some("a.example")), ("A", None)]);
    let version = case.save("alice", &ya, &edit, 1).json()["version"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(case.apply("alice", &ya, &version, 1).status, 200);
    let unknown = case.get("/api/v1/workspaces/0123456789abcdef", &case.token("root"));
    let hidden = case.apply("bob", &ya, &version, 2);
    assert_eq!((hidden.status, &hidden.body), (404, &unknown.body));
    assert_eq!(case.at("bob", &yb).1, 1);
    assert_eq!(case.show("carol", &ya).json()["operate"], false);
    let write = case.apply("carol", &yb, &version, 1);
    assert_eq!((write.status, code(&write)), (403, "not_operator".into()));
    let with_a = case.hosted(&[("Y", None), ("B", None), ("A", None)]);
    let refused = case.save("bob", &yb, &with_a, 1);
    assert_eq!(
        (refused.status, code(&refused)),
        (403, "entries_refused".into())
    );
    assert_eq!(case.at("bob", &yb).1, 1);

    case.job_ok(&["workspace", "pull", "--workspace", MANIFEST]);
    case.roles();
    let shown = case.job_ok(&["workspace", "show", "--pulled", "--workspace", MANIFEST]);
    for identity in [&y, &a, &b] {
        assert_eq!(
            shown.matches(&format!("repo = \"{identity}\"\n")).count(),
            1,
            "{shown}"
        );
    }
    case.job_ok(&["index", "--workspace", MANIFEST]);
    let generation = case.job_ok(&["snapshot", "publish", "--workspace", MANIFEST]);
    let generation = generation.trim().to_string();
    let read = |generation: &str| -> Value {
        let (stdout, stderr, code) = pull(&case.dir, MANIFEST, &[]);
        assert_eq!(code, Some(0), "{stdout} {stderr}");
        serde_json::from_slice(
            &fs::read(
                case.dir
                    .join("console-generations")
                    .join(generation)
                    .join("generation.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let document = read(&generation);
    let members: Vec<&str> = (document["members"].as_array().unwrap().iter())
        .map(|m| m["identity"].as_str().unwrap())
        .collect();
    assert_eq!(members, [a.as_str(), b.as_str(), y.as_str()]);
    let context = (document["contexts"].as_array().unwrap().iter())
        .find(|c| c["id"] == json!(ya))
        .unwrap();
    assert_eq!(context["version"], json!(version));
    assert_eq!(case.show("alice", &ya).json()["state"], "live");

    let removed = case.post(
        &format!("/api/v1/workspaces/{yb}/delete"),
        &case.token("bob"),
        &json!({"base": 1}),
    );
    assert_eq!(removed.status, 200, "{}", removed.body);
    case.job_ok(&["workspace", "pull", "--workspace", MANIFEST]);
    case.roles();
    let shown = case.job_ok(&["workspace", "show", "--pulled", "--workspace", MANIFEST]);
    assert_eq!(
        shown.matches(&format!("repo = \"{y}\"\n")).count(),
        1,
        "{shown}"
    );
    assert_eq!(
        shown.matches(&format!("repo = \"{a}\"\n")).count(),
        1,
        "{shown}"
    );
    assert!(!shown.contains(&b), "{shown}");
    case.job_ok(&["index", "--workspace", MANIFEST]);
    let next = case.job_ok(&["snapshot", "publish", "--workspace", MANIFEST]);
    let document = read(next.trim());
    let members: Vec<&str> = (document["members"].as_array().unwrap().iter())
        .map(|m| m["identity"].as_str().unwrap())
        .collect();
    assert_eq!(members, [a.as_str(), y.as_str()]);
    let ids: Vec<&str> = (document["contexts"].as_array().unwrap().iter())
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [ya.as_str()]);
    let export = |caller: &str, repo: &str| {
        let path = format!(
            "/api/v1/workspaces/{ya}/repositories/{}/export",
            repo.replace('/', "%2F")
        );
        case.get(&path, &case.token(caller))
    };
    assert_eq!(export("erin", &y).status, 200);
    assert_eq!(export("alice", &a).status, 200);
    case.revoke(&[("alice", "A")]);
    let denied = export("alice", &a);
    assert_ne!(denied.status, 200, "{}", denied.body);
    assert_eq!(export("alice", &y).status, 200);
    let denied = export("bob", &y);
    assert_eq!(denied.status, 404, "{}", denied.body);

    case.job_ok(&["index", "--workspace", MANIFEST]);
    let last = case.job_ok(&["snapshot", "publish", "--workspace", MANIFEST]);
    let document = read(last.trim());
    let context = (document["contexts"].as_array().unwrap().iter())
        .find(|c| c["id"] == json!(ya))
        .unwrap();
    let recorded: Vec<&str> = (context["repositories"].as_array().unwrap().iter())
        .map(|r| r["repo"].as_str().unwrap())
        .collect();
    assert_eq!(recorded, [y.as_str()]);
    assert_eq!(context["excluded"], json!([a]));
    let (node, pulled) = case.node();
    assert_eq!(pulled, last.trim());
    let route = |id: &str| format!("/-/workspaces/{id}/{y}");
    assert_eq!(case.show("erin", &ya).status, 200);
    until_status(&node, &route(&ya), &case.node_token("erin"), 200);
    let hidden = case.show("yoda", &ya);
    assert_eq!((hidden.status, &hidden.body), (404, &unknown.body));
    let yoda = case.node_token("yoda");
    let (status, listing) = node_call(&node, &route(&ya), &yoda, "handle");
    assert_eq!(status, 404, "{listing}");
    assert!(!listing.contains(&ya), "{listing}");
    let (_, other) = node_call(&node, &route("0123456789abcdef"), &yoda, "handle");
    assert_eq!(listing, other);
    let (status, _) = node_call(&node, &format!("{}@{pulled}", route(&ya)), &yoda, "handle");
    assert_eq!(status, 404);
    case.github.clean(&case.console.stderr(), &[]);
}

#[cfg(feature = "directory")]
#[test]
fn a_deleted_user_is_blocked_from_every_lifecycle_route_until_provisioned_active() {
    use super::super::directory::{USER, expect};
    let Some(case) = Case::start("ws_scim", true) else {
        return;
    };
    case.refresh();
    case.ready();
    let scim_user = |name: &str, active: bool| -> String {
        let body = json!({"schemas": [USER], "userName": name, "active": active});
        let reply = expect(&case.console, "POST", "/Users", Some(&body), 201);
        reply["id"].as_str().unwrap().to_string()
    };
    let alice = scim_user("alice", true);
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let text = case.hosted(&[("Y", Some("y.example")), ("A", None)]);
    let ok = case.save("alice", &ya, &text, 1);
    assert_eq!(ok.status, 201, "{}", ok.body);
    let version = ok.json()["version"].as_str().unwrap().to_string();

    expect(
        &case.console,
        "DELETE",
        &format!("/Users/{alice}"),
        None,
        204,
    );
    let blocked = |reply: Reply| {
        assert_eq!(
            (reply.status, code(&reply)),
            (403, "deprovisioned".into()),
            "{}",
            reply.body
        );
    };
    let body = |extra: Value| {
        let mut body = json!({"base": 1});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };
    let token = case.token("alice");
    let path = |op: &str| format!("/api/v1/workspaces/{ya}/{op}");
    blocked(case.post(
        &path("versions"),
        &token,
        &body(json!({"entries": text, "message": "m"})),
    ));
    blocked(case.post(&path("apply"), &token, &body(json!({"version": version}))));
    blocked(case.post(&path("rename"), &token, &body(json!({"name": "Blocked"}))));
    blocked(case.post(&path("delete"), &token, &body(json!({}))));
    blocked(case.get(&format!("/api/v1/workspaces/{ya}"), &token));
    assert_eq!(case.at("root", &ya), (case.at("carol", &ya).0, 1));
    assert_eq!(case.show("carol", &ya).status, 200);
    let again = scim_user("alice", false);
    blocked(case.get(&format!("/api/v1/workspaces/{ya}"), &token));
    let put = json!({"schemas": [USER], "userName": "alice", "active": true});
    expect(
        &case.console,
        "PUT",
        &format!("/Users/{again}"),
        Some(&put),
        200,
    );
    let applied = case.apply("alice", &ya, &version, 1);
    assert_eq!(applied.status, 200, "{}", applied.body);
    case.github.clean(&case.console.stderr(), &[]);
}

#[cfg(feature = "directory")]
#[test]
fn a_deprovisioned_user_is_blocked_from_operators_and_the_catalog_until_provisioned_active() {
    use super::super::directory::{USER, expect, patch};
    let Some(case) = Case::start("ws_scim_catalog", true) else {
        return;
    };
    case.refresh();
    case.ready();
    let scim_user = |active: bool| -> String {
        let body = json!({"schemas": [USER], "userName": "root", "active": active});
        let reply = expect(&case.console, "POST", "/Users", Some(&body), 201);
        reply["id"].as_str().unwrap().to_string()
    };
    let ya = id_of(&case.create("root", "Y and A", &["Y", "A"]));
    let text = case.hosted(&[("Y", Some("y.example")), ("A", None)]);
    let version = case.save("root", &ya, &text, 1).json()["version"]
        .as_str()
        .unwrap()
        .to_string();
    let token = case.token("root");
    let path = |op: &str| format!("/api/v1/workspaces/{ya}/{op}");
    let dave = json!({"subject": case.subject("dave"), "login": "dave"});
    let routes = [
        ("GET", "/api/v1/catalog".to_string(), json!(null)),
        (
            "POST",
            "/api/v1/catalog/check".to_string(),
            json!({"identities": [case.identity("Y")]}),
        ),
        ("GET", format!("/api/v1/workspaces/{ya}"), json!(null)),
        (
            "POST",
            path("versions"),
            json!({"entries": text, "message": "m", "base": 1}),
        ),
        (
            "POST",
            path("apply"),
            json!({"version": version, "base": 1}),
        ),
        (
            "POST",
            path("rename"),
            json!({"name": "Blocked", "base": 1}),
        ),
        ("POST", path("operators"), json!({"add": [dave], "base": 1})),
        ("POST", path("delete"), json!({"base": 1})),
    ];
    let call = |(method, path, body): &(&str, String, Value)| match *method {
        "GET" => case.get(path, &token),
        _ => case.post(path, &token, body),
    };
    let admin = crate::postgres::Admin::new(&case.github.url);
    let schema = &case.github.schema;
    let rows = || -> Vec<String> {
        let tables = ["workspaces", "workspace_operators", "workspace_versions"];
        let tables = tables.iter().chain(&["workspace_history"]);
        let select = |table: &&str| format!("SELECT t::text FROM {schema}.{table} t ORDER BY 1");
        tables
            .flat_map(|table| admin.texts(&select(table)))
            .collect()
    };
    let held = rows();
    let (catalog, check, operators) = (&routes[0], &routes[1], &routes[6]);
    let blocked = || {
        for route in &routes {
            let reply = call(route);
            assert_eq!(
                (reply.status, code(&reply)),
                (403, "deprovisioned".into()),
                "{} {}: {}",
                route.0,
                route.1,
                reply.body
            );
        }
        assert_eq!(rows(), held, "a refused route wrote");
        let never = case.get("/api/v1/catalog", &case.token("carol"));
        assert_eq!(
            never.status, 200,
            "a user never provisioned is admitted: {}",
            never.body
        );
    };

    let id = scim_user(true);
    assert_eq!(call(catalog).status, 200);
    let off = patch(json!([{"op": "Replace", "path": "active", "value": false}]));
    expect(
        &case.console,
        "PATCH",
        &format!("/Users/{id}"),
        Some(&off),
        200,
    );
    blocked();
    let on = patch(json!([{"op": "Replace", "path": "active", "value": true}]));
    expect(
        &case.console,
        "PATCH",
        &format!("/Users/{id}"),
        Some(&on),
        200,
    );
    assert_eq!(call(catalog).status, 200);
    assert_eq!(call(check).status, 200);

    expect(&case.console, "DELETE", &format!("/Users/{id}"), None, 204);
    blocked();
    let again = scim_user(false);
    blocked();
    let put = json!({"schemas": [USER], "userName": "root", "active": true});
    expect(
        &case.console,
        "PUT",
        &format!("/Users/{again}"),
        Some(&put),
        200,
    );
    assert_eq!(call(catalog).status, 200);
    let delegated = call(operators);
    assert_eq!(delegated.status, 200, "{}", delegated.body);
    case.github.clean(&case.console.stderr(), &[]);
}

#[test]
fn without_the_audit_no_named_workspace_write_runs_and_reads_still_answer() {
    let Some(case) = Case::new("ws_audit_down") else {
        return;
    };
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("root", "Y and A", &["Y", "A"]));
    let first = case.at("root", &ya).0;
    let hosted = |host: &str| case.hosted(&[("Y", Some(host)), ("A", None)]);
    let second = case.save("root", &ya, &hosted("two.example"), 1).json()["version"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(case.apply("root", &ya, &second, 1).status, 200);
    let third = case.save("root", &ya, &hosted("three.example"), 2).json()["version"]
        .as_str()
        .unwrap()
        .to_string();
    let admin = crate::postgres::Admin::new(&case.github.url);
    let schema = &case.github.schema;
    let s3 = bucket(&case.endpoint, &bucket_name(&case.name));
    let state = || {
        let rows: Vec<String> = [
            "workspaces",
            "workspace_operators",
            "workspace_versions",
            "workspace_history",
        ]
        .iter()
        .flat_map(|table| {
            admin.texts(&format!(
                "SELECT t::text FROM {schema}.{table} t ORDER BY 1"
            ))
        })
        .collect();
        let token = case.token("root");
        let reads = [
            "/api/v1/workspaces".to_string(),
            format!("/api/v1/workspaces/{ya}"),
            format!("/api/v1/workspaces/{ya}/versions"),
            format!("/api/v1/workspaces/{ya}/history"),
        ]
        .map(|path| {
            let reply = case.get(&path, &token);
            assert_eq!(reply.status, 200, "{path}: {}", reply.body);
            reply.json()
        });
        let keys: Vec<String> = crate::snapshot::objects(&s3, "")
            .iter()
            .map(|o| o.location.to_string())
            .collect();
        (rows, reads, keys)
    };
    let before = state();

    admin.run(&format!("ALTER TABLE {schema}.audit RENAME TO audit_gone"));
    let fresh = hosted("four.example");
    let path = |op: &str| format!("/api/v1/workspaces/{ya}/{op}");
    let writes = [
        (
            "/api/v1/workspaces".to_string(),
            json!({"name": "Other", "description": "", "entries": case.entries(&["Y"]),
                   "message": "first"}),
        ),
        (
            path("versions"),
            json!({"entries": fresh, "message": "m", "base": 2}),
        ),
        (path("apply"), json!({"version": third, "base": 2})),
        (path("apply"), json!({"version": first, "base": 2})),
        (path("rename"), json!({"name": "Renamed", "base": 2})),
        (
            path("operators"),
            json!({"add": [{"subject": case.subject("dave"), "login": "dave"}], "base": 2}),
        ),
        (path("delete"), json!({"base": 2})),
    ];
    let token = case.token("root");
    for (route, body) in &writes {
        let reply = case.post(route, &token, body);
        assert_eq!(
            (reply.status, code(&reply)),
            (503, "audit_unavailable".into()),
            "{route} {body}: {}",
            reply.body
        );
    }
    assert_eq!(state(), before);
    assert_eq!(case.get("/api/v1/session", &token).status, 200);
    let page = case.get("/api/v1/audit", &token);
    assert_eq!(
        (page.status, code(&page)),
        (503, "audit_unavailable".into())
    );

    admin.run(&format!("ALTER TABLE {schema}.audit_gone RENAME TO audit"));
    let renamed = case.post(
        &path("rename"),
        &token,
        &json!({"name": "Renamed", "base": 2}),
    );
    assert_eq!(renamed.status, 200, "{}", renamed.body);
    case.github.clean(&case.console.stderr(), &[]);
}

#[test]
fn the_cli_twins_act_as_the_administrator_through_the_same_boundary() {
    let Some(case) = Case::new("ws_cli") else {
        return;
    };
    case.refresh();
    case.ready();
    let file = |name: &str, text: &str| {
        let path = case.dir.join(name);
        fs::write(&path, text).unwrap();
        path.display().to_string()
    };
    let ya = file("ya.toml", &case.hosted(&[("Y", None), ("A", None)]));
    let hosted = file(
        "hosted.toml",
        &case.hosted(&[("Y", Some("cli.example")), ("A", None)]),
    );
    let run = |args: &[&str]| -> (String, String, i32) {
        let mut full: Vec<&str> = vec!["workspaces"];
        full.extend_from_slice(args);
        full.extend_from_slice(&["--workspace", MANIFEST]);
        case.job(&full)
    };
    let ok = |args: &[&str]| -> String {
        let (stdout, stderr, code) = run(args);
        assert_eq!(code, 0, "{args:?}: {stderr}");
        stdout
    };

    let id = ok(&[
        "create",
        &ya,
        "--name",
        "From the CLI",
        "--message",
        "first",
    ])
    .trim()
    .to_string();
    assert!(meada_core::ids::is_id(&id), "{id}");
    assert!(ok(&["list"]).contains(&format!("{id}\t")));
    let shown = ok(&["show", &id]);
    assert!(
        shown.contains("# base: 1\n") && shown.contains("From the CLI"),
        "{shown}"
    );
    assert!(
        shown.contains(&format!("repo = \"{}\"", case.identity("Y"))),
        "{shown}"
    );
    assert_eq!(case.show("root", &id).status, 200);
    assert_eq!(case.show("alice", &id).json()["operate"], false);

    let v2 = ok(&["save", &id, &hosted, "--message", "hosts", "--base", "1"])
        .trim()
        .to_string();
    let listed = ok(&["versions", &id]);
    assert_eq!(listed.lines().count(), 2, "{listed}");
    assert!(
        listed.contains(&format!("{v2}\t")) && listed.contains("\tcurrent\t"),
        "{listed}"
    );
    let diff = ok(&["diff", &id, &v2]);
    assert_eq!(diff.trim(), format!("changed {} serve", case.identity("Y")));
    let (_, _, code) = run(&["diff", &id, "0123456789abcdef"]);
    assert_eq!(code, 1);
    let (_, stderr, code) = run(&["save", &id, &hosted, "--message", "x", "--base", "9"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("base 1"), "{stderr}");
    let applied = ok(&["apply", &id, &v2, "--base", "1"]);
    assert_eq!(applied.trim(), format!("apply {v2} base 2"));
    assert_eq!(case.at("root", &id), (v2.clone(), 2));
    let v1 = case.get(
        &format!("/api/v1/workspaces/{id}/versions"),
        &case.token("root"),
    );
    let v1 = (v1.json()["versions"].as_array().unwrap().iter())
        .map(|v| v["version"].as_str().unwrap().to_string())
        .find(|v| *v != v2)
        .unwrap();
    let back = ok(&["apply", &id, &v1, "--base", "2"]);
    assert_eq!(back.trim(), format!("rollback {v1} base 3"));
    let (_, stderr, code) = run(&["apply", &id, &v2, "--base", "2"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("base 3"), "{stderr}");

    assert_eq!(
        ok(&["rename", &id, "Renamed", "--base", "3"]).trim(),
        "base 4"
    );
    assert_eq!(case.show("root", &id).json()["name"], "Renamed");
    ok(&["operators", &id, "--add", "alice=alice", "--base", "4"]);
    assert_eq!(case.show("alice", &id).json()["operate"], true);
    let (_, _, code) = run(&["operators", &id, "--add", "no equals sign", "--base", "5"]);
    assert_eq!(code, 1);
    assert_eq!(ok(&["delete", &id, "--base", "5"]).trim(), "deleted base 6");
    assert_eq!(case.show("root", &id).status, 404);
    assert!(!ok(&["list"]).contains(&id));

    let records = case.audited(&id);
    let actions: Vec<&str> = records
        .iter()
        .map(|r| r["action"].as_str().unwrap())
        .collect();
    let accepted: Vec<&str> = (records.iter())
        .filter(|r| r["outcome"] == "accepted")
        .map(|r| r["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        accepted,
        [
            "workspaces.create",
            "workspaces.save",
            "workspaces.apply",
            "workspaces.rollback",
            "workspaces.rename",
            "workspaces.operators",
            "workspaces.delete",
        ],
        "{actions:?}"
    );
    assert!(
        records
            .iter()
            .all(|r| r["via"] == "cli" && r["role"] == "operator")
    );
    assert!(
        records
            .iter()
            .all(|r| r["subject"].as_str().unwrap().starts_with("cli:"))
    );
    let refused = records.iter().filter(|r| r["outcome"] == "refused").count();
    assert_eq!(refused, 2, "{actions:?}");
    case.github.clean(&case.console.stderr(), &[]);
}

#[test]
fn the_bounds_of_the_lifecycle_routes_refuse_before_anything_changes() {
    let Some(case) = Case::new("ws_bounds") else {
        return;
    };
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let version = case.at("alice", &ya).0;
    let path = |op: &str| format!("/api/v1/workspaces/{ya}/{op}");
    let big = "x".repeat(2 << 10);
    let apply = case.post(
        &path("apply"),
        &case.token("alice"),
        &json!({"version": version, "base": 1, "x": big}),
    );
    assert_eq!(apply.status, 413, "{}", apply.body);
    let rename = case.post(
        &path("rename"),
        &case.token("alice"),
        &json!({"name": big.repeat(3), "base": 1}),
    );
    assert_eq!(rename.status, 413, "{}", rename.body);
    let extra = case.post(
        &path("delete"),
        &case.token("alice"),
        &json!({"base": 1, "actor": "root"}),
    );
    assert_eq!(extra.status, 400, "{}", extra.body);
    for query in ["after=zz", "after=1&after=2", "extra=1"] {
        let reply = case.get(
            &format!("{}?{query}", path("versions")),
            &case.token("alice"),
        );
        assert_eq!(reply.status, 400, "{query}: {}", reply.body);
    }
    let history = case.get(
        &format!("{}?after=0", path("history")),
        &case.token("alice"),
    );
    assert_eq!(history.status, 400);
    let operators = |body: Value| case.post(&path("operators"), &case.token("root"), &body);
    let long =
        operators(json!({"add": [{"subject": "s".repeat(257), "login": "dave"}], "base": 1}));
    assert_eq!(
        (long.status, code(&long)),
        (422, "invalid_operators".into())
    );
    let many: Vec<Value> = (0..20)
        .map(|i| json!({"subject": format!("s{i}"), "login": "dave"}))
        .collect();
    let past = operators(json!({"add": many, "base": 1}));
    assert_eq!(
        (past.status, code(&past)),
        (422, "too_many_operators".into()),
        "{}",
        past.body
    );
    let twenty: Vec<Value> = (0..19)
        .map(|i| json!({"subject": format!("s{i}"), "login": "dave"}))
        .collect();
    assert_eq!(operators(json!({"add": twenty, "base": 1})).status, 200);
    assert_eq!(case.at("root", &ya), (version, 2));
    case.github.clean(&case.console.stderr(), &[]);
}
