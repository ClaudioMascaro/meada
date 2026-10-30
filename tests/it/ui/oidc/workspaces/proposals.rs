//! The proposal server (ADR 0022, #346), as public processes: `meada workspaces mcp` answers the
//! callers of the shared Y/A/B table (`tests/fixtures/named-workspaces.json`) over MCP with the
//! console's own rules, beside a console over the same bootstrap, the loopback issuer, the fake
//! GitHub API and the local Valkey, Postgres and S3-compatible store. What a caller may do and
//! what a call leaves behind are seen through MCP answers, the console's reads, the audit page
//! and the fake GitHub's request count, never the module's tables.

use super::*;

/// The proposal server's URL and its tokens' audience.
const PROPOSALS: &str = "https://proposals.example";

/// The arguments no tool takes: anything naming authority is an undeclared property.
const FORGED: [&str; 8] = [
    "actor",
    "owner",
    "role",
    "grant",
    "login",
    "groups",
    "token",
    "permissions",
];

impl Case {
    /// `[proposals]` added to the bootstrap the console already read, and the proposal server
    /// over it.
    fn proposals(&self) -> Server {
        self.proposals_with(&[])
    }

    /// `proposals` with `env` besides the case's.
    fn proposals_with(&self, env: &[(&str, &str)]) -> Server {
        let file = self.dir.join(MANIFEST);
        let mut text = fs::read_to_string(&file).unwrap();
        if !text.contains("[proposals]") {
            text += &format!("\n[proposals]\npublic_url = \"{PROPOSALS}\"\n");
            fs::write(&file, text).unwrap();
        }
        let mut all = vec![
            ("MEADA_PERMISSIONS_POLL_MS", POLL_MS),
            ("MEADA_TEST_S3_KEY", KEY_ID),
        ];
        all.extend_from_slice(env);
        Server::start_command(
            &self.dir,
            "workspaces",
            &["mcp", "--listen", "127.0.0.1:0", "--workspace", MANIFEST],
            &all,
        )
    }

    /// An agent's token of the table's caller `name`, for the proposal server.
    fn agent(&self, name: &str) -> String {
        self.token_for(name, PROPOSALS)
    }

    /// What the console shows of every workspace and its versions and history, and the audit:
    /// the state a proposal must leave as it found it.
    fn state(&self, ids: &[&str]) -> Vec<String> {
        let root = self.token("root");
        let mut state = vec![self.get("/api/v1/audit?limit=100", &root).body];
        state.push(self.get("/api/v1/workspaces", &root).body);
        for id in ids {
            for tail in ["", "/versions", "/history"] {
                let reply = self.get(&format!("/api/v1/workspaces/{id}{tail}"), &root);
                assert_eq!(reply.status, 200, "{}", reply.body);
                state.push(reply.body);
            }
        }
        state
    }
}

/// A JSON-RPC request to the proposal server with `headers`.
fn rpc(server: &Server, headers: &[(&str, &str)], message: &Value) -> Reply {
    let mut all = vec![
        ("Content-Type", "application/json"),
        ("Accept", "application/json, text/event-stream"),
    ];
    all.extend_from_slice(headers);
    send_body(server, "POST", "/", &all, &message.to_string())
}

/// `tools/call` of `tool` with `arguments` by `token`: the JSON-RPC answer.
fn call(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let bearer = format!("Bearer {token}");
    let message = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    });
    let reply = rpc(server, &[("Authorization", &bearer)], &message);
    assert_eq!(reply.status, 200, "{}", reply.body);
    reply.json()
}

/// The structured answer of a call that succeeded.
fn answer(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let reply = call(server, token, tool, arguments);
    let result = &reply["result"];
    assert_eq!(result["isError"], false, "{reply}");
    result["structuredContent"].clone()
}

/// The error a tool answered (`isError`), `{code, message, ...}`.
fn refused(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let reply = call(server, token, tool, arguments);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    reply["result"]["structuredContent"]["error"].clone()
}

/// Whether a call was refused for its arguments: a tool error, from the tool's schema (an
/// undeclared or mistyped property) or from its bounds (`invalid_params`), so the agent corrects
/// it.
fn invalid(server: &Server, token: &str, tool: &str, arguments: Value) -> bool {
    let reply = call(server, token, tool, arguments);
    let result = &reply["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    result["isError"] == true
        && (result["structuredContent"]["error"]["code"] == "invalid_params"
            || text.starts_with("failed to deserialize parameters:"))
}

/// Waits until the proposal server has read a permissions set.
fn read_a_set(server: &Server, token: &str) {
    let start = Instant::now();
    loop {
        let catalog = answer(server, token, "catalog", json!({}));
        if catalog["reason"] != "no_set" {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "{catalog}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Admission is Meada's own and the audience is the proposal server's: no token, an invalid
/// one, a console's or a node's token is a 401 naming the metadata before any tool is reached,
/// a browser is a 403, and this server's token is refused by the console. The tools are four,
/// read-only, with closed input schemas that name no authority, and an argument asserting an
/// actor, a role or a grant is invalid, never trusted. The Y/A/B table through `propose` is the
/// console's: each refusal is the console's own error, a hidden repository is counted and never
/// named, a hint changes nothing and its note is never echoed or logged, a token without a login
/// is pending, and the inputs are bounded. No call reaches GitHub or leaves a token on stderr.
#[test]
fn proposals_follow_the_shared_table_and_trust_no_asserted_identity() {
    let Some(case) = Case::new("ws_proposals_table") else {
        return;
    };
    // Without `[proposals]` the server refuses to start, naming the section.
    let (_, stderr, code) = case.job(&[
        "workspaces",
        "mcp",
        "--listen",
        "127.0.0.1:0",
        "--workspace",
        MANIFEST,
    ]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("no [proposals] section"), "{stderr}");
    case.refresh();
    case.ready();
    let server = case.proposals();
    read_a_set(&server, &case.agent("root"));
    let requests = case.github.api.with(|s| s.requests);
    let (y, a, b) = (case.identity("Y"), case.identity("A"), case.identity("B"));

    // The metadata needs no token and names this server and the issuer.
    let metadata = send(&server, "GET", "/.well-known/oauth-protected-resource", &[]);
    assert_eq!(metadata.status, 200, "{}", metadata.body);
    assert_eq!(metadata.json()["resource"], PROPOSALS);
    assert_eq!(
        metadata.json()["authorization_servers"],
        json!([case.issuer.base])
    );
    // No token, a forged one, a console token and a node token: 401 before any tool.
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let forged = format!("{}x", case.agent("alice"));
    for (label, token) in [
        ("none", None),
        ("forged", Some(forged.clone())),
        ("console", Some(case.token("alice"))),
        ("node", Some(case.node_token("alice"))),
    ] {
        let bearer = token.map(|t| format!("Bearer {t}"));
        let headers: Vec<(&str, &str)> = bearer
            .iter()
            .map(|b| ("Authorization", b.as_str()))
            .collect();
        let reply = rpc(&server, &headers, &list);
        assert_eq!(reply.status, 401, "{label}: {}", reply.body);
        let challenge = reply.header("www-authenticate").unwrap_or_default();
        assert!(
            challenge.contains(&format!(
                "resource_metadata=\"{PROPOSALS}/.well-known/oauth-protected-resource\""
            )),
            "{label}: {challenge}"
        );
    }
    // A browser is refused first; this server's token opens nothing on the console.
    let bearer = format!("Bearer {}", case.agent("alice"));
    let browser = rpc(
        &server,
        &[("Authorization", &bearer), ("Origin", PUBLIC)],
        &list,
    );
    assert_eq!(browser.status, 403);
    assert_eq!(
        case.get("/api/v1/workspaces", &case.agent("alice")).status,
        401
    );
    let elsewhere = send(&server, "GET", "/other", &[("Authorization", &bearer)]);
    assert_eq!(elsewhere.status, 404);

    // Four read-only tools whose closed schemas name no authority.
    let tools = rpc(&server, &[("Authorization", &bearer)], &list).json();
    let tools = tools["result"]["tools"].as_array().unwrap().clone();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["catalog", "propose", "show", "workspaces"]);
    for tool in &tools {
        let hints = &tool["annotations"];
        assert_eq!(
            (
                &hints["readOnlyHint"],
                &hints["destructiveHint"],
                &hints["idempotentHint"],
                &hints["openWorldHint"]
            ),
            (&json!(true), &json!(false), &json!(true), &json!(false)),
            "{tool}"
        );
        assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{tool}");
        let properties = tool["inputSchema"]["properties"].as_object().unwrap();
        for forged in FORGED {
            assert!(!properties.contains_key(forged), "{tool}");
        }
    }

    // The creation rows of the table, through `propose`: the console's decision for each.
    for row in case.table["creation"].as_array().unwrap() {
        let caller = row["caller"].as_str().unwrap();
        let shorts: Vec<&str> = (row["select"].as_array().unwrap().iter())
            .map(|s| s.as_str().unwrap())
            .collect();
        let name = format!("{caller} {}", shorts.join("+"));
        let token = case.agent(caller);
        let proposal = answer(
            &server,
            &token,
            "propose",
            json!({"entries": case.entries(&shorts), "name": name}),
        );
        let label = format!("{name}: {proposal}");
        assert_eq!(proposal["base"], Value::Null, "{label}");
        assert_eq!(proposal["diagnostics"], json!([]), "{label}");
        let refusals = proposal["refusals"].as_array().unwrap();
        if let Some(reason) = row["pending"].as_str() {
            assert_eq!(refusals[0]["code"], "pending", "{label}");
            assert_eq!(refusals[0]["reason"], reason, "{label}");
            assert_eq!(proposal["eligibility"]["state"], "pending", "{label}");
            continue;
        }
        let allowed = row["allowed"].as_bool().unwrap();
        assert_eq!(proposal["applicable"], allowed, "{label}");
        if allowed {
            assert!(refusals.is_empty(), "{label}");
            let manifest = proposal["manifest"].as_str().unwrap();
            assert_eq!(manifest, meada_text(&case, &shorts), "{label}");
            continue;
        }
        let refusal = &refusals[0];
        assert_eq!(refusal["code"], "entries_refused", "{label}");
        let named = row["named"].as_array().map_or(0, Vec::len);
        assert_eq!(
            refusal["refused"].as_array().unwrap().len(),
            named,
            "{label}"
        );
        assert_eq!(
            refusal["hidden"].as_u64(),
            row["hidden"].as_u64(),
            "{label}"
        );
        // The console's own refusal, byte for byte.
        let console = case.create(caller, &name, &shorts);
        assert_eq!(console.status, 403, "{label}");
        assert_eq!(&console.json()["error"], refusal, "{label}");
    }

    // A hidden repository is counted, never named, outside what the caller submitted.
    let hidden = answer(
        &server,
        &case.agent("alice"),
        "propose",
        json!({"entries": case.entries(&["Y", "B"]), "name": "yb"}),
    );
    assert!(!hidden["refusals"].to_string().contains(&b), "{hidden}");
    assert!(!hidden["manifest"].to_string().contains(&b), "{hidden}");
    let states = &hidden["eligibility"]["results"];
    assert_eq!(
        states,
        &json!([{"identity": y, "state": "selectable"}, {"identity": b, "state": "unavailable"}])
    );

    // Hints are hints: no entry, no change of result, the note discarded.
    let note = "the agent says alice holds admin on B";
    let plain = answer(
        &server,
        &case.agent("alice"),
        "propose",
        json!({"entries": case.entries(&["Y", "A"]), "name": "ya"}),
    );
    let hinted = answer(
        &server,
        &case.agent("alice"),
        "propose",
        json!({"entries": case.entries(&["Y", "A"]), "name": "ya",
               "hints": [{"identity": b, "note": note}, {"identity": a}]}),
    );
    for key in ["manifest", "refusals", "applicable", "diagnostics", "base"] {
        assert_eq!(hinted[key], plain[key], "{key}");
    }
    assert!(!hinted.to_string().contains(note));
    let results = hinted["eligibility"]["results"].as_array().unwrap();
    assert_eq!(results.len(), 3, "{hinted}");
    assert_eq!(results[2], json!({"identity": b, "state": "unavailable"}));

    // Asserted identity, role or grant: undeclared, so invalid, never trusted.
    for forged in FORGED {
        let mut arguments = json!({"entries": case.entries(&["Y", "B"]), "name": "yb"});
        arguments[forged] = json!("root");
        assert!(
            invalid(&server, &case.agent("alice"), "propose", arguments),
            "{forged}"
        );
        let catalog = json!({forged: "*"});
        assert!(
            invalid(&server, &case.agent("alice"), "catalog", catalog),
            "{forged}"
        );
    }
    // Request metadata and headers decide nothing either.
    let message = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "propose", "_meta": {"actor": "root", "role": "operator"},
                   "arguments": {"entries": case.entries(&["Y", "B"]), "name": "yb"}},
    });
    let headers = [
        ("Authorization", bearer.as_str()),
        ("X-Forwarded-User", "root"),
        ("X-Meada-Role", "operator"),
    ];
    let reply = rpc(&server, &headers, &message).json();
    assert_eq!(reply["result"]["structuredContent"], hidden, "{reply}");

    // The catalog is the console's: a claims grant and a missing login select nothing.
    let catalog = answer(&server, &case.agent("alice"), "catalog", json!({}));
    let listed: Vec<&str> = (catalog["repositories"].as_array().unwrap().iter())
        .map(|r| r["identity"].as_str().unwrap())
        .collect();
    assert_eq!(listed, [a.as_str(), y.as_str()]);
    let console = case.get("/api/v1/catalog", &case.token("alice")).json();
    assert_eq!(console["repositories"], catalog["repositories"]);
    let claims = answer(&server, &case.agent("claims"), "catalog", json!({}));
    assert_eq!(claims["repositories"], json!([]));
    let nologin = answer(&server, &case.agent("nologin"), "catalog", json!({}));
    assert_eq!(
        (&nologin["state"], &nologin["reason"]),
        (&json!("pending"), &json!("missing_login"))
    );
    let page = answer(&server, &case.agent("root"), "catalog", json!({"limit": 1}));
    assert_eq!(page["repositories"].as_array().unwrap().len(), 1);
    let next = page["next"].as_str().unwrap();
    let rest = answer(
        &server,
        &case.agent("root"),
        "catalog",
        json!({"after": next, "query": "github.com/acme/"}),
    );
    assert!(!rest["repositories"].to_string().contains(&a), "{rest}");

    // Bounds: arguments past them are invalid, a body past 80 KiB is a 413 unread.
    let alice = case.agent("alice");
    let big = "#".repeat((64 << 10) + 1);
    for (tool, arguments) in [
        ("propose", json!({"entries": big, "name": "n"})),
        (
            "propose",
            json!({"entries": "", "hints": vec![json!({"identity": y}); 51]}),
        ),
        (
            "propose",
            json!({"entries": "", "hints": [{"identity": y, "note": "n".repeat(257)}]}),
        ),
        (
            "propose",
            json!({"entries": "", "hints": [{"identity": "not an identity"}]}),
        ),
        (
            "propose",
            json!({"entries": "", "hints": [{"identity": y, "extra": 1}]}),
        ),
        ("catalog", json!({"limit": 0})),
        ("catalog", json!({"limit": 101})),
        ("catalog", json!({"query": "q".repeat(129)})),
        ("catalog", json!({"after": "!!"})),
        ("catalog", json!({"after": "Z".repeat(257)})),
        (
            "propose",
            json!({"entries": case.entries(&["Y"]), "workspace": "0123456789abcdef", "name": "n"}),
        ),
        ("workspaces", json!({"limit": 0})),
        ("workspaces", json!({"after": "zz"})),
    ] {
        let refused = invalid(&server, &alice, tool, arguments.clone());
        assert!(refused, "{tool} {arguments}");
    }
    let huge = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "propose", "arguments": {"entries": "#".repeat(90 << 10)}},
    });
    assert_eq!(
        rpc(&server, &[("Authorization", &bearer)], &huge).status,
        413
    );
    // An invalid manifest is a diagnostic naming its entry and key, not a refusal.
    let invalid = answer(
        &server,
        &alice,
        "propose",
        json!({"entries": format!("{}\npath = \"x\"\n", case.entries(&["Y"])), "name": "p"}),
    );
    let diagnostic = &invalid["diagnostics"][0];
    assert_eq!(
        (
            &diagnostic["code"],
            &diagnostic["entry"],
            &diagnostic["key"]
        ),
        (
            &json!("invalid_entries"),
            &json!(1),
            &json!("repository.path")
        ),
        "{invalid}"
    );
    assert_eq!(invalid["applicable"], false);
    let unnamed = answer(
        &server,
        &alice,
        "propose",
        json!({"entries": case.entries(&["Y"])}),
    );
    assert_eq!(
        unnamed["diagnostics"][0]["code"], "invalid_workspace",
        "{unnamed}"
    );

    // Past the calls in flight, a call is a 503 `busy` before its body is read: a server allowed
    // none shows it.
    let full = case.proposals_with(&[("MEADA_PROPOSALS_CALLS", "0")]);
    let busy = rpc(&full, &[("Authorization", &bearer)], &list);
    assert_eq!(
        (busy.status, busy.json()["error"]["code"].clone()),
        (503, json!("busy"))
    );
    drop(full);

    // No call reached GitHub; nothing on stderr holds a token, a note or a manifest.
    assert_eq!(case.github.api.with(|s| s.requests), requests);
    let tokens = vec![case.agent("alice"), case.agent("root"), forged];
    let stderr = server.stderr();
    assert!(
        stderr.contains("proposals: alice propose -: entries_refused"),
        "{stderr}"
    );
    assert!(
        !stderr.contains(note) && !stderr.contains("repository"),
        "{stderr}"
    );
    case.github.clean(&stderr, &tokens);
    case.github.clean(&case.console.stderr(), &[]);
}

/// The canonical text the console stores for the table's `shorts`.
fn meada_text(case: &Case, shorts: &[&str]) -> String {
    let one = |s: &&str| format!("[[repository]]\nrepo = \"{}\"\n", case.identity(s));
    shorts.iter().map(one).collect::<Vec<_>>().join("\n")
}

/// A proposal over an existing workspace is the console's save with persistence off: the caller
/// lists and shows what it may see (a hidden workspace is the one `not_found` of an unknown id),
/// its proposal names the base, the diff against the entries it reads and the effective
/// configuration, and a peer is refused as a non-operator. Equal calls give equal bytes, and
/// the console's records, versions, history and audit are as they were: no version stored, no
/// pointer moved. A person then saves the manifest at the base in the console, after which the
/// old base is stale.
#[test]
fn a_proposal_changes_nothing_and_a_person_saves_it_in_the_console() {
    let Some(case) = Case::new("ws_proposals_base") else {
        return;
    };
    case.refresh();
    case.ready();
    let id_of = |reply: &Reply| {
        assert!(matches!(reply.status, 200 | 201), "{}", reply.body);
        reply.json()["id"].as_str().unwrap().to_string()
    };
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let yab = id_of(&case.create("carol", "all", &["Y", "A", "B"]));
    let only_y = id_of(&case.create("yoda", "Y alone", &["Y"]));
    let server = case.proposals();
    read_a_set(&server, &case.agent("root"));
    let before = case.state(&[&ya, &yb, &yab, &only_y]);
    let (alice, dave, bob) = (case.agent("alice"), case.agent("dave"), case.agent("bob"));
    let (y, b) = (case.identity("Y"), case.identity("B"));

    // What each caller sees: the console's discovery rule.
    let listed = |token: &str| -> Vec<(String, bool, i64)> {
        let page = answer(&server, token, "workspaces", json!({}));
        (page["workspaces"].as_array().unwrap().iter())
            .map(|w| {
                (
                    w["id"].as_str().unwrap().to_string(),
                    w["operate"].as_bool().unwrap(),
                    w["seq"].as_i64().unwrap(),
                )
            })
            .collect()
    };
    let sorted = |mut list: Vec<(String, bool, i64)>| {
        list.sort();
        list
    };
    let peer = |id: &String| (id.clone(), false, 1);
    let operator = |id: &String| (id.clone(), true, 1);
    assert_eq!(listed(&alice), sorted(vec![operator(&ya), peer(&only_y)]));
    assert_eq!(listed(&dave), sorted(vec![peer(&ya), peer(&only_y)]));
    assert_eq!(listed(&bob), sorted(vec![operator(&yb), peer(&only_y)]));
    assert_eq!(listed(&case.agent("yoda")), [operator(&only_y)]);
    let shown = answer(&server, &alice, "show", json!({"workspace": ya}));
    assert_eq!(shown["seq"], 1);
    assert_eq!(shown["manifest"], meada_text(&case, &["Y", "A"]));
    let unknown = refused(
        &server,
        &bob,
        "show",
        json!({"workspace": "0123456789abcdef"}),
    );
    assert_eq!(unknown["code"], "not_found");
    assert_eq!(
        refused(&server, &bob, "show", json!({"workspace": ya})),
        unknown
    );
    // The table's discovery rows through `show`: a hidden workspace is the unknown id's answer.
    let ids = [
        ("w_ya", &ya),
        ("w_yb", &yb),
        ("w_yab", &yab),
        ("w_y", &only_y),
    ];
    for row in case.table["discovery"].as_array().unwrap() {
        let caller = row["caller"].as_str().unwrap();
        let (_, id) = ids
            .iter()
            .find(|(name, _)| row["workspace"] == *name)
            .unwrap();
        let reply = call(
            &server,
            &case.agent(caller),
            "show",
            json!({"workspace": id}),
        );
        let result = &reply["result"];
        match row["visible"].as_bool().unwrap() {
            true => assert_eq!(result["isError"], false, "{row}: {reply}"),
            false => assert_eq!(result["structuredContent"]["error"], unknown, "{row}"),
        }
    }

    // A change alice may make: Y served under a host, A kept.
    let text = format!(
        "[[repository]]\nrepo = \"{y}\"\n[repository.serve]\nhosts = [\"y.example\"]\n\n\
         [[repository]]\nrepo = \"{}\"\n",
        case.identity("A")
    );
    let arguments = json!({"workspace": ya, "entries": text});
    let proposal = answer(&server, &alice, "propose", arguments.clone());
    assert_eq!(proposal["applicable"], true, "{proposal}");
    let version = shown["version"].clone();
    assert_eq!(
        proposal["base"],
        json!({"workspace": ya, "seq": 1, "version": version})
    );
    assert_eq!(
        proposal["diff"],
        json!({"added": [], "removed": [], "changed": [{"repo": y, "keys": ["serve"]}]})
    );
    assert_eq!(proposal["effective"]["workspace"], json!(ya));
    assert_eq!(
        proposal["effective"]["entries"].as_array().unwrap().len(),
        2
    );
    assert!(!proposal["manifest"].as_str().unwrap().contains("ingestion"));
    // Equal calls, equal bytes.
    let again = call(&server, &alice, "propose", arguments.clone());
    assert_eq!(again["result"]["structuredContent"], proposal);
    // Adding B is refused as the console's save is: B counted, never named.
    let wider = answer(
        &server,
        &alice,
        "propose",
        json!({"workspace": ya, "entries": case.entries(&["Y", "A", "B"])}),
    );
    assert_eq!(wider["applicable"], false);
    assert_eq!(wider["refusals"][0]["code"], "entries_refused");
    assert_eq!(wider["refusals"][0]["hidden"], 1);
    assert!(!wider["refusals"].to_string().contains(&b));
    // A peer is no operator; a caller who cannot see the workspace gets the unknown id's answer.
    let peer = answer(&server, &dave, "propose", arguments.clone());
    assert_eq!(peer["refusals"][0]["code"], "not_operator", "{peer}");
    assert_eq!(
        refused(&server, &bob, "propose", arguments.clone()),
        unknown
    );

    // Nothing stored, no pointer moved, no audit record.
    assert_eq!(case.state(&[&ya, &yb, &yab, &only_y]), before);

    // A person saves the manifest at the proposal's base in the console, then applies it.
    let manifest = proposal["manifest"].as_str().unwrap();
    let saved = case.save("alice", &ya, manifest, 1);
    assert_eq!(saved.status, 201, "{}", saved.body);
    let saved_version = saved.json()["version"].as_str().unwrap().to_string();
    let applied = case.apply("alice", &ya, &saved_version, 1);
    assert_eq!(applied.status, 200, "{}", applied.body);
    // The old proposal's base is stale now, and the console refuses it.
    let stale = case.save("alice", &ya, manifest, 1);
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.json()["error"]["code"], "stale_base");
    // A new proposal names the new base and has nothing left to change.
    let now = answer(&server, &alice, "propose", arguments);
    assert_eq!(now["base"]["seq"], 2);
    assert_eq!(
        now["diff"],
        json!({"added": [], "removed": [], "changed": []})
    );
    // The creator loses A: its proposal composes over A, which stays in place unnamed, and no
    // field of the proposal or of `show` names it.
    let a = case.identity("A");
    case.revoke(&[("alice", "A")]);
    let start = Instant::now();
    while answer(&server, &alice, "catalog", json!({}))["repositories"]
        .to_string()
        .contains(&a)
    {
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(50));
    }
    let only = answer(
        &server,
        &alice,
        "propose",
        json!({"workspace": ya, "entries": case.entries(&["Y"])}),
    );
    assert_eq!(only["applicable"], true, "{only}");
    assert_eq!(only["manifest"], meada_text(&case, &["Y"]));
    assert!(!only.to_string().contains(&a), "{only}");
    let shown = answer(&server, &alice, "show", json!({"workspace": ya}));
    assert_eq!(shown["hidden"], 1, "{shown}");
    assert!(!shown.to_string().contains(&a), "{shown}");
    // Naming A is no way around it: the proposal is the same, A still unnamed in it.
    let named = answer(
        &server,
        &alice,
        "propose",
        json!({"workspace": ya, "entries": case.entries(&["Y", "A"])}),
    );
    for key in ["manifest", "diff", "refusals", "effective", "applicable"] {
        assert_eq!(named[key], only[key], "{key}");
    }
    // A rename is the console's: no name or description with a base.
    let renamed = json!({"workspace": ya, "entries": case.entries(&["Y"]), "name": "n"});
    assert!(invalid(&server, &alice, "propose", renamed));

    // A creation the console's transaction would refuse: the same workspace created and deleted.
    let gone = id_of(&case.create("bob", "gone", &["Y"]));
    let deleted = case.post(
        &format!("/api/v1/workspaces/{gone}/delete"),
        &case.token("bob"),
        &json!({"base": 1}),
    );
    assert_eq!(deleted.status, 200, "{}", deleted.body);
    let again = answer(
        &server,
        &bob,
        "propose",
        json!({"entries": case.entries(&["Y"]), "name": "gone"}),
    );
    assert_eq!(again["applicable"], false);
    assert_eq!(again["refusals"][0]["code"], "workspace_deleted", "{again}");
    let tokens = vec![alice, dave, bob];
    case.github.clean(&server.stderr(), &tokens);
    case.github.clean(&case.console.stderr(), &[]);
}

/// Eligibility is read from the newest set, never cached past its age: once the set is stale,
/// a proposal that was applicable is pending `stale_set` for everyone, the administrator
/// included, and the catalog lists nothing.
#[test]
fn a_stale_set_makes_every_proposal_pending() {
    let Some(case) = Case::aged("ws_proposals_stale", false, 4) else {
        return;
    };
    case.refresh();
    case.ready();
    let server = case.proposals();
    read_a_set(&server, &case.agent("root"));
    let arguments = json!({"entries": case.entries(&["Y", "A"]), "name": "ya"});
    let fresh = answer(&server, &case.agent("alice"), "propose", arguments.clone());
    assert_eq!(fresh["applicable"], true, "{fresh}");
    let start = Instant::now();
    loop {
        let catalog = answer(&server, &case.agent("alice"), "catalog", json!({}));
        if catalog["reason"] == "stale_set" {
            assert_eq!(catalog["repositories"], json!([]));
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "{catalog}");
        std::thread::sleep(Duration::from_millis(200));
    }
    for caller in ["alice", "root"] {
        let stale = answer(&server, &case.agent(caller), "propose", arguments.clone());
        assert_eq!(stale["applicable"], false, "{caller}: {stale}");
        assert_eq!(
            (
                &stale["refusals"][0]["code"],
                &stale["refusals"][0]["reason"]
            ),
            (&json!("pending"), &json!("stale_set")),
            "{caller}: {stale}"
        );
        let states = stale["eligibility"]["results"].as_array().unwrap();
        assert!(states.iter().all(|s| s["state"] == "pending"), "{stale}");
    }
}

/// A user the SCIM directory deleted is refused by the proposal server as by the console, until
/// it is provisioned active again; a user never provisioned keeps normal admission.
#[cfg(feature = "directory")]
#[test]
fn a_deleted_user_is_refused_until_provisioned_active() {
    use super::super::directory::{USER, expect};
    let Some(case) = Case::start("ws_proposals_scim", true) else {
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
    let server = case.proposals();
    read_a_set(&server, &case.agent("root"));
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let status = |caller: &str| {
        let bearer = format!("Bearer {}", case.agent(caller));
        rpc(&server, &[("Authorization", &bearer)], &list)
    };
    assert_eq!(status("alice").status, 200);
    expect(
        &case.console,
        "DELETE",
        &format!("/Users/{alice}"),
        None,
        204,
    );
    let blocked = status("alice");
    assert_eq!(
        (blocked.status, blocked.json()["error"]["code"].clone()),
        (403, json!("deprovisioned")),
        "{}",
        blocked.body
    );
    assert_eq!(status("carol").status, 200);
    let again = scim_user("alice", false);
    assert_eq!(status("alice").status, 403);
    let put = json!({"schemas": [USER], "userName": "alice", "active": true});
    expect(
        &case.console,
        "PUT",
        &format!("/Users/{again}"),
        Some(&put),
        200,
    );
    assert_eq!(status("alice").status, 200);
}
