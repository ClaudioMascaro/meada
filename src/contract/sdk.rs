//! Mechanical SDK extraction shared by contract kinds: constructor evidence, folded request
//! fields and call sites. Library names and operations belong to each kind's table.

use super::{Found, Role, Site};
use crate::lang::{Part, Value, literal};
use crate::resolve::{Code, Expand};
use std::collections::{BTreeMap, BTreeSet};

mod csharp;
mod go;
pub(crate) mod java;
pub(crate) mod mutations;
pub(crate) mod origins;

/// Complete normalized argument identity for the shared constructor memo. The kind
/// projects scalar leaves; object and list structure is language-neutral syntax.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub(crate) enum Identity<T> {
    #[default]
    Unknown,
    Leaf(T),
    Object(Vec<(String, Identity<T>)>),
    List(Vec<Identity<T>>),
}

impl<T> Identity<T> {
    pub(crate) fn read(value: &Value, leaf: &impl Fn(&Value) -> Option<T>) -> Self {
        match value {
            Value::Object(fields) => Self::Object(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::read(value, leaf)))
                    .collect(),
            ),
            Value::List(values) => {
                Self::List(values.iter().map(|value| Self::read(value, leaf)).collect())
            }
            _ => leaf(value).map_or(Self::Unknown, Self::Leaf),
        }
    }
}

/// A folded SDK name, retaining verified runtime environment aliases. Physical identity
/// projection belongs to the caller's contract kind.
pub(crate) fn argument<'a>(
    code: &Code<'a>,
    path: &'a str,
    value: &Value,
) -> Option<crate::names::Argument> {
    use crate::names::Argument;
    match value {
        Value::Str(parts) => match parts.as_slice() {
            [
                Part::Read {
                    scope,
                    callee,
                    key,
                    then,
                },
            ] if then.is_empty() => {
                let aliases = code.files()[path]
                    .imports
                    .iter()
                    .map(|i| (i.local.as_str(), i.local.as_str()))
                    .collect();
                Some(
                    if csharp::environment(code, path, scope, callee, &aliases) {
                        Argument::Environment(key.clone())
                    } else {
                        Argument::Unresolved
                    },
                )
            }
            _ => crate::names::argument(value),
        },
        Value::Call(call) if code.pack().extensions().contains(&"go") => {
            Some(go::environment(call).map_or(Argument::Unresolved, Argument::Environment))
        }
        _ => crate::names::argument(value),
    }
}

/// The SDK spellings of one client, supplied by its contract kind.
pub(crate) struct Client {
    pub java: &'static str,
    pub go: &'static str,
    pub csharp: &'static str,
}

/// An operation's direction and the request field naming the contract.
pub(crate) struct Operation {
    pub method: &'static str,
    /// The path to the name: `.` descends, `[]` takes every element, `.*` every key of a map.
    /// Before the last member, `A|B` names the variants an element may set (a transaction entry's
    /// `Put|Update|Delete`): an element setting none of them has no use for this row. A method
    /// can have several rows, one per direction.
    pub field: &'static str,
    pub role: Role,
    /// A hierarchy query does not identify one physical resource.
    pub hierarchy: bool,
}

/// A kind's SDK client and the operations it recognizes.
pub(crate) struct Service {
    pub client: Client,
    /// SDK client types whose declaration alone is the client's origin, in full: the injected
    /// clients SQS forms took before constructor evidence (#660). Empty for the other services.
    pub declared: &'static [&'static str],
    pub kind: &'static str,
    pub operations: &'static [Operation],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Language {
    Java,
    Go,
    Csharp,
}

impl Language {
    fn of(code: &Code) -> Option<Self> {
        let extensions = code.pack().extensions();
        if extensions.contains(&"java") {
            Some(Self::Java)
        } else if extensions.contains(&"go") {
            Some(Self::Go)
        } else if extensions.contains(&"cs") {
            Some(Self::Csharp)
        } else {
            None
        }
    }

    fn method(self, method: &str, canonical: &str) -> bool {
        match self {
            Self::Java => {
                method.len() == canonical.len()
                    && method.as_bytes().first()
                        == canonical
                            .as_bytes()
                            .first()
                            .map(u8::to_ascii_lowercase)
                            .as_ref()
                    && method.get(1..) == canonical.get(1..)
            }
            Self::Go => method == canonical,
            Self::Csharp => csharp::method(method, canonical),
        }
    }
}

/// Extract the kind's SDK operations. Only a service's `declared` types stand in for
/// construction.
pub(crate) fn extract<'a>(
    code: &Code<'a>,
    prefix: &str,
    services: &[Service],
    indexed: Option<&crate::index::Indexed>,
) -> Vec<Found> {
    let Some(language) = Language::of(code) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let written = std::cell::OnceCell::new();
    let configs = std::cell::OnceCell::new();
    let inject = |code: &Code<'a>, path: &'a str, on: crate::resolve::Injected<'a>| {
        let indexed = indexed?;
        let configs = configs
            .get_or_init(|| crate::jvm::spring::Configs::read(indexed.configs, indexed.manifests));
        crate::jvm::spring::inject(configs, code, path, on.annotated()?)
    };
    for (&path, facts) in code.files() {
        if language == Language::Java && !path.ends_with(".java") {
            continue;
        }
        let mut origins = origins::Origins::new(code, language);
        let aliases: BTreeMap<_, _> = if language == Language::Csharp {
            facts
                .imports
                .iter()
                .map(|i| (i.local.as_str(), i.local.as_str()))
                .collect()
        } else {
            BTreeMap::new()
        };
        for (index, stored) in facts.calls.iter().enumerate() {
            let method = crate::resolve::method(&stored.callee);
            // A method can have several rows: a transaction's entries differ in direction.
            let operations = services.iter().flat_map(|service| {
                (service.operations.iter())
                    .filter(|o| language.method(method, o.method))
                    .map(move |operation| (service, operation))
            });
            for (service, operation) in operations {
                if !origins.call(path, index, &service.client)
                    && !origins.declared(path, index, service.declared)
                {
                    continue;
                }
                let changed =
                    changed_request(code, path, index, operation.field, language, &written);
                let read = |args: &[Value], keywords: &crate::resolve::Keywords| {
                    let [Value::Call(call)] = args else {
                        return Some(vec![Name::default()]);
                    };
                    let values = request(call, keywords, operation.field, language)?;
                    let mut names = Vec::new();
                    for value in &values {
                        names.push(match (changed, value) {
                            (true, _) => Name::default(),
                            (false, Some(v)) => name(code, path, Some(v), service.kind, &aliases)?,
                            // ADR 0026: an entry without a bus sends to `default`; an SNS
                            // phone or endpoint publish names no topic.
                            (false, None) if service.kind == "eventbridge" => Name {
                                literal: Some("default".into()),
                                env: None,
                            },
                            (false, None) if service.kind == "sns" => continue,
                            (false, None) => Name::default(),
                        });
                    }
                    Some(names)
                };
                let injected = (language == Language::Java)
                    .then_some(&inject as &crate::resolve::Inject<'a, '_>);
                for expanded in code.expand_with(Expand::Chain(path, index), injected, &read) {
                    let (at, index) = expanded.site;
                    for mut name in expanded.found {
                        if operation.hierarchy {
                            name.literal = None;
                        }
                        let contract = match &name.literal {
                            Some(name) => format!("{prefix}::{}::{name}", service.kind),
                            None => format!("{prefix}::{}", service.kind),
                        };
                        found.push(Found {
                            identity: None,
                            role: operation.role,
                            site: if operation.role == Role::Consumer {
                                Site::Def {
                                    path: at.into(),
                                    name: code.files()[at].calls[index].scope.clone(),
                                }
                            } else {
                                Site::Call {
                                    path: at.into(),
                                    index,
                                }
                            },
                            contract: Some(contract),
                            via: name.env.map(|v| format!("env:{v}")),
                            method: Some(operation.method.into()),
                            fragments: Vec::new(),
                            shape: None,
                        });
                    }
                }
            }
        }
    }
    found
}

/// A named request can change after construction, and so can a named value an inline request
/// holds on the name's path (an EventBridge entry); inline literals cannot change before the call.
fn changed_request<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    selector: &str,
    language: Language,
    written: &std::cell::OnceCell<BTreeSet<&'a str>>,
) -> bool {
    let Some(call) = code.call(path, index) else {
        return false;
    };
    let request = match language {
        Language::Go => call.args.get(1),
        Language::Csharp => call.keyword("request").or_else(|| call.args.first()),
        Language::Java => {
            // A step sets a member of the name's path (`topicArn`, `entries`, `eventBusName`).
            let setter = |method: &str| {
                selector.split(['.', '|']).any(|member| {
                    let member = member.trim_end_matches("[]");
                    member.get(..1).is_some_and(|head| {
                        method.len() == member.len()
                            && method[..1] == head.to_ascii_lowercase()
                            && method[1..] == member[1..]
                    })
                })
            };
            return (call.args.first())
                .is_some_and(|r| java::changed(code, path, &call.scope, r, &call.range, &setter));
        }
    };
    // Follow the name's path through the request as written: a named value met on it changes
    // when a file writes the member read off it.
    let mut values: Vec<&Value> = request.into_iter().collect();
    for member in selector.split('.') {
        let mut next = Vec::new();
        for value in values {
            // Each variant of a member is on the path.
            for field in member.split('|').map(|f| f.trim_end_matches("[]")) {
                if let Value::Ref(_) = value {
                    if opaque(code, field, language, written) {
                        return true;
                    }
                    continue;
                }
                let inner = match language {
                    Language::Go => go::field_of(value, field),
                    _ => csharp::field_of(value, field),
                };
                let Some(Some(inner)) = inner else {
                    continue;
                };
                match member.ends_with("[]").then(|| sequence(inner)).flatten() {
                    Some(items) => next.extend(items),
                    None => next.push(inner),
                }
            }
        }
        values = next;
    }
    false
}

/// Whether a file writes `field` of a named request value (Go and C#).
fn opaque<'a>(
    code: &Code<'a>,
    field: &str,
    language: Language,
    written: &std::cell::OnceCell<BTreeSet<&'a str>>,
) -> bool {
    let field = match language {
        Language::Csharp => csharp::field_name(field),
        _ => field,
    };
    code.field_written(field)
        || language == Language::Csharp
            && written
                .get_or_init(|| {
                    // C# keeps selector writes as bound paths. Index their last member once,
                    // across files, so a write through any alias invalidates a named request.
                    code.files()
                        .keys()
                        .flat_map(|&path| code.bindings(path))
                        .filter_map(|bound| {
                            bound
                                .name
                                .rsplit_once('.')
                                .map(|(_, field)| field.trim_end_matches('='))
                        })
                        .collect()
                })
                .contains(field)
}

/// Read scalar fields, name lists, event entries and table-keyed batch requests. Each value is
/// visited once; every path here is a short constant supplied by the kind, never source text.
/// `None` is a field a request Meada sees whole leaves out.
fn request<'v>(
    call: &'v crate::lang::Invocation,
    keywords: &'v crate::resolve::Keywords,
    path: &str,
    language: Language,
) -> Option<Vec<Option<Value>>> {
    let (head, tail) = path
        .split_once('.')
        .map_or((path, None), |(h, t)| (h, Some(t)));
    let values = match language {
        Language::Java => java::names(call, head.trim_end_matches("[]")),
        Language::Go => go::names(call, head.trim_end_matches("[]")),
        Language::Csharp => csharp::names(call, keywords, head.trim_end_matches("[]")),
    }?;
    let whole = match language {
        Language::Java => call.args.first(),
        Language::Go => call.args.get(1),
        Language::Csharp => (keywords
            .iter()
            .find(|(k, _)| k == "request")
            .map(|(_, v)| v))
        .or_else(|| call.args.first()),
    };
    // `Put|Update.TableName`: the variants an element may set, then the name read off it.
    let (variants, field) = tail.map_or((None, None), |tail| match tail.rsplit_once('.') {
        Some((variants, field)) => (Some(variants), Some(field)),
        None => (None, Some(tail)),
    });
    let member = |value: &'v Value, field: &str| match language {
        Language::Java => Some(java::field_of(value, field)?.first().copied().flatten()),
        Language::Go => go::field_of(value, field),
        Language::Csharp => csharp::field_of(value, field),
    };
    let mut out = Vec::new();
    let mut skipped = false;
    for value in values {
        let Some(value) = value else {
            out.push((!whole.is_some_and(complete)).then_some(Value::Unknown));
            continue;
        };
        if tail == Some("*") {
            out.extend(map_keys(value)?.into_iter().map(Some));
            continue;
        }
        let values = if head.ends_with("[]") {
            sequence(value)?
        } else {
            vec![value]
        };
        for value in values {
            let value = match variants {
                None => value,
                Some(variants) => {
                    let mut set = None;
                    for variant in variants.split('|') {
                        set = set.or(member(value, variant)?);
                    }
                    match set {
                        Some(set) => set,
                        // A whole element that sets none of the variants has no use here.
                        None if complete(value) => {
                            skipped = true;
                            continue;
                        }
                        None => {
                            out.push(Some(Value::Unknown));
                            continue;
                        }
                    }
                }
            };
            let read = match field {
                None => Some(value),
                Some(field) => member(value, field)?,
            };
            out.push(match read {
                Some(read) => Some(read.clone()),
                None => (!complete(value)).then_some(Value::Unknown),
            });
        }
    }
    if out.is_empty() && !skipped {
        out.push(Some(Value::Unknown));
    }
    Some(out)
}

/// Whether a request value shows every field it sets: a Go or C# literal, or a Java builder
/// chain started by an SDK request type's `builder()` (`PublishRequest.builder()`, not a
/// helper or factory of the repository's own named `builder`; a transaction entry is built by
/// `TransactWriteItem.builder()` or `TransactGetItem.builder()`). Only then does a missing field
/// mean the request leaves it out.
fn complete(value: &Value) -> bool {
    match value {
        Value::Object(_) => true,
        Value::New(_, args) => matches!(args.as_slice(), [Value::Object(_)]),
        Value::Call(call) => std::iter::successors(Some(call), |s| match s.receiver.as_deref() {
            Some(Value::Call(next)) => Some(next),
            _ => None,
        })
        .last()
        .is_some_and(|first| {
            first.callee.rsplit_once('.').is_some_and(|(ty, method)| {
                let ty = ty.rsplit('.').next().unwrap_or(ty);
                method == "builder"
                    && (ty.ends_with("Request")
                        || ty.ends_with("RequestEntry")
                        || matches!(ty, "TransactWriteItem" | "TransactGetItem"))
            })
        }),
        _ => false,
    }
}

fn sequence(value: &Value) -> Option<Vec<&Value>> {
    match value {
        Value::Ref(_) => None,
        Value::List(values) => Some(values.iter().collect()),
        Value::New(_, values) => match values.as_slice() {
            [Value::List(values)] => Some(values.iter().collect()),
            _ => Some(vec![value]),
        },
        Value::Call(call) if java::sequence(call) => Some(call.args.iter().collect()),
        value => Some(vec![value]),
    }
}

fn map_keys(value: &Value) -> Option<Vec<Value>> {
    let value = match value {
        Value::New(_, args) => match args.as_slice() {
            [value] => value,
            _ => return Some(vec![Value::Unknown]),
        },
        other => other,
    };
    match value {
        Value::Ref(_) => None,
        Value::Object(fields) => Some(
            fields
                .iter()
                .map(|(key, value)| match (key.as_str(), value) {
                    ("[]", Value::List(pair)) => pair.first().cloned().unwrap_or(Value::Unknown),
                    ("[]" | "...", _) => Value::Unknown,
                    _ => Value::Str(vec![Part::Text(key.clone())]),
                })
                .collect(),
        ),
        Value::List(entries) => Some(
            entries
                .iter()
                .map(|entry| match entry {
                    Value::List(pair) if pair.len() == 2 => pair[0].clone(),
                    _ => Value::Unknown,
                })
                .collect(),
        ),
        Value::Call(call) if java::map(call) => {
            Some(call.args.iter().step_by(2).cloned().collect())
        }
        _ => Some(vec![Value::Unknown]),
    }
}

#[derive(Default)]
struct Name {
    literal: Option<String>,
    env: Option<String>,
}

fn name<'a>(
    code: &Code<'a>,
    path: &'a str,
    value: Option<&Value>,
    kind: &str,
    aliases: &BTreeMap<&'a str, &'a str>,
) -> Option<Name> {
    match value {
        Some(Value::Ref(_)) => None,
        Some(Value::Str(parts)) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => None,
        Some(Value::Str(parts)) => {
            let env = parts.iter().find_map(|p| match p {
                Part::Env(v) | Part::EnvOr(v, _) => Some(v.clone()),
                Part::Read {
                    scope,
                    callee,
                    key,
                    then,
                } if then.is_empty() && csharp::environment(code, path, scope, callee, aliases) => {
                    Some(key.clone())
                }
                _ => None,
            });
            // Queue identity is its URL's final segment, even with a dynamic account/host.
            // This preserves the existing SQS form's custom endpoint and wrapper behavior.
            let literal = if kind == "sqs" {
                let mut tail: Vec<_> = parts
                    .iter()
                    .rev()
                    .map_while(|p| match p {
                        Part::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                tail.reverse();
                let tail = tail.concat();
                if tail.contains('/') {
                    tail.rsplit('/')
                        .next()
                        .filter(|n| !n.is_empty())
                        .map(str::to_string)
                } else {
                    literal(parts).and_then(|s| physical(kind, &s))
                }
            } else {
                literal(parts).and_then(|s| physical(kind, &s))
            };
            Some(Name {
                env: env.filter(|_| literal.is_none()),
                literal,
            })
        }
        Some(Value::Call(call)) if code.pack().extensions().contains(&"go") => Some(Name {
            literal: None,
            env: go::environment(call),
        }),
        _ => Some(Name::default()),
    }
}

/// Normalize names by the same type-table forms as their provisioners. A structured name of
/// another service stays unresolved, rather than becoming a plausible physical name.
fn physical(kind: &str, text: &str) -> Option<String> {
    let ty = match kind {
        "sqs" => "aws_sqs_queue",
        "sns" => "aws_sns_topic",
        "eventbridge" => "aws_cloudwatch_event_bus",
        "kinesis" => "aws_kinesis_stream",
        "s3" => "aws_s3_bucket",
        "dynamodb" => "aws_dynamodb_table",
        "ssm" => "aws_ssm_parameter",
        "secretsmanager" => "aws_secretsmanager_secret",
        _ => return None,
    };
    let read = crate::iac::types::entry("terraform", ty)?.read(text);
    (!read.is_empty() && !read.starts_with("arn:") && !read.contains("://"))
        .then(|| read.to_string())
}
