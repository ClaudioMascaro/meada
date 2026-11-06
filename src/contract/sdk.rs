//! Mechanical SDK extraction shared by contract kinds: constructor evidence, folded request
//! fields and call sites. Library names and operations belong to each kind's table.

use super::{Found, Role, Site};
use crate::lang::{Part, Value, literal};
use crate::resolve::{Code, Expand};
use std::collections::{BTreeMap, BTreeSet};

mod csharp;
mod go;
mod java;
mod origins;

/// The SDK spellings of one client, supplied by its contract kind.
pub(crate) struct Client {
    pub java: &'static str,
    pub go: &'static str,
    pub csharp: &'static str,
}

/// An operation's direction and the request field naming the contract.
pub(crate) struct Operation {
    pub method: &'static str,
    pub field: &'static str,
    pub role: Role,
    /// A hierarchy query does not identify one physical resource.
    pub hierarchy: bool,
}

/// A kind's SDK client and the operations it recognizes.
pub(crate) struct Service {
    pub client: Client,
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

/// Extract the kind's SDK operations, without declarations standing in for construction.
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
            for service in services {
                let Some(operation) = service
                    .operations
                    .iter()
                    .find(|o| language.method(method, o.method))
                else {
                    continue;
                };
                if !origins.call(path, index, &service.client) {
                    continue;
                }
                let changed =
                    changed_request(code, path, index, operation.field, language, &written);
                let read = |args: &[Value], keywords: &crate::resolve::Keywords| {
                    let [Value::Call(call)] = args else {
                        return Some(vec![Name::default()]);
                    };
                    let values = request(call, keywords, operation.field, language)?;
                    values
                        .iter()
                        .map(|v| {
                            if changed {
                                Some(Name::default())
                            } else {
                                name(code, path, Some(v), service.kind, &aliases)
                            }
                        })
                        .collect::<Option<Vec<_>>>()
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
                            role: operation.role,
                            site: if prefix == "channel" && operation.role == Role::Consumer {
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

/// A named request can change after construction; inline literals cannot change before the call.
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
    let field = selector
        .split('.')
        .next()
        .unwrap_or(selector)
        .trim_end_matches("[]");
    let (request, field) = match language {
        Language::Go => (call.args.get(1), field),
        Language::Csharp => (
            call.keyword("request").or_else(|| call.args.first()),
            csharp::field_name(field),
        ),
        Language::Java => return false,
    };
    let Some(Value::Ref(_)) = request else {
        return false;
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
fn request<'v>(
    call: &'v crate::lang::Invocation,
    keywords: &'v crate::resolve::Keywords,
    path: &str,
    language: Language,
) -> Option<Vec<Value>> {
    let (head, tail) = path
        .split_once('.')
        .map_or((path, None), |(h, t)| (h, Some(t)));
    let values = match language {
        Language::Java => java::names(call, head.trim_end_matches("[]")),
        Language::Go => go::names(call, head.trim_end_matches("[]")),
        Language::Csharp => csharp::names(call, keywords, head.trim_end_matches("[]")),
    }?;
    let mut out = Vec::new();
    for value in values {
        let Some(value) = value else {
            out.push(Value::Unknown);
            continue;
        };
        if tail == Some("*") {
            out.extend(map_keys(value)?);
            continue;
        }
        let values = if head.ends_with("[]") {
            sequence(value)?
        } else {
            vec![value]
        };
        for value in values {
            let read = match tail {
                None => Some(value),
                Some(field) => match language {
                    Language::Java => java::field_of(value, field)?.first().copied().flatten(),
                    Language::Go => go::field_of(value, field)?,
                    Language::Csharp => csharp::field_of(value, field)?,
                },
            };
            out.push(read.cloned().unwrap_or(Value::Unknown));
        }
    }
    if out.is_empty() {
        out.push(Value::Unknown);
    }
    Some(out)
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
