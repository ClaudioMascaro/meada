//! The directory's SCIM 2.0 endpoint (RFC 7643, RFC 7644; #334): with `[ui]` and `[directory]`
//! the console serves `/scim/v2/` on its listener, the surface Entra ID's and Okta's
//! provisioning send and no more. The console admits a request (`Scim::admits`, after the `Host`
//! and `Origin` checks) and bounds its body at `SCIM_BODY_MAX`; `Scim::answer` routes it:
//!
//! - `/Users` and `/Groups`: `POST` (201), `GET` with `filter` (`userName eq`, `externalId eq`;
//!   `displayName eq`, `externalId eq`), `startIndex` and `count` (at most `PAGE_MAX`), and `GET`,
//!   `PUT`, `PATCH` and `DELETE` (204) of `/<kind>/<id>`. A user `PATCH` answers 200 with the
//!   user, a group `PATCH` 204; `excludedAttributes=members` leaves a group's members out.
//! - `/ServiceProviderConfig`, `/ResourceTypes`, `/Schemas`: fixed documents.
//!
//! Every write runs in one transaction under one advisory lock of the schema, so a uniqueness or
//! membership check and the write it guards cannot interleave with another write, and a `PATCH`
//! whose later operation fails writes nothing. A resource is stored as the JSON the provider sent,
//! without `id`, `meta` and `password` (Meada's own, and never kept), a group without `members`,
//! which live in their own table; at most `SCIM_RESOURCE_MAX` bytes after every change. Meada's
//! ids (32 lowercase hex characters, from Postgres) are checked wherever read. A deleted user
//! leaves every group and a revocation marker (its `userName` and `externalId`) that only a write
//! naming `active` true for a user with the same `user_attribute` clears. An error is RFC 7644's, its
//! detail fixed: none echoes a value. Postgres unreachable, or at a version this binary does not
//! know, is a 503 and one `ui:` line naming why.

use hyper::{Method, StatusCode};
use serde_json::{Map, Value, json};
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

use crate::msg::Msg;
use crate::postgres::Opened;

// ponytail: the most bytes of a request's body; past it a 413.
pub const SCIM_BODY_MAX: usize = 1 << 20;
// ponytail: the most bytes of a stored resource, as JSON, after every change; past it a 413.
pub const SCIM_RESOURCE_MAX: usize = 64 << 10;
// ponytail: the most resources a page holds, and how many without `count`.
const PAGE_MAX: i64 = 100;

const ERROR: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
const LIST: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
const USER: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
const GROUP: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";

/// The fixed details of the refusals the console makes before routing.
pub const ORIGIN_REFUSED: &str = "a request carrying Origin is refused";
pub const UNAUTHORIZED: &str = "the SCIM bearer token is missing or wrong";
pub const TOO_LARGE: &str = "the request body is too large";
pub const UNREADABLE: &str = "the request body could not be read";
pub const MEDIA_TYPE: &str = "the body must be application/scim+json or application/json";

/// A timestamp column as RFC 3339, UTC, to the microsecond.
macro_rules! time {
    ($column:literal) => {
        concat!(
            "to_char(",
            $column,
            " AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')"
        )
    };
}

/// What a read of a resource selects: `Stored::of` takes it.
const COLUMNS: &str = concat!(
    "id, resource::text, ",
    time!("created"),
    ", ",
    time!("last_modified")
);

/// Whether `path` is the endpoint's.
pub fn routed(path: &str) -> bool {
    path == "/scim/v2" || path.starts_with("/scim/v2/")
}

/// The resource a write's stderr line names: `Users`, `Groups` or `-`.
pub fn resource(path: &str) -> &'static str {
    let first = path.trim_start_matches("/scim/v2/").split('/').next();
    first.and_then(Kind::of).map_or("-", Kind::name)
}

/// The endpoint: the token's hash and where the directory lives.
pub struct Scim {
    /// The SCIM token's BLAKE3 hash: comparing hashes is constant-time and keeps no token.
    token: blake3::Hash,
    postgres: crate::postgres::Config,
    /// `<[ui] public_url>/scim/v2`.
    base: String,
    user_attribute: &'static str,
    group_attribute: &'static str,
}

/// An answer: its status, its document, and the id of the stored resource it names, if any.
pub struct Reply {
    pub status: StatusCode,
    pub body: Option<Value>,
    pub named: Option<String>,
    /// A created resource's location.
    pub location: Option<String>,
}

impl Reply {
    /// RFC 7644's error, `scim_type` set for a 400 or 409.
    pub fn error(status: StatusCode, scim_type: Option<&str>, detail: &str) -> Self {
        let mut body =
            json!({"schemas": [ERROR], "status": status.as_u16().to_string(), "detail": detail});
        if let Some(scim_type) = scim_type {
            body["scimType"] = scim_type.into();
        }
        Self {
            status,
            body: Some(body),
            named: None,
            location: None,
        }
    }

    fn ok(status: StatusCode, body: Option<Value>) -> Self {
        Self {
            status,
            body,
            named: None,
            location: None,
        }
    }
}

/// A refusal: its status, `scimType` and fixed detail.
struct Fail(StatusCode, Option<&'static str>, &'static str);

type Done<T> = Result<T, Fail>;

fn bad(scim_type: &'static str, detail: &'static str) -> Fail {
    Fail(StatusCode::BAD_REQUEST, Some(scim_type), detail)
}

fn not_found() -> Fail {
    Fail(StatusCode::NOT_FOUND, None, "no such resource")
}

fn too_large() -> Fail {
    Fail(
        StatusCode::PAYLOAD_TOO_LARGE,
        None,
        "the resource is too large",
    )
}

/// Postgres unreachable, or at a version this binary does not know: one stderr line naming why
/// (never the URL or its password), and a 503.
fn unavailable(reason: &str) -> Fail {
    eprintln!(
        "ui: {}",
        Msg::PostgresUnreachable(reason).text().escape_debug()
    );
    Fail(
        StatusCode::SERVICE_UNAVAILABLE,
        None,
        "the directory in Postgres is unavailable",
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Users,
    Groups,
}

impl Kind {
    fn of(segment: &str) -> Option<Self> {
        match segment {
            "Users" => Some(Kind::Users),
            "Groups" => Some(Kind::Groups),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Users => "Users",
            Kind::Groups => "Groups",
        }
    }

    fn resource_type(self) -> &'static str {
        match self {
            Kind::Users => "User",
            Kind::Groups => "Group",
        }
    }

    fn schema(self) -> &'static str {
        match self {
            Kind::Users => USER,
            Kind::Groups => GROUP,
        }
    }

    fn table(self) -> &'static str {
        match self {
            Kind::Users => "directory_users",
            Kind::Groups => "directory_groups",
        }
    }
}

/// Whether `id` is one of Meada's: 32 lowercase hex characters.
fn is_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A resource as stored.
struct Stored {
    id: String,
    resource: Map<String, Value>,
    created: String,
    modified: String,
}

impl Stored {
    /// A row of `COLUMNS`; `None` for one that fails its checks (every row is hostile).
    fn of(row: &Row) -> Option<Self> {
        let text = |i| row.try_get::<_, Option<String>>(i).ok().flatten();
        let id = text(0).filter(|id| is_id(id))?;
        let Ok(Value::Object(resource)) = serde_json::from_str(&text(1)?) else {
            return None;
        };
        Some(Self {
            id,
            resource,
            created: text(2)?,
            modified: text(3)?,
        })
    }
}

/// A resource about to be stored, checked.
struct Candidate {
    resource: Map<String, Value>,
    /// `userName` or `displayName`.
    name: String,
    external_id: Option<String>,
    active: bool,
    /// Whether the resource names `active` itself: only an explicit active user clears a
    /// revocation marker.
    active_named: bool,
    /// A group's members, sorted, without repeats.
    members: Vec<String>,
}

/// The key of `map` named `name` ignoring case, else `name`.
fn key_of(map: &Map<String, Value>, name: &str) -> String {
    let found = map.keys().find(|k| k.eq_ignore_ascii_case(name));
    found.cloned().unwrap_or_else(|| name.to_string())
}

/// The value of `map` named `name` ignoring case.
fn get<'a>(map: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

/// `value` as `active`: a boolean, or the string `True` or `False` ignoring case.
fn boolean(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::String(s) if s.eq_ignore_ascii_case("true") => Some(true),
        Value::String(s) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

/// `resource` checked for storage as a `kind`: `id`, `meta` and `password` dropped, `userName`
/// or `displayName` a non-empty string, `externalId` a string if any, `active` a boolean (true
/// when absent), a group's `members` objects naming an id each, taken out; at most
/// `SCIM_RESOURCE_MAX` bytes.
fn candidate(kind: Kind, mut resource: Map<String, Value>) -> Done<Candidate> {
    let invalid = |detail| bad("invalidValue", detail);
    resource.retain(|k, _| {
        !["id", "meta", "password"]
            .iter()
            .any(|n| k.eq_ignore_ascii_case(n))
    });
    let name = match kind {
        Kind::Users => "userName",
        Kind::Groups => "displayName",
    };
    let name = match get(&resource, name) {
        Some(Value::String(name)) if !name.is_empty() => name.clone(),
        _ => {
            return Err(invalid(
                "userName or displayName must be a non-empty string",
            ));
        }
    };
    let external_id = match get(&resource, "externalId") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => Some(id.clone()),
        Some(_) => return Err(invalid("externalId must be a string")),
    };
    let (mut active, mut active_named) = (true, false);
    let mut members = Vec::new();
    match kind {
        Kind::Users => {
            let key = key_of(&resource, "active");
            match resource.get(&key) {
                None | Some(Value::Null) => {}
                Some(value) => {
                    active = boolean(value).ok_or(invalid("active must be a boolean"))?;
                    active_named = true;
                    resource.insert(key, active.into());
                }
            }
        }
        Kind::Groups => {
            let key = key_of(&resource, "members");
            let listed = match resource.remove(&key) {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(listed)) => listed,
                Some(_) => return Err(invalid("members must be a list")),
            };
            for member in &listed {
                let value = member.as_object().and_then(|m| get(m, "value"));
                match value.and_then(Value::as_str) {
                    Some(id) => members.push(id.to_string()),
                    None => return Err(invalid("a member must name a user by value")),
                }
            }
            members.sort();
            members.dedup();
        }
    }
    if serde_json::to_vec(&resource).map_or(usize::MAX, |b| b.len()) > SCIM_RESOURCE_MAX {
        return Err(too_large());
    }
    Ok(Candidate {
        resource,
        name,
        external_id,
        active,
        active_named,
        members,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Add,
    Remove,
    Replace,
}

/// A step of an attribute path: an attribute, and for a multi-valued one the `eq` filter
/// selecting its values (`emails[type eq "work"]`).
#[derive(Debug, PartialEq)]
struct Step {
    name: String,
    filter: Option<(String, Value)>,
}

/// One `PATCH` operation.
struct Operation {
    op: Op,
    path: Option<Vec<Step>>,
    value: Option<Value>,
}

/// Whether `name` is an attribute name (RFC 7643's `ATTRNAME`, or `$ref`).
fn attribute(name: &str) -> bool {
    name == "$ref"
        || (name.starts_with(|c: char| c.is_ascii_alphabetic())
            && (name.bytes()).all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
}

/// An attribute path: an optional schema URN (the core schemas' dropped), an attribute, an
/// optional `[<sub-attribute> eq <value>]` filter and an optional `.<sub-attribute>`. Anything
/// else is `invalidPath`.
fn path(text: &str) -> Done<Vec<Step>> {
    let invalid = || bad("invalidPath", "the attribute path is not supported");
    let head = text.find('[').unwrap_or(text.len());
    let urn = (text.get(..4)).is_some_and(|p| p.eq_ignore_ascii_case("urn:"));
    let (schema, rest) = match text[..head].rfind(':') {
        Some(at) if urn && at > 4 => (Some(&text[..at]), &text[at + 1..]),
        _ if urn => return Err(invalid()),
        Some(_) => return Err(invalid()),
        None => (None, text),
    };
    let mut steps = Vec::new();
    if let Some(schema) = schema
        && ![USER, GROUP].iter().any(|s| s.eq_ignore_ascii_case(schema))
    {
        steps.push(Step {
            name: schema.to_string(),
            filter: None,
        });
    }
    let end = rest.find(['[', '.']).unwrap_or(rest.len());
    let (name, mut rest) = rest.split_at(end);
    if !attribute(name) {
        return Err(invalid());
    }
    let mut filter = None;
    if let Some(inside) = rest.strip_prefix('[') {
        let close = inside.rfind(']').ok_or_else(invalid)?;
        let mut parts = inside[..close].trim().splitn(3, ' ');
        let (Some(sub), Some(op), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(invalid());
        };
        let value: Value = serde_json::from_str(value).map_err(|_| invalid())?;
        let scalar = matches!(value, Value::String(_) | Value::Bool(_) | Value::Number(_));
        if !attribute(sub) || !op.eq_ignore_ascii_case("eq") || !scalar {
            return Err(invalid());
        }
        filter = Some((sub.to_string(), value));
        rest = &inside[close + 1..];
    }
    steps.push(Step {
        name: name.to_string(),
        filter,
    });
    if let Some(sub) = rest.strip_prefix('.') {
        if !attribute(sub) {
            return Err(invalid());
        }
        steps.push(Step {
            name: sub.to_string(),
            filter: None,
        });
    } else if !rest.is_empty() {
        return Err(invalid());
    }
    Ok(steps)
}

/// A `PATCH` body's operations: `Operations`, each with `op` (`add`, `remove` or `replace`,
/// ignoring case), an optional `path` and the `value` an `add` or `replace` needs.
fn operations(body: &Value) -> Done<Vec<Operation>> {
    let syntax = || bad("invalidSyntax", "the body is not a SCIM PatchOp");
    let body = body.as_object().ok_or_else(syntax)?;
    let listed = get(body, "Operations").and_then(Value::as_array);
    let listed = listed.filter(|l| !l.is_empty()).ok_or_else(syntax)?;
    let mut operations = Vec::new();
    for operation in listed {
        let operation = operation.as_object().ok_or_else(syntax)?;
        let op = match get(operation, "op").and_then(Value::as_str) {
            Some(op) if op.eq_ignore_ascii_case("add") => Op::Add,
            Some(op) if op.eq_ignore_ascii_case("remove") => Op::Remove,
            Some(op) if op.eq_ignore_ascii_case("replace") => Op::Replace,
            _ => return Err(bad("invalidSyntax", "the operation is not supported")),
        };
        let path = match get(operation, "path") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(path(text)?),
            Some(_) => return Err(bad("invalidPath", "the attribute path is not supported")),
        };
        let value = get(operation, "value").filter(|v| !v.is_null()).cloned();
        match (op, &path, &value) {
            (Op::Remove, None, _) => {
                return Err(bad("noTarget", "a remove operation needs a path"));
            }
            (Op::Add | Op::Replace, _, None) => {
                return Err(bad("invalidValue", "the operation needs a value"));
            }
            (Op::Add | Op::Replace, None, Some(value)) if !value.is_object() => {
                return Err(bad(
                    "invalidValue",
                    "an operation without a path needs an object",
                ));
            }
            _ => {}
        }
        operations.push(Operation { op, path, value });
    }
    Ok(operations)
}

/// What tells a value of a multi-valued attribute from the others: its `value` when it is an
/// object that has one, else itself, as JSON text.
fn identity(v: &Value) -> String {
    let value = v.as_object().and_then(|o| get(o, "value"));
    value.unwrap_or(v).to_string()
}

/// Whether `element`, a value of a multi-valued attribute, has `sub` equal to `expected`
/// (strings ignoring case).
fn matches(element: &Value, (sub, expected): &(String, Value)) -> bool {
    let found = element.as_object().and_then(|o| get(o, sub));
    match (found, expected) {
        (Some(Value::String(a)), Value::String(b)) => a.eq_ignore_ascii_case(b),
        (Some(found), expected) => found == expected,
        (None, _) => false,
    }
}

/// `new`'s attributes set in `into`, names compared ignoring case.
fn merge(into: &mut Map<String, Value>, new: &Map<String, Value>) {
    for (name, value) in new {
        into.insert(key_of(into, name), value.clone());
    }
}

/// `operation` applied to `resource` (RFC 7644, section 3.5.2).
fn apply(resource: &mut Map<String, Value>, operation: &Operation) -> Done<()> {
    let value = operation.value.as_ref();
    match &operation.path {
        Some(steps) => at(resource, operation.op, steps, value),
        // Without a path the value's attributes are the targets: a URN names an extension's
        // attributes, anything else is a path.
        None => {
            let Some(Value::Object(attributes)) = value else {
                return Ok(());
            };
            for (name, value) in attributes {
                let steps = match (name.get(..4)).is_some_and(|p| p.eq_ignore_ascii_case("urn:")) {
                    true => vec![Step {
                        name: name.clone(),
                        filter: None,
                    }],
                    false => path(name)?,
                };
                at(resource, operation.op, &steps, Some(value))?;
            }
            Ok(())
        }
    }
}

/// `op` with `value` at `steps` under `target`.
fn at(target: &mut Map<String, Value>, op: Op, steps: &[Step], value: Option<&Value>) -> Done<()> {
    let invalid = || bad("invalidPath", "the attribute path is not supported");
    let no_target = || bad("noTarget", "the filter matches no value");
    let (step, rest) = steps.split_first().ok_or_else(invalid)?;
    let key = key_of(target, &step.name);
    let Some(filter) = &step.filter else {
        if rest.is_empty() {
            leaf(target, key, op, value);
            return Ok(());
        }
        return match target.get_mut(&key) {
            Some(Value::Object(inner)) => at(inner, op, rest, value),
            None | Some(Value::Null) if op == Op::Remove => Ok(()),
            None | Some(Value::Null) => {
                let mut inner = Map::new();
                at(&mut inner, op, rest, value)?;
                target.insert(key, Value::Object(inner));
                Ok(())
            }
            Some(_) => Err(invalid()),
        };
    };
    if matches!(target.get(&key), None | Some(Value::Null)) {
        match op {
            Op::Remove => return Ok(()),
            Op::Replace => return Err(no_target()),
            Op::Add => {
                target.insert(key.clone(), Value::Array(Vec::new()));
            }
        }
    }
    let Some(Value::Array(values)) = target.get_mut(&key) else {
        return Err(invalid());
    };
    if op == Op::Remove && rest.is_empty() {
        values.retain(|v| !matches(v, filter));
        return Ok(());
    }
    let mut found = false;
    for element in values.iter_mut().filter(|v| matches(v, filter)) {
        found = true;
        let Value::Object(element) = element else {
            continue;
        };
        match (rest.is_empty(), value) {
            (true, Some(Value::Object(new))) if op == Op::Add => merge(element, new),
            (true, Some(Value::Object(new))) => *element = new.clone(),
            (true, _) => return Err(bad("invalidValue", "a filtered value needs an object")),
            (false, _) => at(element, op, rest, value)?,
        }
    }
    match (found, op) {
        (true, _) | (false, Op::Remove) => Ok(()),
        (false, Op::Replace) => Err(no_target()),
        // An `add` through a filter that matches nothing adds the value it describes.
        (false, Op::Add) => {
            let mut element = Map::new();
            element.insert(filter.0.clone(), filter.1.clone());
            match (rest.is_empty(), value) {
                (true, Some(Value::Object(new))) => merge(&mut element, new),
                (true, _) => return Err(bad("invalidValue", "a filtered value needs an object")),
                (false, _) => at(&mut element, op, rest, value)?,
            }
            values.push(Value::Object(element));
            Ok(())
        }
    }
}

/// `op` with `value` on the attribute `key` of `target`: an `add` appends to a multi-valued
/// attribute (skipping values already there) and merges into a complex one; a `replace` merges
/// into a complex one and sets anything else; a `remove` with a list removes those values from a
/// multi-valued attribute, and otherwise the attribute.
fn leaf(target: &mut Map<String, Value>, key: String, op: Op, value: Option<&Value>) {
    match (op, target.get_mut(&key), value) {
        (Op::Remove, Some(Value::Array(values)), Some(Value::Array(removed))) => {
            let removed: std::collections::HashSet<String> = removed.iter().map(identity).collect();
            values.retain(|v| !removed.contains(&identity(v)));
        }
        (Op::Remove, _, _) => {
            target.remove(&key);
        }
        (Op::Add, Some(Value::Array(values)), Some(added)) => {
            let added = match added {
                Value::Array(added) => added.clone(),
                one => vec![one.clone()],
            };
            let mut held: std::collections::HashSet<String> = values.iter().map(identity).collect();
            for value in added {
                if held.insert(identity(&value)) {
                    values.push(value);
                }
            }
        }
        (Op::Add | Op::Replace, Some(Value::Object(inner)), Some(Value::Object(new))) => {
            merge(inner, new);
        }
        (_, _, Some(value)) => {
            target.insert(key, value.clone());
        }
        (_, _, None) => {}
    }
}

/// The decoded parameters of a query, form-encoded (`+` a space).
fn parameters(query: Option<&str>) -> Vec<(String, String)> {
    let decode = |s: &str| {
        let spaced = s.replace('+', " ");
        percent_encoding::percent_decode_str(&spaced)
            .decode_utf8_lossy()
            .into_owned()
    };
    (query.into_iter().flat_map(|q| q.split('&')))
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(name), decode(value))
        })
        .collect()
}

/// A list's `filter`: `<attribute> eq "<value>"`, the attribute one `kind` supports (names
/// ignoring case); the SQL condition on `$1` and the value. Anything else is `invalidFilter`.
fn filter(kind: Kind, text: &str) -> Done<(&'static str, String)> {
    let invalid = || bad("invalidFilter", "the filter is not supported");
    let mut parts = text.trim().splitn(3, ' ');
    let (Some(name), Some(op), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(invalid());
    };
    if !op.eq_ignore_ascii_case("eq") {
        return Err(invalid());
    }
    let Ok(Value::String(value)) = serde_json::from_str(value.trim()) else {
        return Err(invalid());
    };
    let condition = match (kind, name) {
        (Kind::Users, n) if n.eq_ignore_ascii_case("userName") => "lower(user_name) = lower($1)",
        (Kind::Groups, n) if n.eq_ignore_ascii_case("displayName") => "display_name = $1",
        (_, n) if n.eq_ignore_ascii_case("externalId") => "external_id = $1",
        _ => return Err(invalid()),
    };
    Ok((condition, value))
}

/// A connection to the directory's schema, with `{s}` in a statement standing for it.
struct Db {
    opened: Opened,
    schema: String,
}

impl Db {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Done<Vec<Row>> {
        let sql = sql.replace("{s}", &self.schema);
        let rows = self.opened.client.query(&sql, params).await;
        rows.map_err(|e| self.failed(e))
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Done<u64> {
        let sql = sql.replace("{s}", &self.schema);
        let done = self.opened.client.execute(&sql, params).await;
        done.map_err(|e| self.failed(e))
    }

    /// A statement's error: a value Postgres cannot store (SQLSTATE class 22, as a NUL in a
    /// string) is the provider's `invalidValue`; anything else is Postgres unavailable.
    fn failed(&self, e: tokio_postgres::Error) -> Fail {
        match e.code().is_some_and(|code| code.code().starts_with("22")) {
            true => bad("invalidValue", "a value cannot be stored"),
            false => unavailable(&self.opened.scrub(e)),
        }
    }

    /// The resource `id` of `kind`, if stored and sound.
    async fn load(&self, kind: Kind, id: &str) -> Done<Option<Stored>> {
        let sql = format!("SELECT {COLUMNS} FROM {{s}}.{} WHERE id = $1", kind.table());
        let rows = self.query(&sql, &[&id]).await?;
        Ok(rows.first().and_then(Stored::of))
    }

    /// The members of each group of `ids`, by group, sorted.
    async fn members(&self, ids: &[String]) -> Done<Vec<(String, String)>> {
        let sql = "SELECT group_id, user_id FROM {s}.directory_members WHERE group_id = ANY($1) \
                   ORDER BY user_id";
        let rows = self.query(sql, &[&ids]).await?;
        let pairs = rows.iter().filter_map(|row| {
            let (group, user) = (row.try_get(0).ok()?, row.try_get::<_, String>(1).ok()?);
            is_id(&user).then_some((group, user))
        });
        Ok(pairs.collect())
    }

    /// Records the write's time in the directory's state (#335) and commits.
    async fn commit(&self) -> Done<()> {
        let state = "UPDATE {s}.directory_state SET last_write = now()";
        self.execute(state, &[]).await?;
        self.execute("COMMIT", &[]).await.map(|_| ())
    }
}

impl Scim {
    pub fn new(
        config: &super::Config,
        token: &str,
        postgres: crate::postgres::Config,
        public_url: &str,
    ) -> Self {
        Self {
            token: blake3::hash(token.as_bytes()),
            postgres,
            base: format!("{public_url}/scim/v2"),
            user_attribute: config.user_attribute,
            group_attribute: config.group_attribute,
        }
    }

    /// Whether `authorization`, a request's header, is `Bearer <the SCIM token>`, compared in
    /// constant time.
    pub fn admits(&self, authorization: Option<&str>) -> bool {
        let presented = authorization.and_then(|a| {
            let (scheme, token) = a.split_once(' ')?;
            scheme.eq_ignore_ascii_case("bearer").then_some(token)
        });
        blake3::hash(presented.unwrap_or("").as_bytes()) == self.token
    }

    /// The answer to an admitted request: `method` on `path` (under `/scim/v2`) with `query`
    /// and `body`, at most `SCIM_BODY_MAX` bytes.
    pub async fn answer(
        &self,
        method: &Method,
        path: &str,
        query: Option<&str>,
        body: &[u8],
    ) -> Reply {
        let mut named = None;
        let path = path.strip_prefix("/scim/v2").unwrap_or(path);
        let steps: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let parsed = || match serde_json::from_slice::<Value>(body) {
            Ok(Value::Object(body)) => Ok(body),
            _ => Err(bad("invalidSyntax", "the body is not a JSON object")),
        };
        let not_allowed = || {
            Fail(
                StatusCode::METHOD_NOT_ALLOWED,
                None,
                "the method is not supported",
            )
        };
        let kind = Kind::of(steps[0]);
        let answered = match (steps.as_slice(), kind, method) {
            (["ServiceProviderConfig"], _, &Method::GET) => Ok(self.provider_config()),
            (["ResourceTypes"], _, &Method::GET) => Ok(self.resource_types()),
            (["Schemas"], _, &Method::GET) => Ok(schemas(self.group_attribute)),
            (["ServiceProviderConfig" | "ResourceTypes" | "Schemas"], _, _) => Err(not_allowed()),
            ([_], Some(kind), &Method::GET) => self.list(kind, query).await,
            ([_], Some(kind), &Method::POST) => match parsed() {
                Ok(body) => self.create(kind, body, &mut named).await,
                Err(fail) => Err(fail),
            },
            ([_], Some(_), _) => Err(not_allowed()),
            ([_, id], Some(kind), method) => {
                let id = Some(*id).filter(|id| is_id(id));
                match (method, id) {
                    (&Method::GET | &Method::PUT | &Method::PATCH | &Method::DELETE, None) => {
                        Err(not_found())
                    }
                    (&Method::GET, Some(id)) => self.get(kind, id, query).await,
                    (&Method::PUT, Some(id)) => match parsed() {
                        Ok(body) => self.replace(kind, id, body, &mut named).await,
                        Err(fail) => Err(fail),
                    },
                    (&Method::PATCH, Some(id)) => match parsed() {
                        Ok(body) => self.patch(kind, id, body, &mut named).await,
                        Err(fail) => Err(fail),
                    },
                    (&Method::DELETE, Some(id)) => self.delete(kind, id, &mut named).await,
                    _ => Err(not_allowed()),
                }
            }
            _ => Err(not_found()),
        };
        let mut reply = answered.unwrap_or_else(|Fail(status, scim_type, detail)| {
            Reply::error(status, scim_type, detail)
        });
        reply.named = named;
        reply
    }

    fn location(&self, kind: Kind, id: &str) -> String {
        format!("{}/{}/{id}", self.base, kind.name())
    }

    /// A connection whose schema holds this binary's version.
    async fn connect(&self) -> Done<Db> {
        let opened = crate::postgres::connected(&self.postgres).await;
        let opened = opened.map_err(|reason| unavailable(&reason))?;
        let schema = self.postgres.quoted();
        Ok(Db { opened, schema })
    }

    /// `connect`, in a transaction holding the directory's advisory lock until it ends: a
    /// connection dropped without `commit` rolls it back.
    async fn begin(&self) -> Done<Db> {
        let db = self.connect().await?;
        // ponytail: one lock for every directory write, so they run one at a time; per-resource
        // locks if a provider's throughput needs them.
        db.execute("BEGIN", &[]).await?;
        let hash = blake3::hash(format!("meada directory {}", db.schema).as_bytes());
        let key = i64::from_le_bytes(hash.as_bytes()[..8].try_into().expect("eight bytes"));
        db.execute("SELECT pg_advisory_xact_lock($1)", &[&key])
            .await?;
        Ok(db)
    }

    /// The answer's document of `stored`, a `kind`, with `members` for a group that shows them.
    fn document(&self, kind: Kind, stored: &Stored, members: Option<&[String]>) -> Value {
        let mut document = stored.resource.clone();
        if get(&document, "schemas").is_none() {
            document.insert("schemas".into(), json!([kind.schema()]));
        }
        document.insert("id".into(), stored.id.clone().into());
        let meta = json!({
            "resourceType": kind.resource_type(),
            "created": stored.created,
            "lastModified": stored.modified,
            "location": self.location(kind, &stored.id),
        });
        document.insert("meta".into(), meta);
        if let Some(members) = members {
            let members: Vec<Value> = (members.iter()).map(|id| json!({"value": id})).collect();
            document.insert("members".into(), members.into());
        }
        Value::Object(document)
    }

    /// `GET /<kind>`: the resources `filter` selects (all without it), from `startIndex` (1 by
    /// default) at most `count` (`PAGE_MAX` by default and at most).
    async fn list(&self, kind: Kind, query: Option<&str>) -> Done<Reply> {
        let (mut selected, mut start, mut count, mut members) = (None, 1, PAGE_MAX, true);
        for (name, value) in parameters(query) {
            let number = || {
                let n = value.trim().parse::<i64>();
                n.map_err(|_| bad("invalidValue", "startIndex and count must be integers"))
            };
            match name.as_str() {
                "filter" => selected = Some(filter(kind, &value)?),
                "startIndex" => start = number()?.max(1),
                "count" => count = number()?.clamp(0, PAGE_MAX),
                "excludedAttributes" => members = !excludes_members(&value),
                _ => {}
            }
        }
        let (condition, value) = match &selected {
            Some((condition, value)) => (*condition, Some(value.as_str())),
            None => ("$1::text IS NULL", None),
        };
        let table = kind.table();
        let db = self.connect().await?;
        let total = format!("SELECT count(*) FROM {{s}}.{table} WHERE {condition}");
        let total: i64 = db.query(&total, &[&value]).await?[0].get(0);
        let page = format!(
            "SELECT {COLUMNS} FROM {{s}}.{table} WHERE {condition} ORDER BY created, id \
             OFFSET $2 LIMIT $3"
        );
        let rows = db.query(&page, &[&value, &(start - 1), &count]).await?;
        let stored: Vec<Stored> = rows.iter().filter_map(Stored::of).collect();
        let held = match kind == Kind::Groups && members {
            true => {
                let ids: Vec<String> = stored.iter().map(|s| s.id.clone()).collect();
                Some(db.members(&ids).await?)
            }
            false => None,
        };
        let resources: Vec<Value> = (stored.iter())
            .map(|s| {
                let of = held.as_ref().map(|held| {
                    let mine = held.iter().filter(|(group, _)| *group == s.id);
                    mine.map(|(_, user)| user.clone()).collect::<Vec<_>>()
                });
                self.document(kind, s, of.as_deref())
            })
            .collect();
        Ok(Reply::ok(
            StatusCode::OK,
            Some(json!({
                "schemas": [LIST],
                "totalResults": total,
                "startIndex": start,
                "itemsPerPage": resources.len(),
                "Resources": resources,
            })),
        ))
    }

    /// `GET /<kind>/<id>`.
    async fn get(&self, kind: Kind, id: &str, query: Option<&str>) -> Done<Reply> {
        let db = self.connect().await?;
        let stored = db.load(kind, id).await?.ok_or_else(not_found)?;
        let excluded = (parameters(query).iter())
            .any(|(name, value)| name == "excludedAttributes" && excludes_members(value));
        let members = match kind == Kind::Groups && !excluded {
            true => Some(self.member_ids(&db, id).await?),
            false => None,
        };
        let document = self.document(kind, &stored, members.as_deref());
        Ok(Reply::ok(StatusCode::OK, Some(document)))
    }

    async fn member_ids(&self, db: &Db, id: &str) -> Done<Vec<String>> {
        let held = db.members(&[id.to_string()]).await?;
        Ok(held.into_iter().map(|(_, user)| user).collect())
    }

    /// `POST /<kind>`: 201 with the resource and its location.
    async fn create(
        &self,
        kind: Kind,
        body: Map<String, Value>,
        named: &mut Option<String>,
    ) -> Done<Reply> {
        let candidate = candidate(kind, body)?;
        let db = self.begin().await?;
        let (stored, members) = self.store(&db, kind, None, candidate).await?;
        db.commit().await?;
        *named = Some(stored.id.clone());
        let location = self.location(kind, &stored.id);
        let document = self.document(kind, &stored, members.as_deref());
        let mut reply = Reply::ok(StatusCode::CREATED, Some(document));
        reply.location = Some(location);
        Ok(reply)
    }

    /// `PUT /<kind>/<id>`: the whole resource replaced, 200 with it.
    async fn replace(
        &self,
        kind: Kind,
        id: &str,
        body: Map<String, Value>,
        named: &mut Option<String>,
    ) -> Done<Reply> {
        let db = self.begin().await?;
        if db.load(kind, id).await?.is_none() {
            return Err(not_found());
        }
        *named = Some(id.to_string());
        let candidate = candidate(kind, body)?;
        let (stored, members) = self.store(&db, kind, Some(id), candidate).await?;
        db.commit().await?;
        let document = self.document(kind, &stored, members.as_deref());
        Ok(Reply::ok(StatusCode::OK, Some(document)))
    }

    /// `PATCH /<kind>/<id>`: every operation applied in order, then stored as one write; one that
    /// fails writes nothing. A user answers 200 with itself, a group 204.
    async fn patch(
        &self,
        kind: Kind,
        id: &str,
        body: Map<String, Value>,
        named: &mut Option<String>,
    ) -> Done<Reply> {
        let operations = operations(&Value::Object(body))?;
        let db = self.begin().await?;
        let stored = db.load(kind, id).await?.ok_or_else(not_found)?;
        *named = Some(id.to_string());
        let mut resource = stored.resource;
        if kind == Kind::Groups {
            let members = self.member_ids(&db, id).await?;
            let members: Vec<Value> = (members.iter()).map(|id| json!({"value": id})).collect();
            resource.insert("members".into(), members.into());
        }
        for operation in &operations {
            apply(&mut resource, operation)?;
        }
        let candidate = candidate(kind, resource)?;
        let (stored, members) = self.store(&db, kind, Some(id), candidate).await?;
        db.commit().await?;
        Ok(match kind {
            Kind::Users => {
                let document = self.document(kind, &stored, members.as_deref());
                Reply::ok(StatusCode::OK, Some(document))
            }
            Kind::Groups => Reply::ok(StatusCode::NO_CONTENT, None),
        })
    }

    /// `DELETE /<kind>/<id>`: 204. A user leaves every group and a revocation marker.
    async fn delete(&self, kind: Kind, id: &str, named: &mut Option<String>) -> Done<Reply> {
        let db = self.begin().await?;
        if kind == Kind::Users {
            let touched = "UPDATE {s}.directory_groups SET last_modified = now() WHERE id IN \
                           (SELECT group_id FROM {s}.directory_members WHERE user_id = $1)";
            db.execute(touched, &[&id]).await?;
        }
        let sql = match kind {
            Kind::Users => {
                "DELETE FROM {s}.directory_users WHERE id = $1 RETURNING user_name, external_id"
            }
            Kind::Groups => "DELETE FROM {s}.directory_groups WHERE id = $1 RETURNING id",
        };
        let rows = db.query(sql, &[&id]).await?;
        let Some(row) = rows.first() else {
            return Err(not_found());
        };
        *named = Some(id.to_string());
        if kind == Kind::Users {
            let (Ok(user_name), Ok(external_id)) = (
                row.try_get::<_, String>(0),
                row.try_get::<_, Option<String>>(1),
            ) else {
                return Err(not_found());
            };
            let marker =
                "INSERT INTO {s}.directory_revoked (user_name, external_id) VALUES ($1, $2)";
            db.execute(marker, &[&user_name, &external_id]).await?;
        }
        db.commit().await?;
        Ok(Reply::ok(StatusCode::NO_CONTENT, None))
    }

    /// Stores `candidate` as `id` (a new resource without one) inside `begin`'s transaction:
    /// `uniqueness` for a `userName` (ignoring case) or `externalId` another user holds, or a
    /// `group_attribute` another group holds; `invalidValue` for a member naming no stored user.
    /// An active user clears the revocation markers its `user_attribute` matches. Returns the
    /// stored resource and, for a group, its members.
    async fn store(
        &self,
        db: &Db,
        kind: Kind,
        id: Option<&str>,
        candidate: Candidate,
    ) -> Done<(Stored, Option<Vec<String>>)> {
        let taken = || {
            Fail(
                StatusCode::CONFLICT,
                Some("uniqueness"),
                "another resource holds a unique attribute",
            )
        };
        let Candidate {
            resource,
            name,
            external_id,
            active,
            active_named,
            members,
        } = candidate;
        let resource = Value::Object(resource).to_string();
        let rows = match kind {
            Kind::Users => {
                let held = "SELECT 1 FROM {s}.directory_users WHERE (lower(user_name) = \
                            lower($1) OR external_id = $2) AND ($3::text IS NULL OR id <> $3) \
                            LIMIT 1";
                if !db
                    .query(held, &[&name, &external_id, &id])
                    .await?
                    .is_empty()
                {
                    return Err(taken());
                }
                let params: [&(dyn ToSql + Sync); 4] = [&name, &external_id, &active, &resource];
                let rows = match id {
                    None => {
                        let sql = format!(
                            "INSERT INTO {{s}}.directory_users (user_name, external_id, active, \
                             resource) VALUES ($1, $2, $3, $4::text::jsonb) RETURNING {COLUMNS}"
                        );
                        db.query(&sql, &params).await?
                    }
                    Some(id) => {
                        let sql = format!(
                            "UPDATE {{s}}.directory_users SET user_name = $1, external_id = $2, \
                             active = $3, resource = $4::text::jsonb, last_modified = now() \
                             WHERE id = $5 RETURNING {COLUMNS}"
                        );
                        db.query(&sql, &[params[0], params[1], params[2], params[3], &id])
                            .await?
                    }
                };
                if active && active_named {
                    let clear = match self.user_attribute {
                        "externalId" => "DELETE FROM {s}.directory_revoked WHERE external_id = $1",
                        _ => "DELETE FROM {s}.directory_revoked WHERE lower(user_name) = lower($1)",
                    };
                    let matched = match self.user_attribute {
                        "externalId" => external_id.as_deref(),
                        _ => Some(name.as_str()),
                    };
                    if let Some(matched) = matched {
                        db.execute(clear, &[&matched]).await?;
                    }
                }
                rows
            }
            Kind::Groups => {
                let (column, unique) = match self.group_attribute {
                    "externalId" => ("external_id", external_id.as_deref()),
                    _ => ("display_name", Some(name.as_str())),
                };
                if let Some(unique) = unique {
                    let held = format!(
                        "SELECT 1 FROM {{s}}.directory_groups WHERE {column} = $1 AND ($2::text \
                         IS NULL OR id <> $2) LIMIT 1"
                    );
                    if !db.query(&held, &[&unique, &id]).await?.is_empty() {
                        return Err(taken());
                    }
                }
                let unknown = bad("invalidValue", "a member names no stored user");
                if !members.iter().all(|m| is_id(m)) {
                    return Err(unknown);
                }
                let known = "SELECT count(*) FROM {s}.directory_users WHERE id = ANY($1)";
                let found: i64 = db.query(known, &[&members]).await?[0].get(0);
                if found != members.len() as i64 {
                    return Err(unknown);
                }
                let rows = match id {
                    None => {
                        let sql = format!(
                            "INSERT INTO {{s}}.directory_groups (display_name, external_id, \
                             resource) VALUES ($1, $2, $3::text::jsonb) RETURNING {COLUMNS}"
                        );
                        db.query(&sql, &[&name, &external_id, &resource]).await?
                    }
                    Some(id) => {
                        let sql = format!(
                            "UPDATE {{s}}.directory_groups SET display_name = $1, external_id = \
                             $2, resource = $3::text::jsonb, last_modified = now() WHERE id = $4 \
                             RETURNING {COLUMNS}"
                        );
                        db.query(&sql, &[&name, &external_id, &resource, &id])
                            .await?
                    }
                };
                let provisioned = "UPDATE {s}.directory_state SET groups_provisioned = true";
                db.execute(provisioned, &[]).await?;
                let group = rows
                    .first()
                    .and_then(|row| row.try_get::<_, String>(0).ok());
                let group = group.ok_or_else(not_found)?;
                let drop = "DELETE FROM {s}.directory_members WHERE group_id = $1";
                db.execute(drop, &[&group]).await?;
                let add = "INSERT INTO {s}.directory_members (group_id, user_id) SELECT $1, \
                           unnest($2::text[])";
                db.execute(add, &[&group, &members]).await?;
                rows
            }
        };
        let stored = rows.first().and_then(Stored::of).ok_or_else(not_found)?;
        let members = (kind == Kind::Groups).then_some(members);
        Ok((stored, members))
    }

    /// `/ServiceProviderConfig`: `patch` and `filter` (up to `PAGE_MAX` results) supported;
    /// `bulk`, `sort`, `etag` and `changePassword` not; the bearer token scheme.
    fn provider_config(&self) -> Reply {
        Reply::ok(
            StatusCode::OK,
            Some(json!({
                "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
                "patch": {"supported": true},
                "bulk": {"supported": false, "maxOperations": 0, "maxPayloadSize": 0},
                "filter": {"supported": true, "maxResults": PAGE_MAX},
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
                    "location": format!("{}/ServiceProviderConfig", self.base),
                },
            })),
        )
    }

    /// `/ResourceTypes`: `User` and `Group`, with no extension.
    fn resource_types(&self) -> Reply {
        let of = |kind: Kind| {
            let name = kind.resource_type();
            json!({
                "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
                "id": name,
                "name": name,
                "endpoint": format!("/{}", kind.name()),
                "schema": kind.schema(),
                "meta": {
                    "resourceType": "ResourceType",
                    "location": format!("{}/ResourceTypes/{name}", self.base),
                },
            })
        };
        Reply::ok(
            StatusCode::OK,
            Some(listed(vec![of(Kind::Users), of(Kind::Groups)])),
        )
    }
}

/// Whether an `excludedAttributes` value names `members`.
fn excludes_members(value: &str) -> bool {
    (value.split(',')).any(|name| name.trim().eq_ignore_ascii_case("members"))
}

/// A list response of `resources`, all of them.
fn listed(resources: Vec<Value>) -> Value {
    json!({
        "schemas": [LIST],
        "totalResults": resources.len(),
        "startIndex": 1,
        "itemsPerPage": resources.len(),
        "Resources": resources,
    })
}

/// `/Schemas`: the attributes Meada reads of a user and a group, as it compares them: only
/// `userName` ignoring case, and a group's `group_attribute` unique.
fn schemas(group_attribute: &str) -> Reply {
    let attribute = |name: &str, unique: bool| {
        json!({
            "name": name, "type": "string", "multiValued": false,
            "required": name != "externalId", "caseExact": name != "userName",
            "mutability": "readWrite", "returned": "default",
            "uniqueness": if unique { "server" } else { "none" },
        })
    };
    let members = json!({
        "name": "members", "type": "complex", "multiValued": true, "required": false,
        "mutability": "readWrite", "returned": "default", "uniqueness": "none",
        "subAttributes": [{
            "name": "value", "type": "string", "multiValued": false, "required": true,
            "caseExact": true, "mutability": "immutable", "returned": "default",
            "uniqueness": "none",
        }],
    });
    let user = json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
        "id": USER,
        "name": "User",
        "attributes": [
            attribute("userName", true),
            attribute("externalId", true),
            {"name": "active", "type": "boolean", "multiValued": false, "required": false,
             "mutability": "readWrite", "returned": "default"},
        ],
    });
    let group = json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
        "id": GROUP,
        "name": "Group",
        "attributes": [
            attribute("displayName", group_attribute == "displayName"),
            attribute("externalId", group_attribute == "externalId"),
            members,
        ],
    });
    Reply::ok(StatusCode::OK, Some(listed(vec![user, group])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(text: &str) -> Vec<(String, Option<(String, Value)>)> {
        let steps = path(text).unwrap_or_else(|_| panic!("{text}"));
        steps.into_iter().map(|s| (s.name, s.filter)).collect()
    }

    #[test]
    fn paths_take_a_schema_an_attribute_a_filter_and_a_sub_attribute() {
        assert_eq!(steps("userName"), [("userName".into(), None)]);
        assert_eq!(
            steps("name.givenName"),
            [("name".into(), None), ("givenName".into(), None)]
        );
        let work = Some(("type".to_string(), json!("work")));
        assert_eq!(
            steps("emails[type eq \"work\"].value"),
            [("emails".into(), work), ("value".into(), None)]
        );
        let enterprise = "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User";
        assert_eq!(
            steps(&format!("{enterprise}:manager.value")),
            [
                (enterprise.into(), None),
                ("manager".into(), None),
                ("value".into(), None)
            ]
        );
        assert_eq!(steps(&format!("{USER}:active")), [("active".into(), None)]);
        let id = Some(("value".to_string(), json!("a:b]c")));
        assert_eq!(
            steps("members[value eq \"a:b]c\"]"),
            [("members".into(), id)]
        );
        for bad in [
            "",
            "a..b",
            "a.b.c",
            "emails[type co \"w\"]",
            "emails[type eq work]",
            "emails[type eq \"w\"",
            "emails[type eq \"w\"]x",
            "urn:x",
            "a:b",
            "1a",
            "members[value eq \"x\"] or 1",
        ] {
            assert!(path(bad).is_err(), "{bad}");
        }
    }

    fn patched(resource: Value, ops: Value) -> Done<Value> {
        let Value::Object(mut resource) = resource else {
            unreachable!()
        };
        for operation in operations(&json!({"Operations": ops}))? {
            apply(&mut resource, &operation)?;
        }
        Ok(Value::Object(resource))
    }

    #[test]
    fn operations_follow_rfc_7644_names_ignoring_case() {
        let user = json!({"userName": "a", "emails": [{"type": "work", "value": "a@x"}]});
        let done = patched(
            user.clone(),
            json!([
                {"op": "Replace", "path": "USERNAME", "value": "b"},
                {"op": "add", "path": "displayName", "value": "A"},
                {"op": "replace", "path": "emails[type eq \"WORK\"].value", "value": "b@x"},
                {"op": "Add", "path": "emails[type eq \"home\"].value", "value": "h@x"},
                {"op": "add", "path": "name.familyName", "value": "F"},
                {"op": "replace", "value": {"active": "False", "name.givenName": "G"}},
            ]),
        );
        assert_eq!(
            done.ok().unwrap(),
            json!({
                "userName": "b", "displayName": "A", "active": "False",
                "emails": [{"type": "work", "value": "b@x"}, {"type": "home", "value": "h@x"}],
                "name": {"familyName": "F", "givenName": "G"},
            })
        );
        let group = json!({"displayName": "g", "members": [{"value": "1"}, {"value": "2"}]});
        let done = patched(
            group,
            json!([
                {"op": "add", "path": "members", "value": [{"value": "3"}, {"value": "1"}]},
                {"op": "remove", "path": "members[value eq \"2\"]"},
                {"op": "remove", "path": "members", "value": [{"value": "3"}]},
            ]),
        );
        assert_eq!(
            done.ok().unwrap(),
            json!({"displayName": "g", "members": [{"value": "1"}]})
        );
        // A replace through a filter that matches nothing has no target; a remove needs a path.
        for (operations, kind) in [
            (
                json!([{"op": "replace", "path": "emails[type eq \"x\"].value", "value": "v"}]),
                "noTarget",
            ),
            (json!([{"op": "remove"}]), "noTarget"),
            (json!([{"op": "move", "path": "userName"}]), "invalidSyntax"),
            (json!([{"op": "add", "path": "userName"}]), "invalidValue"),
            (
                json!([{"op": "add", "path": "a[b gt 1]", "value": 1}]),
                "invalidPath",
            ),
        ] {
            let Err(Fail(_, Some(found), _)) = patched(user.clone(), operations.clone()) else {
                panic!("{operations}")
            };
            assert_eq!(found, kind, "{operations}");
        }
    }

    #[test]
    fn a_candidate_drops_id_meta_and_password_and_checks_its_attributes() {
        let Value::Object(user) = json!({
            "id": "x", "META": {}, "Password": "p", "userName": "a", "active": "True",
        }) else {
            unreachable!()
        };
        let done = candidate(Kind::Users, user).ok().unwrap();
        assert_eq!(
            Value::Object(done.resource),
            json!({"userName": "a", "active": true})
        );
        let Value::Object(group) = json!({"displayName": "g", "members": [{"value": "b"}, {"value": "a"}, {"value": "b"}]})
        else {
            unreachable!()
        };
        let done = candidate(Kind::Groups, group).ok().unwrap();
        assert_eq!(done.members, ["a", "b"]);
        assert_eq!(Value::Object(done.resource), json!({"displayName": "g"}));
        for (kind, resource) in [
            (Kind::Users, json!({"userName": ""})),
            (Kind::Users, json!({"userName": "a", "active": "yes"})),
            (Kind::Users, json!({"userName": "a", "externalId": 1})),
            (
                Kind::Groups,
                json!({"displayName": "g", "members": [{"display": "x"}]}),
            ),
        ] {
            let Value::Object(resource) = resource else {
                unreachable!()
            };
            assert!(candidate(kind, resource).is_err());
        }
        let Value::Object(big) = json!({"userName": "a", "x": "y".repeat(SCIM_RESOURCE_MAX)})
        else {
            unreachable!()
        };
        let Err(Fail(status, None, _)) = candidate(Kind::Users, big) else {
            panic!("a resource past the cap")
        };
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn filters_are_one_eq_on_a_supported_attribute() {
        assert_eq!(
            filter(Kind::Users, "UserName eq \"A b\"").ok().unwrap(),
            ("lower(user_name) = lower($1)", "A b".into())
        );
        assert_eq!(
            filter(Kind::Groups, "externalId EQ \"x\"").ok().unwrap().0,
            "external_id = $1"
        );
        for bad in [
            "userName co \"a\"",
            "userName eq a",
            "displayName eq \"a\"",
            "userName eq \"a\" and active eq true",
            "emails eq \"a\"",
            "",
        ] {
            assert!(filter(Kind::Users, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_token_is_compared_whole() {
        let config = super::super::Config {
            token: crate::modules::Secret::Env("X".into()),
            user_claim: "oid".into(),
            user_attribute: "userName",
            group_attribute: "displayName",
        };
        let postgres: crate::postgres::File = toml::from_str("").unwrap();
        let postgres = postgres.validate(std::path::Path::new("")).ok().unwrap();
        let token = "t".repeat(32);
        let scim = Scim::new(&config, &token, postgres, "https://m.example");
        assert!(scim.admits(Some(&format!("Bearer {token}"))));
        assert!(scim.admits(Some(&format!("bearer {token}"))));
        for wrong in [
            None,
            Some(token.clone()),
            Some(format!("Bearer {}", &token[1..])),
            Some(format!("Bearer {token}x")),
            Some(format!("Basic {token}")),
        ] {
            assert!(!scim.admits(wrong.as_deref()), "{wrong:?}");
        }
        assert_eq!(resource("/scim/v2/Users/abc"), "Users");
        assert_eq!(resource("/scim/v2/Groups"), "Groups");
        assert_eq!(resource("/scim/v2/Bulk"), "-");
    }
}
