//! The failure side of the enterprise acceptance (#347, ADR 0020 and 0021): a refresh whose role
//! check, index or local publication fails leaves every workspace on the last complete
//! generation, the permissions set the job last committed keeps deciding (a revoked grant is not
//! restored by a failure, only by a later complete set), and a refresh that is not complete never
//! moves the pointer. Public processes as in the sibling modules: the console, the job's
//! commands and a node over the loopback issuer, the fake GitHub and the local services.

use super::*;

/// The id a creation answered.
fn id_of(reply: &Reply) -> String {
    assert_eq!(reply.status, 201, "{}", reply.body);
    reply.json()["id"].as_str().unwrap().to_string()
}

/// A step of the job's refresh that must stop: a non-zero exit with a reason on stderr.
fn stops(case: &Case, args: &[&str]) {
    let (stdout, stderr, code) = case.job(args);
    assert_ne!(code, 0, "{args:?} must fail: {stdout}");
    assert!(!stderr.is_empty(), "{args:?} fails with a reason");
}

/// The generation the node pulls from the store: the pointer.
fn pointer(case: &Case) -> String {
    let (stdout, stderr, code) = pull(&case.dir, NODE_MANIFEST, &[]);
    assert_eq!(code, Some(0), "{stdout} {stderr}");
    stdout.trim().to_string()
}

/// The roles, fetch/index and publication of one refresh fail in turn: each leaves the current
/// generation queryable for the peers who still hold their grants, a role check that cannot
/// finish writes no set (the previous one keeps deciding), a revocation the job committed stays
/// effective whatever happens to the publication, and the grant comes back only through a later
/// complete set.
#[test]
fn every_failed_stage_keeps_the_generation_and_a_revoked_grant_never_revives() {
    let Some(case) = Case::new("ws_failed_stages") else {
        return;
    };
    case.refresh();
    case.ready();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let (y, a, b) = (case.identity("Y"), case.identity("A"), case.identity("B"));
    let route = |id: &str, identity: &str| format!("/-/workspaces/{id}/{identity}");
    let refresh = || {
        case.job_ok(&["workspace", "pull", "--workspace", MANIFEST]);
        case.roles();
        case.job_ok(&["index", "--workspace", MANIFEST]);
        case.job_ok(&["snapshot", "publish", "--workspace", MANIFEST])
            .trim()
            .to_string()
    };
    let g1 = refresh();
    let (node, held) = case.node();
    assert_eq!(held, g1);
    let (alice, bob) = (case.node_token("alice"), case.node_token("bob"));
    until_status(&node, &route(&ya, &a), &alice, 200);
    until_status(&node, &route(&yb, &b), &bob, 200);

    // 1. The role check fails (GitHub answers 500): it exits non-zero and writes no set, so the
    //    previous one keeps deciding and the pointer does not move.
    case.github.api.with(|s| s.listing_status = Some(500));
    case.job_ok(&["workspace", "pull", "--workspace", MANIFEST]);
    stops(&case, &["github-app", "roles", "--workspace", MANIFEST]);
    assert_eq!(pointer(&case), g1);
    assert_eq!(case.listed("alice"), vec![ya.clone()]);
    assert_eq!(node_call(&node, &route(&ya, &y), &alice, "handle").0, 200);

    // 2. Alice loses A and the next role check completes: the revocation is effective at the
    //    node while the generation is still the one that recorded her workspace.
    case.github.api.with(|s| s.listing_status = None);
    case.revoke(&[("alice", "A")]);
    until_status(&node, &route(&ya, &a), &alice, 404);
    assert_eq!(pointer(&case), g1, "a revocation publishes nothing");
    assert_eq!(node_call(&node, &route(&yb, &b), &bob, "handle").0, 200);

    // 3. A later refresh fails at the role check again, while GitHub already shows A restored:
    //    no set is written, so the grant does not come back and nothing else moves.
    case.grant_all_but(&[]);
    case.github.api.with(|s| s.listing_status = Some(500));
    stops(&case, &["github-app", "roles", "--workspace", MANIFEST]);
    assert_eq!(node_call(&node, &route(&ya, &a), &alice, "handle").0, 404);
    assert_eq!(case.listed("alice"), vec![ya.clone()]);
    case.github.api.with(|s| s.listing_status = None);
    case.grant_all_but(&[("alice", "A")]);

    // 4. The fetch/index stage fails (the fetch is the operator's script, so the test forces what Meada
    //    owns): a member's mirror is missing (B's; A's entry is already
    //    excluded for lack of a sponsor and needs none), so `index` stops and nothing is
    //    published. The pointer stays, bob still reads his workspace from the last complete
    //    generation, and alice's revocation stays effective.
    let mirror = case.dir.join("mirrors").join(&b);
    fs::rename(&mirror, case.dir.join("b-away")).unwrap();
    case.job_ok(&["workspace", "pull", "--workspace", MANIFEST]);
    case.roles();
    stops(&case, &["index", "--workspace", MANIFEST]);
    assert_eq!(pointer(&case), g1);
    assert_eq!(node_call(&node, &route(&yb, &y), &bob, "handle").0, 200);
    assert_eq!(node_call(&node, &route(&ya, &a), &alice, "handle").0, 404);
    fs::rename(case.dir.join("b-away"), &mirror).unwrap();

    // 5. Local publication fails: the index is complete but the publish stops (here the
    //    index directory is missing), and the pointer still does not move.
    case.job_ok(&["index", "--workspace", MANIFEST]);
    let indexes = case.dir.join("indexes");
    let broken = case.dir.join("indexes-away");
    fs::rename(&indexes, &broken).unwrap();
    stops(&case, &["snapshot", "publish", "--workspace", MANIFEST]);
    fs::rename(&broken, &indexes).unwrap();
    assert_eq!(pointer(&case), g1);
    assert_eq!(node_call(&node, &route(&yb, &b), &bob, "handle").0, 200);

    // 6. Only a later complete set brings the grant back, by the roles GitHub then shows.
    case.grant_all_but(&[]);
    case.roles();
    case.await_check("alice", "A", "selectable");
    until_status(&node, &route(&ya, &a), &alice, 200);
    case.github.clean(&node.stderr(), &[]);
    case.github.clean(&case.console.stderr(), &[]);
}
