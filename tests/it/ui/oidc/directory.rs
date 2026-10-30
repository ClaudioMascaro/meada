//! The directory's SCIM endpoint (#334, ADR 0016): `/scim/v2/` on the console's listener with
//! `[ui]`, `[oidc]`, `[postgres]` and `[directory]`, behind a bearer token of its own, answering
//! what Entra ID's and Okta's provisioning send. Admission, the discovery documents, the 503 and
//! the start refusals need no service; the rest run against the local Postgres
//! `MEADA_TEST_POSTGRES_URL` names, in a schema of each test's own behind a loopback relay, and
//! print why and skip without it (fail with `MEADA_TEST_SERVICES_REQUIRED=1`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jsonwebtoken::Algorithm;
use serde_json::{Value, json};

use super::{PUBLIC, Reply, VERSION, ask, bearer, claims, console_dir, head, send_body, ui_oidc};
use crate::manifest::MANIFEST;
use crate::oidc::{Issuer, key, set, sign};
use crate::postgres::{Audited, audited};
use crate::server::Server;

/// The SCIM token of every console here: 40 characters.
pub(super) const TOKEN: &str = "scim-334-token-0123456789abcdefghijklmno";
pub(super) const USER: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
pub(super) const GROUP: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
const ENTERPRISE: &str = "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User";
const PATCH_OP: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";
const ERROR: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
/// The `[directory]` of every console here, its token in the default `MEADA_SCIM_TOKEN`.
pub(super) const DIRECTORY: &str =
    "\n[directory]\nuser_claim = \"oid\"\nuser_attribute = \"userName\"\n";

/// `text` appended to the console's manifest in `dir`.
pub(super) fn append(dir: &Path, text: &str) {
    let file = dir.join(MANIFEST);
    let source = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("{source}{text}")).unwrap();
}

/// `meada ui` in `dir` with `env`.
fn start(dir: &Path, env: &[(&str, &str)]) -> Server {
    let args = ["--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    Server::start_command(dir, "ui", &args, env)
}

/// A console with the directory over the local Postgres, its schema migrated, running.
fn provisioned(case: &'static str) -> Option<(Issuer, Audited, Server)> {
    let (_, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = console_dir(case, Some(VERSION), &head(&issuer.base), &ui_oidc());
    let audit = audited(&dir, case)?;
    append(&dir, &format!("{}{DIRECTORY}", audit.section));
    let console = start(&dir, &[("MEADA_SCIM_TOKEN", TOKEN)]);
    Some((issuer, audit, console))
}

/// `method` on `/scim/v2<path>` with the SCIM token and `body`, as SCIM JSON.
pub(super) fn scim(console: &Server, method: &str, path: &str, body: Option<&Value>) -> Reply {
    let authorization = bearer(TOKEN);
    let mut headers = vec![("Authorization", authorization.as_str())];
    if body.is_some() {
        headers.push(("Content-Type", "application/scim+json"));
    }
    let body = body.map_or(String::new(), Value::to_string);
    send_body(console, method, &format!("/scim/v2{path}"), &headers, &body)
}

/// `scim`, asserting `status`; the document when there is one.
pub(super) fn expect(
    console: &Server,
    method: &str,
    path: &str,
    body: Option<&Value>,
    status: u16,
) -> Value {
    let reply = scim(console, method, path, body);
    assert_eq!(reply.status, status, "{method} {path}: {}", reply.body);
    match reply.body.is_empty() {
        true => Value::Null,
        false => {
            assert_eq!(reply.header("content-type"), Some("application/scim+json"));
            reply.json()
        }
    }
}

/// A `PATCH` body of `operations`.
pub(super) fn patch(operations: Value) -> Value {
    json!({"schemas": [PATCH_OP], "Operations": operations})
}

/// The status and `scimType` of a refusal, checked to be RFC 7644's error.
fn scim_type(reply: &Reply) -> (u16, String) {
    let error = reply.json();
    assert_eq!(error["schemas"], json!([ERROR]), "{}", reply.body);
    assert_eq!(error["status"], json!(reply.status.to_string()));
    let kind = error["scimType"].as_str().unwrap_or("").to_string();
    (reply.status, kind)
}

/// The members of a group document, by id.
fn members(group: &Value) -> Vec<&str> {
    let listed = group["members"].as_array().expect("members");
    listed
        .iter()
        .map(|m| m["value"].as_str().unwrap())
        .collect()
}

/// `filter`, form-encoded for a query.
fn encoded(filter: &str) -> String {
    filter
        .replace(' ', "+")
        .replace('"', "%22")
        .replace('@', "%40")
}

/// The `scim:` lines of `stderr`.
fn lines(stderr: &str) -> Vec<&str> {
    stderr.lines().filter(|l| l.starts_with("scim: ")).collect()
}

/// Neither `text` nor any of `values` there: the token, the Postgres URL, attribute values.
fn assert_leaks_nothing(audit: &Audited, text: &str, values: &[&str]) {
    assert!(!text.contains(TOKEN), "the token in {text}");
    for value in values {
        assert!(!text.contains(value), "{value} in {text}");
    }
    audit.assert_no_url(text);
}

/// What the schema's table holds, as text, one row per line.
fn rows(audit: &Audited, sql: &str) -> Vec<String> {
    audit
        .admin
        .texts(&sql.replace("{s}", &format!("\"{}\"", audit.schema)))
}

/// Entra ID's provisioning as it sends it: the lookup by `userName`, `POST`, a `PATCH` of
/// attributes, `PATCH active` with `"False"`, the user shown inactive, a group with members added
/// and removed by path and by value list (204), `excludedAttributes=members`, then `DELETE`. Each
/// write leaves one `scim:` line; no line or error carries the token, a value or the URL. With
/// Postgres stopped the endpoint is a 503 and the console's other routes answer.
#[test]
fn an_entra_id_sequence_provisions_patches_and_deletes() {
    let Some((_issuer, audit, console)) = provisioned("ui_directory_entra") else {
        return;
    };
    let lookup = format!(
        "/Users?filter={}",
        encoded("userName eq \"alice@contoso.example\"")
    );
    let none = expect(&console, "GET", &lookup, None, 200);
    assert_eq!(none["totalResults"], json!(0));
    assert_eq!(none["Resources"], json!([]));
    let alice = json!({
        "schemas": [USER, ENTERPRISE],
        "externalId": "0a1b2c-alice",
        "userName": "alice@contoso.example",
        "active": true,
        "displayName": "Alice",
        "emails": [{"primary": true, "type": "work", "value": "alice@contoso.example"}],
        "name": {"formatted": "Alice Smith", "familyName": "Smith", "givenName": "Alice"},
        ENTERPRISE: {"department": "Engineering"},
    });
    let reply = scim(&console, "POST", "/Users", Some(&alice));
    assert_eq!(reply.status, 201, "{}", reply.body);
    let created = reply.json();
    let id = created["id"].as_str().unwrap().to_string();
    assert!(
        id.len() == 32
            && id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    let location = format!("{PUBLIC}/scim/v2/Users/{id}");
    assert_eq!(reply.header("location"), Some(location.as_str()));
    assert_eq!(created["meta"]["resourceType"], json!("User"));
    assert_eq!(created["meta"]["location"], json!(location));
    assert_eq!(created["meta"]["created"], created["meta"]["lastModified"]);
    assert_eq!(created["userName"], alice["userName"]);
    assert_eq!(created[ENTERPRISE], alice[ENTERPRISE]);
    let found = expect(&console, "GET", &lookup, None, 200);
    assert_eq!(found["totalResults"], json!(1));
    assert_eq!(found["Resources"][0]["id"], json!(id));
    let path = format!("/Users/{id}");
    let edited = patch(json!([
        {"op": "Replace", "path": "displayName", "value": "Alice Smith"},
        {"op": "Replace", "path": "emails[type eq \"work\"].value", "value": "alice.smith@contoso.example"},
        {"op": "Add", "path": "name.familyName", "value": "Smith-Jones"},
        {"op": "Replace", "path": format!("{ENTERPRISE}:department"), "value": "Research"},
    ]));
    let patched = expect(&console, "PATCH", &path, Some(&edited), 200);
    assert_eq!(patched["displayName"], json!("Alice Smith"));
    assert_eq!(
        patched["emails"][0]["value"],
        json!("alice.smith@contoso.example")
    );
    assert_eq!(patched["name"]["familyName"], json!("Smith-Jones"));
    assert_eq!(patched[ENTERPRISE]["department"], json!("Research"));
    let inactive = patch(json!([{"op": "Replace", "path": "active", "value": "False"}]));
    expect(&console, "PATCH", &path, Some(&inactive), 200);
    let shown = expect(&console, "GET", &path, None, 200);
    assert_eq!(shown["active"], json!(false));
    assert_eq!(shown["displayName"], json!("Alice Smith"));
    assert_eq!(shown["meta"]["created"], created["meta"]["created"]);
    let bob =
        json!({"schemas": [USER], "userName": "bob@contoso.example", "externalId": "3d4e-bob"});
    let bob = expect(&console, "POST", "/Users", Some(&bob), 201)["id"]
        .as_str()
        .unwrap()
        .to_string();

    let group = json!({"schemas": [GROUP], "externalId": "g-eng-77", "displayName": "Engineering", "members": []});
    let created = expect(&console, "POST", "/Groups", Some(&group), 201);
    let group = created["id"].as_str().unwrap().to_string();
    assert_eq!(members(&created), Vec::<&str>::new());
    let at = format!("/Groups/{group}");
    let add =
        patch(json!([{"op": "Add", "path": "members", "value": [{"value": id}, {"value": bob}]}]));
    assert_eq!(scim(&console, "PATCH", &at, Some(&add)).status, 204);
    let mut both = vec![id.as_str(), bob.as_str()];
    both.sort();
    assert_eq!(members(&expect(&console, "GET", &at, None, 200)), both);
    let by_path = patch(json!([{"op": "Remove", "path": format!("members[value eq \"{id}\"]")}]));
    assert_eq!(scim(&console, "PATCH", &at, Some(&by_path)).status, 204);
    assert_eq!(
        members(&expect(&console, "GET", &at, None, 200)),
        [bob.as_str()]
    );
    let by_value = patch(json!([{"op": "Remove", "path": "members", "value": [{"value": bob}]}]));
    assert_eq!(scim(&console, "PATCH", &at, Some(&by_value)).status, 204);
    assert_eq!(
        members(&expect(&console, "GET", &at, None, 200)),
        Vec::<&str>::new()
    );
    assert_eq!(scim(&console, "PATCH", &at, Some(&add)).status, 204);
    let listed = format!(
        "/Groups?excludedAttributes=members&filter={}",
        encoded("displayName eq \"Engineering\"")
    );
    let listed = expect(&console, "GET", &listed, None, 200);
    assert_eq!(listed["totalResults"], json!(1));
    assert!(listed["Resources"][0].get("members").is_none(), "{listed}");
    let one = expect(
        &console,
        "GET",
        &format!("{at}?excludedAttributes=members"),
        None,
        200,
    );
    assert!(one.get("members").is_none() && one["displayName"] == json!("Engineering"));

    // A deleted user leaves every group, and a revocation marker.
    assert_eq!(expect(&console, "DELETE", &path, None, 204), Value::Null);
    expect(&console, "GET", &path, None, 404);
    assert_eq!(
        expect(&console, "GET", &lookup, None, 200)["totalResults"],
        json!(0)
    );
    assert_eq!(
        members(&expect(&console, "GET", &at, None, 200)),
        [bob.as_str()]
    );
    let marked = rows(
        &audit,
        "SELECT user_name || ' ' || external_id FROM {s}.directory_revoked",
    );
    assert_eq!(marked, ["alice@contoso.example 0a1b2c-alice"]);
    expect(&console, "DELETE", &at, None, 204);
    expect(&console, "GET", &at, None, 404);

    // Postgres stopped: a 503 in the same schema; the console's other routes still answer.
    audit.relay.close();
    let down = scim(&console, "GET", &path, None);
    assert_eq!(down.status, 503, "{}", down.body);
    assert_eq!(down.json()["schemas"], json!([ERROR]));
    let refused = scim(&console, "POST", "/Users", Some(&alice));
    assert_eq!(refused.status, 503);
    assert_eq!(super::get(&console, "/api/v1/config").status, 200);

    let stderr = console.stderr();
    assert_eq!(
        lines(&stderr),
        [
            format!("scim: POST Users {id}: 201"),
            format!("scim: PATCH Users {id}: 200"),
            format!("scim: PATCH Users {id}: 200"),
            format!("scim: POST Users {bob}: 201"),
            format!("scim: POST Groups {group}: 201"),
            format!("scim: PATCH Groups {group}: 204"),
            format!("scim: PATCH Groups {group}: 204"),
            format!("scim: PATCH Groups {group}: 204"),
            format!("scim: PATCH Groups {group}: 204"),
            format!("scim: DELETE Users {id}: 204"),
            format!("scim: DELETE Groups {group}: 204"),
            "scim: POST Users -: 503".to_string(),
        ]
    );
    let values = [
        "alice@contoso.example",
        "alice.smith@contoso.example",
        "0a1b2c-alice",
        "bob@contoso.example",
        "Engineering",
        "Research",
        "Smith",
    ];
    assert_leaks_nothing(&audit, &stderr, &values);
}

/// Okta's provisioning: the lookup by `userName`, `POST`, `PUT` of the whole user, `PATCH`
/// `active=false` without a path, a group created with no members, `PUT` replacing its members,
/// a rename, `DELETE`; and paging with `startIndex` and `count=100` over 150 users.
#[test]
fn an_okta_sequence_and_paging_over_150_users() {
    let Some((_issuer, audit, console)) = provisioned("ui_directory_okta") else {
        return;
    };
    let lookup = format!(
        "/Users?filter={}&startIndex=1&count=100",
        encoded("userName eq \"carol@okta.example\"")
    );
    assert_eq!(
        expect(&console, "GET", &lookup, None, 200)["totalResults"],
        json!(0)
    );
    let carol = json!({
        "schemas": [USER],
        "userName": "carol@okta.example",
        "name": {"givenName": "Carol", "familyName": "Diaz"},
        "emails": [{"primary": true, "value": "carol@okta.example", "type": "work"}],
        "displayName": "Carol Diaz",
        "locale": "en-US",
        "externalId": "00u1okta",
        "groups": [],
        "password": {"value": "Secr3t-334!"},
        "active": true,
    });
    let created = expect(&console, "POST", "/Users", Some(&carol), 201);
    let id = created["id"].as_str().unwrap().to_string();
    assert!(created.get("password").is_none(), "{created}");
    let path = format!("/Users/{id}");
    let mut whole = created.clone();
    whole["name"]["familyName"] = json!("Diaz-Ruiz");
    whole["displayName"] = json!("Carol Diaz-Ruiz");
    whole["password"] = json!("Secr3t-334!");
    let replaced = expect(&console, "PUT", &path, Some(&whole), 200);
    assert_eq!(replaced["id"], json!(id));
    assert_eq!(replaced["name"]["familyName"], json!("Diaz-Ruiz"));
    assert_eq!(replaced["meta"]["created"], created["meta"]["created"]);
    let off = patch(json!([{"op": "replace", "value": {"active": false}}]));
    assert_eq!(
        expect(&console, "PATCH", &path, Some(&off), 200)["active"],
        json!(false)
    );
    let stored = rows(&audit, "SELECT resource::text FROM {s}.directory_users");
    assert!(
        stored.len() == 1 && !stored[0].contains("Secr3t") && !stored[0].contains("password"),
        "{stored:?}"
    );

    let group = json!({"schemas": [GROUP], "displayName": "Okta Admins", "members": []});
    let group = expect(&console, "POST", "/Groups", Some(&group), 201)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let at = format!("/Groups/{group}");
    let filled = json!({
        "schemas": [GROUP], "id": group, "displayName": "Okta Admins",
        "members": [{"value": id, "display": "carol@okta.example"}],
    });
    let replaced = expect(&console, "PUT", &at, Some(&filled), 200);
    assert_eq!(members(&replaced), [id.as_str()]);
    let rename =
        patch(json!([{"op": "replace", "value": {"id": group, "displayName": "Okta Owners"}}]));
    assert_eq!(scim(&console, "PATCH", &at, Some(&rename)).status, 204);
    let renamed = expect(&console, "GET", &at, None, 200);
    assert_eq!(renamed["displayName"], json!("Okta Owners"));
    assert_eq!(members(&renamed), [id.as_str()]);
    let emptied = json!({"schemas": [GROUP], "displayName": "Okta Owners", "members": []});
    assert_eq!(
        members(&expect(&console, "PUT", &at, Some(&emptied), 200)),
        Vec::<&str>::new()
    );
    expect(&console, "DELETE", &at, None, 204);
    expect(&console, "DELETE", &path, None, 204);

    let mut ids = Vec::new();
    for n in 0..150 {
        let user = json!({"schemas": [USER], "userName": format!("user{n:03}@okta.example")});
        ids.push(expect(&console, "POST", "/Users", Some(&user), 201)["id"].clone());
    }
    let first = expect(&console, "GET", "/Users?startIndex=1&count=100", None, 200);
    assert_eq!(
        (
            &first["totalResults"],
            &first["startIndex"],
            &first["itemsPerPage"]
        ),
        (&json!(150), &json!(1), &json!(100))
    );
    let second = expect(
        &console,
        "GET",
        "/Users?startIndex=101&count=100",
        None,
        200,
    );
    assert_eq!(
        (
            &second["totalResults"],
            &second["startIndex"],
            &second["itemsPerPage"]
        ),
        (&json!(150), &json!(101), &json!(50))
    );
    let paged: Vec<Value> = (first["Resources"].as_array().unwrap().iter())
        .chain(second["Resources"].as_array().unwrap())
        .map(|user| user["id"].clone())
        .collect();
    assert_eq!(paged, ids, "every user once, in the order created");
    let capped = expect(&console, "GET", "/Users?count=500", None, 200);
    assert_eq!(capped["itemsPerPage"], json!(100));
    let past = expect(&console, "GET", "/Users?startIndex=151", None, 200);
    assert_eq!(
        (&past["itemsPerPage"], &past["Resources"]),
        (&json!(0), &json!([]))
    );
    let stderr = console.stderr();
    assert_eq!(lines(&stderr).len(), 9 + 150, "{stderr}");
    assert_leaks_nothing(
        &audit,
        &stderr,
        &["carol@okta.example", "Secr3t-334!", "Okta Owners"],
    );
}

/// The refusals: 409 `uniqueness` for a `userName` differing only in case (on `POST`, `PUT` and
/// `PATCH`), a repeated `externalId` and a repeated group name; a member naming no user is a 400
/// `invalidValue`; an unsupported filter, path and operation each get their `scimType`; an unknown
/// id is a 404. A `PATCH` whose later operation fails writes nothing; `id` and `meta` stay Meada's;
/// a body over 1 MiB and a resource over 64 KiB, before or after a `PATCH`, are 413 without
/// `scimType`. Concurrent writes of one `userName` store one user. No error echoes a value.
#[test]
fn refusals_carry_their_scim_type_and_write_nothing() {
    let Some((_issuer, audit, console)) = provisioned("ui_directory_refusals") else {
        return;
    };
    let mut errors = Vec::new();
    let mut refused = |method: &str, path: &str, body: Option<&Value>, expected: (u16, &str)| {
        let reply = scim(&console, method, path, body);
        errors.push(reply.body.clone());
        let (status, kind) = scim_type(&reply);
        assert_eq!(
            (status, kind.as_str()),
            expected,
            "{method} {path}: {}",
            reply.body
        );
    };
    let alice = json!({"userName": "alice@contoso.example", "externalId": "ext-alice-91", "displayName": "Alice"});
    let alice = expect(&console, "POST", "/Users", Some(&alice), 201);
    let id = alice["id"].as_str().unwrap().to_string();
    let path = format!("/Users/{id}");
    let bob = json!({"userName": "bob@contoso.example"});
    let bob = expect(&console, "POST", "/Users", Some(&bob), 201)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let uniqueness = (409, "uniqueness");
    let upper = json!({"userName": "ALICE@contoso.example"});
    refused("POST", "/Users", Some(&upper), uniqueness);
    let same_external = json!({"userName": "carol@contoso.example", "externalId": "ext-alice-91"});
    refused("POST", "/Users", Some(&same_external), uniqueness);
    refused("PUT", &format!("/Users/{bob}"), Some(&upper), uniqueness);
    let rename =
        patch(json!([{"op": "replace", "path": "userName", "value": "Alice@Contoso.example"}]));
    refused("PATCH", &format!("/Users/{bob}"), Some(&rename), uniqueness);
    let group = json!({"displayName": "Platform Team", "externalId": "g-plat-5"});
    expect(&console, "POST", "/Groups", Some(&group), 201);
    refused("POST", "/Groups", Some(&group), uniqueness);
    let other = json!({"displayName": "Other Team"});
    let other = expect(&console, "POST", "/Groups", Some(&other), 201)["id"].clone();
    let other = format!("/Groups/{}", other.as_str().unwrap());
    refused(
        "PUT",
        &other,
        Some(&json!({"displayName": "Platform Team"})),
        uniqueness,
    );
    let renamed =
        patch(json!([{"op": "replace", "path": "displayName", "value": "Platform Team"}]));
    refused("PATCH", &other, Some(&renamed), uniqueness);
    let invalid = (400, "invalidValue");
    let stranger = json!({"displayName": "Strangers", "members": [{"value": "0".repeat(32)}]});
    refused("POST", "/Groups", Some(&stranger), invalid);
    let malformed = json!({"displayName": "Strangers", "members": [{"value": "x-1"}]});
    refused("POST", "/Groups", Some(&malformed), invalid);
    let filter = format!("/Users?filter={}", encoded("userName co \"alice\""));
    refused("GET", &filter, None, (400, "invalidFilter"));
    let unsupported =
        patch(json!([{"op": "replace", "path": "emails[type gt \"w\"].value", "value": "v"}]));
    refused("PATCH", &path, Some(&unsupported), (400, "invalidPath"));
    let operation = patch(json!([{"op": "copy", "path": "displayName", "value": "v"}]));
    refused("PATCH", &path, Some(&operation), (400, "invalidSyntax"));
    let unknown = format!("/Users/{}", "0".repeat(32));
    let reply = scim(&console, "GET", &unknown, None);
    assert_eq!(
        (reply.status, &reply.json()["schemas"]),
        (404, &json!([ERROR]))
    );
    assert_eq!(scim(&console, "PATCH", &unknown, Some(&rename)).status, 404);
    assert_eq!(scim(&console, "GET", "/Users/ALICE", None).status, 404);

    // Atomic: the first operation is undone by the failure of the second.
    let half = patch(json!([
        {"op": "replace", "path": "displayName", "value": "Changed"},
        {"op": "replace", "path": "emails[type eq \"home\"].value", "value": "h@contoso.example"},
    ]));
    refused("PATCH", &path, Some(&half), (400, "noTarget"));
    assert_eq!(
        expect(&console, "GET", &path, None, 200)["displayName"],
        json!("Alice")
    );
    // `id` and `meta` stay Meada's.
    let other = "f".repeat(32);
    let forged = patch(json!([
        {"op": "replace", "path": "id", "value": other},
        {"op": "replace", "path": "meta.created", "value": "2001-01-01T00:00:00Z"},
    ]));
    let kept = expect(&console, "PATCH", &path, Some(&forged), 200);
    assert_eq!(
        (&kept["id"], &kept["meta"]["created"]),
        (&json!(id), &alice["meta"]["created"])
    );
    let put = json!({"id": other, "meta": {"created": "x"}, "userName": "alice@contoso.example", "externalId": "ext-alice-91", "displayName": "Alice"});
    let kept = expect(&console, "PUT", &path, Some(&put), 200);
    assert_eq!(
        (&kept["id"], &kept["meta"]["created"]),
        (&json!(id), &alice["meta"]["created"])
    );
    expect(&console, "GET", &format!("/Users/{other}"), None, 404);

    // The caps: a body past 1 MiB, a resource past 64 KiB on `POST` and after a `PATCH`.
    let too_large = |reply: Reply| {
        assert_eq!(reply.status, 413, "{}", reply.body);
        let error = reply.json();
        assert!(
            error["schemas"] == json!([ERROR]) && error.get("scimType").is_none(),
            "{error}"
        );
    };
    let body = json!({"userName": "big@contoso.example", "x": "a".repeat(1 << 20)});
    too_large(scim(&console, "POST", "/Users", Some(&body)));
    let resource = json!({"userName": "big@contoso.example", "x": "a".repeat(65 << 10)});
    too_large(scim(&console, "POST", "/Users", Some(&resource)));
    let grown = patch(json!([{"op": "add", "path": "nickName", "value": "a".repeat(65 << 10)}]));
    too_large(scim(&console, "PATCH", &path, Some(&grown)));
    assert!(
        expect(&console, "GET", &path, None, 200)
            .get("nickName")
            .is_none()
    );
    let total = expect(&console, "GET", "/Users", None, 200)["totalResults"].clone();
    assert_eq!(total, json!(2));

    // A race: eight writers of one `userName`, one stored.
    let mut statuses: Vec<u16> = std::thread::scope(|scope| {
        let racers: Vec<_> = (0..8)
            .map(|n| {
                let console = &console;
                scope.spawn(move || {
                    let user = json!({"userName": "dave@contoso.example", "nickName": format!("racer {n}")});
                    scim(console, "POST", "/Users", Some(&user)).status
                })
            })
            .collect();
        racers.into_iter().map(|r| r.join().unwrap()).collect()
    });
    statuses.sort();
    assert_eq!(statuses, [201, 409, 409, 409, 409, 409, 409, 409]);
    // Four writers of one group name, one stored.
    let mut statuses: Vec<u16> = std::thread::scope(|scope| {
        let racers: Vec<_> = (0..4)
            .map(|_| {
                let console = &console;
                scope.spawn(move || {
                    let group = json!({"displayName": "Racers"});
                    scim(console, "POST", "/Groups", Some(&group)).status
                })
            })
            .collect();
        racers.into_iter().map(|r| r.join().unwrap()).collect()
    });
    statuses.sort();
    assert_eq!(statuses, [201, 409, 409, 409]);
    // A user deleted while a group adds it: whichever writes second sees the first, and no
    // membership outlives its user.
    for round in 0..5 {
        let user = json!({"userName": format!("erin{round}@contoso.example")});
        let user = expect(&console, "POST", "/Users", Some(&user), 201)["id"].clone();
        let group = json!({"displayName": format!("Round {round}")});
        let group = expect(&console, "POST", "/Groups", Some(&group), 201)["id"].clone();
        let (group, user) = (group.as_str().unwrap(), user.as_str().unwrap());
        let add = patch(json!([{"op": "add", "path": "members", "value": [{"value": user}]}]));
        let (at, gone) = (format!("/Groups/{group}"), format!("/Users/{user}"));
        let (added, deleted) = std::thread::scope(|scope| {
            let adding = scope.spawn(|| scim(&console, "PATCH", &at, Some(&add)).status);
            let deleting = scope.spawn(|| scim(&console, "DELETE", &gone, None).status);
            (adding.join().unwrap(), deleting.join().unwrap())
        });
        assert!(
            matches!(added, 204 | 400) && deleted == 204,
            "{added} {deleted}"
        );
        let held = expect(&console, "GET", &format!("/Groups/{group}"), None, 200);
        assert_eq!(members(&held), Vec::<&str>::new(), "round {round}");
    }
    // A value Postgres cannot store is the provider's error, not an outage.
    let nul = json!({"userName": "nul\u{0}@contoso.example"});
    let reply = scim(&console, "POST", "/Users", Some(&nul));
    assert_eq!(
        scim_type(&reply),
        (400, "invalidValue".into()),
        "{}",
        reply.body
    );

    let stderr = console.stderr();
    for line in lines(&stderr) {
        assert!(line.starts_with("scim: "), "{line}");
    }
    let values = [
        "alice@contoso.example",
        "ALICE@contoso.example",
        "ext-alice-91",
        "Platform Team",
        "g-plat-5",
        "x-1",
        "h@contoso.example",
        "Changed",
    ];
    for error in &errors {
        assert_leaks_nothing(&audit, error, &values);
    }
    assert_leaks_nothing(&audit, &stderr, &values);
}

/// The revocation marker: deactivating leaves none; deleting a provisioned user, inactive or
/// active, leaves one that `GET` and `filter` do not show; deleting an unknown id leaves none;
/// provisioning the user again inactive, or active only by default, keeps it, and naming it
/// active (its `userName` compared ignoring case, the default `user_attribute`) clears it.
#[test]
fn a_deleted_user_leaves_a_revocation_marker_until_provisioned_active() {
    let Some((_issuer, audit, console)) = provisioned("ui_directory_revoked") else {
        return;
    };
    let markers = || {
        rows(
            &audit,
            "SELECT user_name FROM {s}.directory_revoked ORDER BY 1",
        )
    };
    let dave = json!({"userName": "dave@contoso.example", "externalId": "ext-dave"});
    let dave = expect(&console, "POST", "/Users", Some(&dave), 201)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let off = patch(json!([{"op": "replace", "path": "active", "value": false}]));
    expect(
        &console,
        "PATCH",
        &format!("/Users/{dave}"),
        Some(&off),
        200,
    );
    assert_eq!(markers(), Vec::<String>::new());
    expect(&console, "DELETE", &format!("/Users/{dave}"), None, 204);
    let erin = json!({"userName": "erin@contoso.example", "active": true});
    let erin = expect(&console, "POST", "/Users", Some(&erin), 201)["id"]
        .as_str()
        .unwrap()
        .to_string();
    expect(&console, "DELETE", &format!("/Users/{erin}"), None, 204);
    expect(
        &console,
        "DELETE",
        &format!("/Users/{}", "a".repeat(32)),
        None,
        404,
    );
    assert_eq!(markers(), ["dave@contoso.example", "erin@contoso.example"]);
    let lookup = format!(
        "/Users?filter={}",
        encoded("userName eq \"dave@contoso.example\"")
    );
    assert_eq!(
        expect(&console, "GET", &lookup, None, 200)["totalResults"],
        json!(0)
    );

    let again = json!({"userName": "DAVE@contoso.example", "active": "False"});
    let again = expect(&console, "POST", "/Users", Some(&again), 201)["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(markers().len(), 2);
    let on = patch(json!([{"op": "replace", "path": "active", "value": "True"}]));
    expect(
        &console,
        "PATCH",
        &format!("/Users/{again}"),
        Some(&on),
        200,
    );
    assert_eq!(markers(), ["erin@contoso.example"]);
    // Active by default is not explicit: the marker stays until `active` is named true.
    let erin = json!({"userName": "erin@contoso.example"});
    let erin = expect(&console, "POST", "/Users", Some(&erin), 201)["id"].clone();
    assert_eq!(markers(), ["erin@contoso.example"]);
    let erin = format!("/Users/{}", erin.as_str().unwrap());
    let on = patch(json!([{"op": "add", "value": {"active": true}}]));
    expect(&console, "PATCH", &erin, Some(&on), 200);
    assert_eq!(markers(), Vec::<String>::new());
}

/// A console whose `[postgres]` names a closed port, so no service is needed: `PG_334_URL`.
/// `directory` its `[directory]`.
fn unreachable_console(case: &str, issuer: &Issuer, directory: &str) -> PathBuf {
    let dir = console_dir(case, Some(VERSION), &head(&issuer.base), &ui_oidc());
    let postgres = "\n[postgres]\nurl_env = \"PG_334_URL\"\nschema = \"meada_t334\"\n";
    append(&dir, &format!("{postgres}{directory}"));
    dir
}

const CLOSED: &str = "postgres://meada:pg-secret-334@127.0.0.1:1/meada";

/// Admission: no token, a console token, a wrong token and the token one character short each
/// get a 401 with `WWW-Authenticate: Bearer`; any request carrying `Origin` gets a 403 before the
/// token is read; the SCIM token under `/api/v1/session` is a 401; a body of another media type
/// is a 415. The discovery documents answer with their fixed content; with Postgres unreachable
/// a resource is a 503 and the other routes answer. The manifest route names the token's
/// variable, never the token.
#[test]
fn admission_is_the_scim_token_alone() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = unreachable_console("ui_directory_admission", &issuer, DIRECTORY);
    let console = start(&dir, &[("MEADA_SCIM_TOKEN", TOKEN), ("PG_334_URL", CLOSED)]);
    let console_token = sign(&signing, "k1", Algorithm::ES256, &claims(&issuer.base, &[]));
    let short = &TOKEN[..TOKEN.len() - 1];
    for authorization in [
        None,
        Some(bearer(&console_token)),
        Some(bearer("scim-334-token-0123456789abcdefghijklmnX")),
        Some(bearer(short)),
        Some(format!("Basic {TOKEN}")),
    ] {
        let headers: Vec<(&str, &str)> = (authorization.iter())
            .map(|a| ("Authorization", a.as_str()))
            .collect();
        let reply = send_body(&console, "GET", "/scim/v2/Users", &headers, "");
        assert_eq!(reply.status, 401, "{authorization:?}");
        assert_eq!(reply.header("www-authenticate"), Some("Bearer"));
        assert_eq!(reply.json()["schemas"], json!([ERROR]));
        assert_eq!(reply.json()["status"], json!("401"));
    }
    let scim_token = bearer(TOKEN);
    for headers in [
        vec![("Origin", PUBLIC), ("Authorization", scim_token.as_str())],
        vec![("Origin", "https://evil.example")],
    ] {
        let reply = send_body(&console, "GET", "/scim/v2/Users", &headers, "");
        assert_eq!(reply.status, 403, "{headers:?}");
        assert_eq!(reply.json()["schemas"], json!([ERROR]));
    }
    let foreign = [
        ("Host", "evil.example"),
        ("Authorization", scim_token.as_str()),
    ];
    assert_eq!(
        send_body(&console, "GET", "/scim/v2/Users", &foreign, "").status,
        403
    );
    let session = ask(&console, "/api/v1/session", TOKEN);
    assert_eq!(session.status, 401);
    let typed = [
        ("Authorization", scim_token.as_str()),
        ("Content-Type", "text/plain"),
    ];
    let reply = send_body(
        &console,
        "POST",
        "/scim/v2/Users",
        &typed,
        "{\"userName\": \"x\"}",
    );
    assert_eq!(reply.status, 415);

    let config = expect(&console, "GET", "/ServiceProviderConfig", None, 200);
    assert_eq!(
        config,
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
            "patch": {"supported": true},
            "bulk": {"supported": false, "maxOperations": 0, "maxPayloadSize": 0},
            "filter": {"supported": true, "maxResults": 100},
            "changePassword": {"supported": false},
            "sort": {"supported": false},
            "etag": {"supported": false},
            "authenticationSchemes": [{
                "type": "oauthbearertoken",
                "name": "OAuth Bearer Token",
                "description": "The directory's own bearer token",
                "primary": true,
            }],
            "meta": {
                "resourceType": "ServiceProviderConfig",
                "location": format!("{PUBLIC}/scim/v2/ServiceProviderConfig"),
            },
        })
    );
    let types = expect(&console, "GET", "/ResourceTypes", None, 200);
    let named: Vec<(&Value, &Value, &Value)> = (types["Resources"].as_array().unwrap().iter())
        .map(|t| (&t["id"], &t["endpoint"], &t["schema"]))
        .collect();
    assert_eq!(
        named,
        [
            (&json!("User"), &json!("/Users"), &json!(USER)),
            (&json!("Group"), &json!("/Groups"), &json!(GROUP))
        ]
    );
    let schemas = expect(&console, "GET", "/Schemas", None, 200);
    let attributes = |at: usize| -> Vec<Value> {
        let listed = schemas["Resources"][at]["attributes"].as_array().unwrap();
        listed.iter().map(|a| a["name"].clone()).collect()
    };
    assert_eq!(schemas["totalResults"], json!(2));
    let unique = |at: usize, attribute: usize| &schemas["Resources"][at]["attributes"][attribute];
    assert_eq!(unique(1, 0)["uniqueness"], json!("server"));
    assert_eq!(unique(1, 1)["uniqueness"], json!("none"));
    assert_eq!(unique(0, 0)["caseExact"], json!(false));
    assert_eq!(
        (attributes(0), attributes(1)),
        (
            vec![json!("userName"), json!("externalId"), json!("active")],
            vec![json!("displayName"), json!("externalId"), json!("members")]
        )
    );
    assert_eq!(scim(&console, "DELETE", "/Schemas", None).status, 405);

    let down = scim(&console, "GET", "/Users", None);
    assert_eq!(
        (down.status, &down.json()["schemas"]),
        (503, &json!([ERROR]))
    );
    assert_eq!(super::get(&console, "/api/v1/config").status, 200);
    let manifest = ask(&console, "/api/v1/manifest", &console_token);
    assert_eq!(manifest.status, 200, "{}", manifest.body);
    assert_eq!(
        manifest.json()["manifest"]["directory"]["token_env"],
        json!("MEADA_SCIM_TOKEN")
    );
    assert!(!manifest.body.contains(TOKEN));

    let stderr = console.stderr();
    assert_eq!(
        lines(&stderr),
        ["scim: POST Users -: 415", "scim: DELETE - -: 405"]
    );
    for leak in [TOKEN, CLOSED, "pg-secret-334"] {
        assert!(!stderr.contains(leak), "{leak} in {stderr}");
    }
}

/// The console refuses to start with a SCIM token of 31 characters or with none, naming the key
/// and its line, never the value; in English under `pt_BR` too, as every start refusal of the
/// console is (ADR 0008).
#[test]
fn the_console_refuses_a_short_or_missing_token() {
    let (_, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = unreachable_console(
        "ui_directory_token",
        &issuer,
        &DIRECTORY.replace("user_claim", "token_env = \"MEADA_SCIM_TOKEN\"\nuser_claim"),
    );
    let line = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let line = 1 + line
        .lines()
        .position(|l| l.starts_with("token_env"))
        .unwrap();
    let short = &TOKEN[..31];
    let at = format!("{MANIFEST}:{line}: `directory.token_env`:");
    for (token, lang, expected) in [
        (
            Some(short),
            "en_US.UTF-8",
            format!("{at} holds a token shorter than 32 characters"),
        ),
        (
            Some(short),
            "pt_BR.UTF-8",
            format!("{at} holds a token shorter than 32 characters"),
        ),
        (
            None,
            "en_US.UTF-8",
            format!("{at} the variable MEADA_SCIM_TOKEN is not set"),
        ),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_meada"));
        command
            .current_dir(&dir)
            .args(["ui", "--listen", "127.0.0.1:0", "--workspace", MANIFEST])
            .env("LANG", lang)
            .env("PG_334_URL", CLOSED)
            .env_remove("MEADA_SCIM_TOKEN");
        if let Some(token) = token {
            command.env("MEADA_SCIM_TOKEN", token);
        }
        let out = command.output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{stderr}");
        assert_eq!(stderr.trim_end(), format!("meada: {expected}"), "{lang}");
        assert!(!stderr.contains(short), "{stderr}");
    }
}
