//! The version lifecycle of named workspaces (#343, ADR 0020 and 0021 contract 4), as public
//! processes: the console (`meada ui`) saves, applies, rolls back, renames, deletes and delegates
//! workspaces for the callers of the shared Y/A/B table (`tests/fixtures/named-workspaces.json`)
//! over the loopback issuer, the fake GitHub API and the local Valkey, Postgres and
//! S3-compatible store; the job pulls, checks sponsors, indexes and publishes. What a caller may
//! do is seen through statuses, answers and the audit's records, never the module's tables.

use super::*;

/// The id a creation answered.
fn id_of(reply: &Reply) -> String {
    assert!(matches!(reply.status, 200 | 201), "{}", reply.body);
    reply.json()["id"].as_str().unwrap().to_string()
}

impl Case {
    /// The manifest text selecting the table's `shorts`, each with the serve host it names.
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

    /// The `version` and `base` of the workspace as `caller` reads it.
    fn at(&self, caller: &str, workspace: &str) -> (String, i64) {
        let shown = self.show(caller, workspace);
        assert_eq!(shown.status, 200, "{}", shown.body);
        let shown = shown.json();
        (
            shown["version"].as_str().unwrap().to_string(),
            shown["base"].as_i64().unwrap(),
        )
    }

    /// Removes the role `caller` holds on `short` at the fake GitHub, runs the role check and
    /// waits until the console reads the new set. `revoked` is every (caller, repository) pair
    /// removed so far in the case, this one included.
    pub(super) fn revoke(&self, revoked: &[(&str, &str)]) {
        let (caller, short) = revoked.last().copied().unwrap();
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
        self.roles();
        // The console has read the set when it no longer lets the caller select the repository.
        let token = self.token(caller);
        let identity = self.identity(short);
        let start = Instant::now();
        loop {
            let reply = self.post(
                "/api/v1/catalog/check",
                &token,
                &json!({"identities": [identity]}),
            );
            if reply.json()["results"][0]["state"] == json!("unavailable") {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "{}", reply.body);
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The audit's records of the workspace `id`, oldest first.
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

/// Save, apply, rollback, rename and delegation move one workspace along its base; a stale
/// base, a version of another workspace and an apply of the current version change nothing; the
/// history and the versions are immutable; every operation is one audit record with its actor
/// and its workspace.
#[test]
fn versions_are_saved_applied_and_rolled_back_along_the_base() {
    let Some(case) = Case::new("ws_life") else {
        return;
    };
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let (v1, base) = case.at("alice", &ya);
    assert_eq!(base, 1);

    // Saving stores a version and moves nothing: the answer is 201 once, 200 the same text again.
    let text = case.hosted(&[("Y", Some("y.example")), ("A", None)]);
    let saved = case.save("alice", &ya, &text, 1);
    assert_eq!(saved.status, 201, "{}", saved.body);
    let v2 = saved.json()["version"].as_str().unwrap().to_string();
    assert_ne!(v1, v2);
    assert_eq!(saved.json()["base"], 1);
    assert_eq!(case.save("alice", &ya, &text, 1).status, 200);
    assert_eq!(case.at("alice", &ya), (v1.clone(), 1));
    // Bob's workspace shares Y and is untouched.
    assert_eq!(case.at("bob", &yb).1, 1);

    // The versions: newest first, the current one marked, the author named to operators only.
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
    assert_eq!(versions[0]["author"], "alice");
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

    // The diff of a stored version against the current one names what changed.
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
    // A version of another workspace is unknown here; so is an unknown id.
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

    // Apply moves the base; applying the current version again changes nothing.
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

    // Rolling back is an apply of a version that was current: marked so in the answer.
    let back = case.apply("alice", &ya, &v1, 2);
    assert_eq!(back.status, 200, "{}", back.body);
    assert_eq!(
        (back.json()["action"].clone(), back.json()["base"].clone()),
        (json!("rollback"), json!(3))
    );
    assert_eq!(case.at("alice", &ya), (v1.clone(), 3));

    // A stale page: the base it read is not the workspace's any more; nothing changes and the
    // refusal names the current base.
    let stale = case.apply("alice", &ya, &v2, 2);
    assert_eq!((stale.status, code(&stale)), (409, "stale_base".into()));
    assert_eq!(case.at("alice", &ya), (v1.clone(), 3));
    let stale = case.save("alice", &ya, &text, 1);
    assert_eq!((stale.status, code(&stale)), (409, "stale_base".into()));

    // Rename changes the name and keeps the description when none is sent; the id, the routes
    // and the versions stay.
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

    // The history is append only and in order; viewers do not read it.
    let history = case.get(
        &format!("/api/v1/workspaces/{ya}/history"),
        &case.token("alice"),
    );
    assert_eq!(history.status, 200, "{}", history.body);
    let actions: Vec<String> = (history.json()["history"].as_array().unwrap().iter())
        .map(|h| h["action"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(actions, ["rename", "rollback", "apply", "create"]);
    let denied = case.get(
        &format!("/api/v1/workspaces/{ya}/history"),
        &case.token("dave"),
    );
    assert_eq!(denied.status, 403);

    // Only the platform administrator delegates; the workspace keeps an operator.
    let operators = |caller: &str, body: Value| {
        let path = format!("/api/v1/workspaces/{ya}/operators");
        case.post(&path, &case.token(caller), &body)
    };
    let add = json!({"add": [{"subject": "dave", "login": "dave"}], "base": 4});
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
    assert_eq!(shown["operators"].as_array().unwrap().len(), 2);
    let last = operators("root", json!({"remove": ["alice", "dave"], "base": 5}));
    assert_eq!((last.status, code(&last)), (422, "last_operator".into()));
    let dropped = operators("root", json!({"remove": ["alice"], "base": 5}));
    assert_eq!(dropped.status, 200, "{}", dropped.body);
    // The creator, no longer an operator, is a viewer of its own workspace now; the delegate edits.
    let refused = case.apply("alice", &ya, &v2, 6);
    assert_eq!(
        (refused.status, code(&refused)),
        (403, "not_operator".into())
    );
    let delegate = case.apply("dave", &ya, &v2, 6);
    assert_eq!(delegate.status, 200, "{}", delegate.body);
    assert_eq!(delegate.json()["action"], "rollback");

    // Delete: gone for everyone, administrator included, with the one answer of an unknown id;
    // the other workspace sharing Y is untouched, and the same triple cannot be created again.
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

    // Every operation is one record with its actor and its workspace, refusals included.
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
    assert_eq!(count("workspaces.create alice accepted"), 1, "{lines:?}");
    assert_eq!(count("workspaces.save alice accepted"), 2, "{lines:?}");
    assert_eq!(count("workspaces.apply alice accepted"), 1, "{lines:?}");
    assert_eq!(count("workspaces.rollback alice accepted"), 1, "{lines:?}");
    assert_eq!(count("workspaces.rollback dave accepted"), 1, "{lines:?}");
    assert_eq!(count("workspaces.rename alice accepted"), 1, "{lines:?}");
    assert_eq!(count("workspaces.rename dave refused"), 1, "{lines:?}");
    assert_eq!(count("workspaces.operators root accepted"), 2, "{lines:?}");
    assert_eq!(count("workspaces.operators root refused"), 1, "{lines:?}");
    assert_eq!(count("workspaces.operators alice refused"), 1, "{lines:?}");
    assert_eq!(count("workspaces.delete dave accepted"), 1, "{lines:?}");
    assert!(records.iter().all(|r| r["via"] == "console"));
    let accepted = records
        .iter()
        .find(|r| r["action"] == "workspaces.delete")
        .unwrap();
    assert_eq!(accepted["arguments"]["authority"], "workspace_operator");
    assert_eq!(accepted["arguments"]["base"], "7");
    case.github.clean(&case.console.stderr(), &[]);
}

/// Two writers that read the same base: exactly one wins, the other gets `stale_base` naming the
/// base now, and no row of the loser is written.
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
        // Two versions that are not the current one, so that neither writer is refused for
        // anything but the race.
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
    // One history row per accepted change, none for the losers.
    let history = case.get(
        &format!("/api/v1/workspaces/{ya}/history"),
        &case.token("root"),
    );
    assert_eq!(history.json()["history"].as_array().unwrap().len(), 5);
    case.github.clean(&case.console.stderr(), &[]);
}

/// A creator that loses a repository keeps its workspace, reads the entries its grant reads and a
/// count of the rest, saves without dropping what it cannot read, and may add or change nothing it
/// is not eligible for; a revoked grant at the atomic boundary leaves everything as it was; a
/// group peer that loses a repository no longer sees the workspace, its versions or its history
/// at all.
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
    // A version saved while the creator still held A, and one that changes Y, waiting to apply.
    let y_hosted = case.hosted(&[("Y", Some("y.example")), ("A", None)]);
    let waiting = case.save("alice", &ya, &y_hosted, 1);
    assert_eq!(waiting.status, 201, "{}", waiting.body);
    let waiting = waiting.json()["version"].as_str().unwrap().to_string();

    // Alice loses A: she keeps the workspace and reads Y with a count of one hidden entry; her
    // peers erin and dave are unaffected.
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
    // Her version reads show the entries she reads and the count of the others, and the diff
    // between versions names nothing she cannot read.
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

    // She saves what she reads: the version stored keeps A in place, in order; the administrator
    // sees both entries and the one she could not read unchanged.
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
    // She cannot add B, which she has no role on, and the refusal counts it as hidden; naming A
    // again changes nothing she may not read. Nothing moved.
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

    // A page prepared before the loss cannot apply what the grant no longer covers: the version
    // waiting changes Y, and Y is lost too. Nothing changes.
    case.revoke(&[("alice", "A"), ("alice", "Y")]);
    let refused = case.apply("alice", &ya, &waiting, before.1);
    assert_eq!(
        (refused.status, code(&refused)),
        (403, "entries_refused".into()),
        "{}",
        refused.body
    );
    assert_eq!(case.at("alice", &ya), before);
    // Without a role on anything she still deletes or renames what she operates, and sees
    // the count of everything hidden.
    let shown = case.show("alice", &ya).json();
    assert_eq!(shown["entries"], json!([]));
    assert_eq!(shown["hidden"], 2);
    // The administrator, whose management view is unfiltered, removes what nobody can read by
    // saving the manifest whole.
    let admin = case.save(
        "root",
        &ya,
        &case.hosted(&[("Y", Some("admin.example"))]),
        before.1,
    );
    assert_eq!(admin.status, 201, "{}", admin.body);

    // A peer that loses A sees an unknown id on every read of the workspace.
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

/// Two workspaces share Y and are maintained independently; removing one keeps Y in the other
/// and in the fleet's membership, and the repository is still indexed once; a context that
/// loses a reader's grant answers that reader nothing.
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

    // Each group edits its own context; the other does not move.
    let edit = case.hosted(&[("Y", Some("a.example")), ("A", None)]);
    let version = case.save("alice", &ya, &edit, 1).json()["version"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(case.apply("alice", &ya, &version, 1).status, 200);
    // Neither operator edits the other's workspace; the answer for the hidden one is a 404.
    let unknown = case.get("/api/v1/workspaces/0123456789abcdef", &case.token("root"));
    let hidden = case.apply("bob", &ya, &version, 2);
    assert_eq!((hidden.status, &hidden.body), (404, &unknown.body));
    assert_eq!(case.at("bob", &yb).1, 1);
    // Carol reads both and operates neither.
    assert_eq!(case.show("carol", &ya).json()["operate"], false);
    let write = case.apply("carol", &yb, &version, 1);
    assert_eq!((write.status, code(&write)), (403, "not_operator".into()));
    // Bob selects A in his workspace? He has no role on it: refused, hidden counted.
    let with_a = case.hosted(&[("Y", None), ("B", None), ("A", None)]);
    let refused = case.save("bob", &yb, &with_a, 1);
    assert_eq!(
        (refused.status, code(&refused)),
        (403, "entries_refused".into())
    );
    assert_eq!(case.at("bob", &yb).1, 1);

    // The job: Y is a member once for both contexts, indexed once.
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
    // Alice's context records the version she applied: a context change is a new generation of
    // the same bundles.
    let context = (document["contexts"].as_array().unwrap().iter())
        .find(|c| c["id"] == json!(ya))
        .unwrap();
    assert_eq!(context["version"], json!(version));
    assert_eq!(case.show("alice", &ya).json()["state"], "live");

    // Bob removes his context: Y stays a member through alice's, B leaves the fleet, and Y is
    // still one bundle.
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
    // The context still answers the readers of both repositories through alice's workspace, and
    // alice, who loses A afterwards, is bounded by her grant in every graph answer.
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
    // The peers that never held the context cannot use the shared Y to reach it.
    let denied = export("bob", &y);
    assert_eq!(denied.status, 404, "{}", denied.body);
    case.github.clean(&case.console.stderr(), &[]);
}

/// A user the SCIM directory deleted stays blocked from every named-workspace write until it is
/// provisioned active again, and its operator rows remain; a user never provisioned keeps normal
/// admission. An audit that cannot record refuses the write.
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
    // Nothing moved, as the administrator reads it; the user never provisioned is admitted.
    assert_eq!(case.at("root", &ya), (case.at("carol", &ya).0, 1));
    assert_eq!(case.show("carol", &ya).status, 200);
    // Created again without `active`, the block stays; an explicit `active: true` lifts it.
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

/// `meada workspaces` does what the console does as the platform administrator, through the same
/// atomic boundary: a stale base is refused, every write is a `cli` audit record naming the
/// workspace, and the console sees the result.
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
    // The console reads what the CLI made, and the CLI has no operator row to show it to.
    assert_eq!(case.show("root", &id).status, 200);
    assert_eq!(case.show("alice", &id).json()["operate"], false);

    // A save moves nothing; a stale base is refused with the current one, and changes nothing.
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

    // Rename, delegate, delete.
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

    // Every write is one record of the CLI's, as the global operator, naming the workspace.
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

/// What the routes bound: a body past its cap, a cursor that is none, a delegation past 20
/// operators or naming a subject past the longest, and a version of a workspace that has none.
/// Each is refused and nothing moves.
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
    // A body past its cap is a 413 before it is read.
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
    // A key the route does not take is a 400, and so is a cursor that is none.
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
    // Delegation: a subject past the longest and a list past 20 operators change nothing.
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
