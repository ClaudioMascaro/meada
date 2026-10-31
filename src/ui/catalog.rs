//! The catalog's reads (#341, ADR 0021 contract 1, feature `workspaces`): `GET /api/v1/catalog`
//! and `POST /api/v1/catalog/check`, the picker's data. Both answer from the newest permissions
//! set the OIDC module's cache holds, never from GitHub or Postgres, and only about the
//! repositories the session may select: an identity that is not selectable is absent from the
//! list and `unavailable` in a check, with no reason, so neither reads as an oracle on the
//! catalog. While the platform or the caller is not ready (no set, a stale one, no login) the
//! answer says `pending` and why, for every identity alike.

use meada_core::manifest::identity_problem;

use super::*;
use crate::oidc::Catalog;

// ponytail: the most identities `POST catalog/check` takes, and the most bytes of its body.
const CHECK_IDENTITIES: usize = 50;
pub(super) const CHECK_BODY_MAX: usize = 16 << 10;

// ponytail: the most records `GET catalog` answers at once, and how many without `limit`.
pub(super) const LIMIT_MAX: usize = 100;
const LIMIT: usize = 50;

// ponytail: the most characters of `q`, a prefix of an identity.
pub(super) const PREFIX_MAX: usize = 128;

// ponytail: the most characters of `after`, the opaque cursor.
pub(super) const CURSOR_MAX: usize = 256;

/// The cursor's alphabet: base64url, no padding.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `bytes` in base64url without padding.
fn encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        let digits = chunk.len() + 1;
        for i in 0..digits {
            text.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    text
}

/// The bytes `text` encodes; `None` for anything but the canonical encoding of some bytes.
fn decode(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3 + 2);
    for chunk in text.as_bytes().chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            let digit = ALPHABET.iter().position(|a| a == c)? as u32;
            n |= digit << (18 - 6 * i);
        }
        for i in 0..chunk.len() - 1 {
            bytes.push((n >> (16 - 8 * i)) as u8);
        }
    }
    (encode(&bytes) == text).then_some(bytes)
}

/// The identity a cursor names: 1 to `CURSOR_MAX` characters that decode to an identity.
fn cursor(text: &str) -> Option<String> {
    if text.is_empty() || text.len() > CURSOR_MAX {
        return None;
    }
    let identity = String::from_utf8(decode(text)?).ok()?;
    identity_ok(&identity).then_some(identity)
}

/// Whether `identity` is one a caller may name: an identity with no control character or space.
pub(super) fn identity_ok(identity: &str) -> bool {
    identity_problem(identity).is_none()
        && !identity
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
}

/// The catalog query: `limit`, `after` and `q`, each at most once and nothing else.
struct Query {
    limit: usize,
    after: Option<String>,
    prefix: String,
}

impl Query {
    fn read(query: Option<&str>) -> Option<Self> {
        let mut found = Self {
            limit: LIMIT,
            after: None,
            prefix: String::new(),
        };
        let (mut limit, mut after, mut prefix) = (false, false, false);
        for pair in query
            .filter(|q| !q.is_empty())
            .into_iter()
            .flat_map(|q| q.split('&'))
        {
            let (name, value) = pair.split_once('=')?;
            let value = percent_decode_str(value).decode_utf8().ok()?;
            match name {
                "limit" if !limit => {
                    limit = true;
                    found.limit = value
                        .parse::<usize>()
                        .ok()
                        .filter(|n| (1..=LIMIT_MAX).contains(n))?;
                }
                "after" if !after => {
                    after = true;
                    found.after = Some(cursor(&value)?);
                }
                "q" if !prefix => {
                    prefix = true;
                    if value.chars().count() > PREFIX_MAX || value.contains('\0') {
                        return None;
                    }
                    found.prefix = value.into_owned();
                }
                _ => return None,
            }
        }
        Some(found)
    }
}

/// The answer's `state`, `reason` and, when the set is known, `set`.
pub(super) fn header(catalog: &Catalog) -> Value {
    let state = if catalog.reason.is_some() {
        "pending"
    } else {
        "ready"
    };
    let set = catalog.set.as_ref().map(|set| {
        json!({
            "finished": set.finished,
            "age_seconds": set.age_seconds,
            "max_age_seconds": set.max_age_seconds,
        })
    });
    json!({"state": state, "reason": catalog.reason, "set": set})
}

impl Console {
    /// The catalog `session` may select from: none unless `[catalog]` is configured and the
    /// OIDC module keeps the permissions cache.
    pub(super) fn catalog_of(&self, session: &Session) -> Option<Catalog> {
        self.bootstrap.catalog.as_ref()?;
        let oidc = self.oidc.as_ref()?;
        oidc.catalog(session.login.as_deref(), session.operator)
    }

    /// `GET catalog?limit=&after=&q=`: `{"state", "reason", "set", "repositories": [{"identity"}],
    /// "next"}`, the identities the session may select, ordered by identity, at most `limit`
    /// (1 to 100, default 50) after the identity `after` names and starting with `q`; `next` is
    /// the cursor of the following page or null. Any other query is a 400. A pending catalog
    /// lists nothing.
    pub(super) fn catalog(&self, query: Option<&str>, session: &Session) -> Answer {
        let Some(catalog) = self.catalog_of(session) else {
            return error(StatusCode::NOT_FOUND, "not_found", &Msg::UiNoRoute.text());
        };
        let Some(query) = Query::read(query) else {
            let message = Msg::CatalogQuery(PREFIX_MAX).text();
            return error(StatusCode::BAD_REQUEST, "bad_request", &message);
        };
        let (page, more) = page(&catalog, query.after.as_deref(), &query.prefix, query.limit);
        let next = page.last().filter(|_| more).map(|i| encode(i.as_bytes()));
        let mut document = header(&catalog);
        document["repositories"] = page.iter().map(|i| json!({"identity": i})).collect();
        document["next"] = json!(next);
        json_answer(StatusCode::OK, &document)
    }

    /// `POST catalog/check` with `{"identities": [...]}`, at most `CHECK_IDENTITIES`, each an
    /// identity: `{"state", "reason", "results": [{"identity", "state"}]}` in the order asked,
    /// each `selectable` or `unavailable`, or `pending` for all of them while the catalog is.
    pub(super) fn catalog_check(&self, body: &Body, session: &Session) -> Answer {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Asked {
            identities: Vec<String>,
        }
        let Some(catalog) = self.catalog_of(session) else {
            return error(StatusCode::NOT_FOUND, "not_found", &Msg::UiNoRoute.text());
        };
        let body = match body {
            Ok(body) => body,
            Err(StatusCode::PAYLOAD_TOO_LARGE) => {
                let message = Msg::UiTooLarge(CHECK_BODY_MAX).text();
                return error(StatusCode::PAYLOAD_TOO_LARGE, "too_large", &message);
            }
            Err(status) => {
                let message = Msg::CatalogCheckBody(CHECK_IDENTITIES).text();
                return error(*status, "bad_request", &message);
            }
        };
        let asked = serde_json::from_slice::<Asked>(body).ok().filter(|asked| {
            (1..=CHECK_IDENTITIES).contains(&asked.identities.len())
                && asked.identities.iter().all(|i| identity_ok(i))
        });
        let Some(asked) = asked else {
            let message = Msg::CatalogCheckBody(CHECK_IDENTITIES).text();
            return error(StatusCode::BAD_REQUEST, "bad_request", &message);
        };
        json_answer(StatusCode::OK, &checked(&catalog, &asked.identities))
    }
}

/// One page of what `catalog` makes selectable: at most `limit` identities after `after` that
/// start with `prefix`, compared as GitHub compares them on the host (#508), and whether more
/// follow.
fn page<'a>(
    catalog: &'a Catalog,
    after: Option<&str>,
    prefix: &str,
    limit: usize,
) -> (Vec<&'a String>, bool) {
    let prefix = crate::modules::fold(&catalog.host, prefix);
    let mut page: Vec<&String> = (catalog.selectable.iter())
        .filter(|i| after.is_none_or(|a| i.as_str() > a))
        .filter(|i| crate::modules::fold(&catalog.host, i).starts_with(&prefix))
        .take(limit + 1)
        .collect();
    let more = page.len() > limit;
    page.truncate(limit);
    (page, more)
}

/// The check of `identities` against `catalog`: `{"state", "reason", "results": [{"identity",
/// "state"}]}` in the order given, each once, `selectable` or `unavailable`, or `pending` for all
/// of them while the catalog is. The console's check and a proposal's eligibility (ADR 0022).
pub(super) fn checked(catalog: &Catalog, identities: &[String]) -> Value {
    let mut seen = std::collections::HashSet::new();
    // GitHub compares names without case, so an identity on the host matches its lowercase row.
    let folded: std::collections::HashSet<String> = (catalog.selectable.iter())
        .map(|i| crate::modules::fold(&catalog.host, i))
        .collect();
    let results: Vec<Value> = (identities.iter())
        .filter(|i| seen.insert(i.as_str()))
        .map(|identity| {
            let state = match (
                catalog.reason,
                folded.contains(&crate::modules::fold(&catalog.host, identity)),
            ) {
                (Some(_), _) => "pending",
                (None, true) => "selectable",
                (None, false) => "unavailable",
            };
            json!({"identity": identity, "state": state})
        })
        .collect();
    let mut document = header(catalog);
    document.as_object_mut().map(|o| o.remove("set"));
    document["results"] = json!(results);
    document
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identity_on_the_host_is_selectable_whatever_its_case() {
        let catalog = Catalog {
            reason: None,
            set: None,
            selectable: ["github.com/acme/api", "ghe.example/acme/x"]
                .map(String::from)
                .into(),
            host: "github.com".into(),
        };
        let asked = ["github.com/Acme/API", "ghe.example/Acme/X"].map(String::from);
        let states: Vec<_> = checked(&catalog, &asked)["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["state"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(states, ["selectable", "unavailable"]);
    }

    #[test]
    fn a_prefix_on_the_host_lists_whatever_its_case() {
        let catalog = Catalog {
            reason: None,
            set: None,
            selectable: [
                "ghe.example/acme/x",
                "github.com/acme/api",
                "github.com/acme/web",
            ]
            .map(String::from)
            .into(),
            host: "github.com".into(),
        };
        let api = "github.com/acme/api".to_string();
        assert_eq!(
            page(&catalog, None, "github.com/Acme/A", 1),
            (vec![&api], false)
        );
        assert_eq!(
            page(&catalog, None, "github.com/ACME/", 1),
            (vec![&api], true)
        );
        // Another host compares exactly, as before.
        assert!(page(&catalog, None, "ghe.example/Acme", 10).0.is_empty());
    }

    #[test]
    fn a_cursor_is_the_canonical_base64url_of_an_identity() {
        let identity = "github.com/acme/y";
        let text = encode(identity.as_bytes());
        assert_eq!(text, "Z2l0aHViLmNvbS9hY21lL3k");
        assert_eq!(cursor(&text).as_deref(), Some(identity));
        for bad in [
            "",
            "Z",
            "Z2l0aHViLmNvbS9hY21lL3k=",
            "Z2l0aHViLmNvbS9hY21lL3l",
            "!!!!",
            &"A".repeat(CURSOR_MAX + 4),
            &encode(b"no slash"),
            &encode(b"github.com/a/b c"),
        ] {
            assert_eq!(cursor(bad), None, "{bad}");
        }
        for n in 0..9 {
            let bytes: Vec<u8> = (0..n).map(|i: u32| (i * 37 + 200) as u8).collect();
            assert_eq!(decode(&encode(&bytes)), Some(bytes));
        }
    }

    #[test]
    fn a_query_takes_limit_after_and_q_once_and_nothing_else() {
        let ok = |q: &str| Query::read(Some(q)).is_some();
        assert!(Query::read(None).is_some() && ok("") && ok("limit=1&q=github.com%2Facme"));
        assert!(ok("limit=100") && ok(&format!("after={}", encode(b"github.com/a/b"))));
        for bad in [
            "limit=0",
            "limit=101",
            "limit=x",
            "limit=1&limit=2",
            "q=a&q=b",
            "after=zz",
            "other=1",
            "limit",
            &format!("q={}", "a".repeat(PREFIX_MAX + 1)),
        ] {
            assert!(!ok(bad), "{bad}");
        }
    }
}
