//! GCP client policy. The shared SDK walk proves construction; these forms select the
//! receiver stage, operation, direction and identity-bearing argument.
use crate::contract::sdk::origins::{Evidence, Policy};
use crate::contract::{Found, Role, Site};
use crate::lang::{Value, property};
use crate::names::{self, Argument};
use crate::resolve::{Code, Frame};

mod csharp;
mod go;
mod java;
mod python;
mod ts;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Language {
    Ts,
    Python,
    Java,
    Go,
    Csharp,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Stage {
    Bus,
    LegacyBus,
    Publisher,
    Subscriber,
    TopicBuilder,
    SubscriptionBuilder,
    StorageBuilder,
    StorageOptions,
    Topic,
    Subscription,
    Storage,
    Bucket,
    Object,
    Secret,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Handle {
    stage: Stage,
    name: Argument,
}
impl Handle {
    fn new(stage: Stage) -> Self {
        Self {
            stage,
            name: Argument::Unresolved,
        }
    }
    fn named(stage: Stage, name: Argument) -> Self {
        Self { stage, name }
    }
}

struct Gcp(Language);
#[derive(Clone, Default, PartialEq, Eq, Hash)]
enum Identity {
    #[default]
    Unknown,
    Name(Argument),
    Object(Vec<(String, Identity)>),
    List(Vec<Identity>),
}

fn identity<'a>(code: &Code<'a>, path: &'a str, value: &Value) -> Identity {
    match value {
        Value::Object(fields) => Identity::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), identity(code, path, value)))
                .collect(),
        ),
        Value::List(values) => {
            Identity::List(values.iter().map(|v| identity(code, path, v)).collect())
        }
        _ => match value_argument(code, path, value) {
            Argument::Unresolved => Identity::Unknown,
            name => Identity::Name(name),
        },
    }
}

impl<'a> Policy<'a> for Gcp {
    type Evidence = Handle;
    type Argument = std::rc::Rc<Identity>;
    fn constructed(
        &self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        ty: &'a str,
        _: &'a [Value],
        _: &[Frame<'a>],
    ) -> Option<Handle> {
        match self.0 {
            Language::Ts => ts::constructed(code, path, scope, ty),
            Language::Python => None,
            Language::Java => None,
            Language::Go => None,
            Language::Csharp => None,
        }
    }
    fn made(
        &self,
        code: &Code<'a>,
        path: &'a str,
        index: usize,
        frames: &[Frame<'a>],
    ) -> Option<Handle> {
        let call = code.call(path, index)?;
        match self.0 {
            Language::Ts => None,
            Language::Python => python::constructed(code, path, &call.scope, &call.callee),
            Language::Java => java::constructed(code, path, index, frames),
            Language::Go => go::constructed(code, path, index),
            Language::Csharp => csharp::constructed(code, path, index, frames),
        }
    }
    fn step(
        &self,
        code: &Code<'a>,
        path: &'a str,
        index: usize,
        parent: Handle,
        frames: &[Frame<'a>],
    ) -> Option<Handle> {
        match self.0 {
            Language::Ts => ts::step(code, path, index, parent, frames),
            Language::Python => python::step(code, path, index, parent, frames),
            Language::Java => java::step(code, path, index, parent),
            Language::Go => go::step(code, path, index, parent, frames),
            Language::Csharp => None,
        }
    }
    fn argument(
        &self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        frames: &[Frame<'a>],
    ) -> Self::Argument {
        let value = code.value_within(path, scope, value, frames, typed_name);
        std::rc::Rc::new(identity(code, path, &value))
    }
}

fn argument<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: Option<&'a Value>,
    frames: &[Frame<'a>],
) -> Argument {
    value
        .map(|v| code.value_within(path, scope, v, frames, typed_name))
        .map(|v| value_argument(code, path, &v))
        .unwrap_or(Argument::Unresolved)
}

fn imported<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> Option<String> {
    let (package, name) = code.imported(path, scope, name)?;
    Some(format!("{package}.{name}"))
}

fn physical(stage: Stage, name: &str) -> Option<String> {
    match stage {
        Stage::Topic => super::pubsub(name, false),
        Stage::Subscription => super::pubsub(name, true),
        Stage::Bucket | Stage::Object | Stage::Storage => super::bucket(name),
        Stage::Secret => super::secret(name),
        _ => None,
    }
}

fn subscription(operation: Option<&str>) -> bool {
    matches!(
        operation,
        Some("on" | "subscribe" | "startAsync" | "Receive" | "StartAsync")
    )
}

/// A plain workload id has the subtype of its SDK operation. A structured wrong
/// collection stays unresolved instead of falling back to a plausible topic id.
pub(crate) fn environment_name(value: &str, operation: Option<&str>) -> Option<String> {
    super::pubsub(value, subscription(operation))
}

pub(crate) fn environment_target(name: &str, operation: Option<&str>) -> bool {
    let sub = subscription(operation);
    let physical = if sub {
        name.strip_prefix("subscriptions/")
    } else {
        Some(name)
    };
    physical.and_then(|s| super::pubsub(s, sub)).as_deref() == Some(name)
}

/// The stage is retained until the final operation: making a handle supplies no direction.
pub(crate) fn extract(code: &Code, channels: bool) -> Vec<Found> {
    let extensions = code.pack().extensions();
    let language = if extensions.contains(&"ts") {
        Language::Ts
    } else if extensions.contains(&"py") {
        Language::Python
    } else if extensions.contains(&"java") {
        Language::Java
    } else if extensions.contains(&"go") {
        Language::Go
    } else if extensions.contains(&"cs") {
        Language::Csharp
    } else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        let mut evidence = Evidence::new(code, Gcp(language));
        for (index, stored) in facts.calls.iter().enumerate() {
            let method = crate::resolve::method(&stored.callee);
            if !match language {
                Language::Ts => ts::operation(method),
                Language::Python => python::operation(method),
                Language::Java => java::operation(method),
                Language::Go => go::operation(method),
                Language::Csharp => csharp::operation(method),
            } {
                continue;
            }
            let Some(handle) = evidence.call(path, index) else {
                continue;
            };
            let read = match language {
                Language::Ts => ts::read(code, path, index, handle),
                Language::Python => python::read(code, path, index, handle),
                Language::Java => java::read(code, path, index, handle),
                Language::Go => go::read(code, path, index, handle),
                Language::Csharp => csharp::read(code, path, index, handle),
            };
            let Some((role, stage, name)) = read else {
                continue;
            };
            let (prefix, kind) = match stage {
                Stage::Topic | Stage::Subscription if channels => ("channel", "pubsub"),
                Stage::Bucket | Stage::Object | Stage::Storage if !channels => ("resource", "gcs"),
                Stage::Secret if !channels => ("resource", "gcpsecretmanager"),
                _ => continue,
            };
            let (name, via) = match name {
                Argument::Literal(name) => (physical(stage, &name), None),
                Argument::Environment(env) => (None, Some(format!("env:{env}"))),
                Argument::Unresolved => (None, None),
            };
            let contract = Some(match name {
                Some(name) => format!("{prefix}::{kind}::{name}"),
                None => format!("{prefix}::{kind}"),
            });
            found.push(Found {
                role,
                site: if role == Role::Consumer {
                    Site::Def {
                        path: path.into(),
                        name: stored.scope.clone(),
                    }
                } else {
                    Site::Call {
                        path: path.into(),
                        index,
                    }
                },
                contract,
                via,
                method: Some(method.into()),
                fragments: Vec::new(),
                shape: None,
            });
        }
    }
    found
}

fn field<'a>(code: &Code<'a>, path: &'a str, value: &Value, name: &str) -> Argument {
    match value {
        Value::New(_, args) if args.len() == 1 => field(code, path, &args[0], name),
        Value::Object(fields) => property(fields, name)
            .ok()
            .flatten()
            .map(|v| value_argument(code, path, v))
            .unwrap_or(Argument::Unresolved),
        _ => Argument::Unresolved,
    }
}

fn external(call: &crate::lang::Invocation, class: &str, method: &str) -> bool {
    crate::resolve::method(&call.callee) == method
        && matches!(call.receiver.as_deref(), Some(Value::External(names))
            if names::one(names, |s| s == class).is_some())
}

fn typed_name(name: &str) -> bool {
    matches!(
        name,
        "Google.Cloud.PubSub.V1.TopicName"
            | "Google.Cloud.PubSub.V1.SubscriptionName"
            | "Google.Cloud.SecretManager.V1.SecretVersionName"
    )
}

/// Only imported typed-name helpers may project an identity component.
fn value_argument<'a>(code: &Code<'a>, path: &'a str, value: &Value) -> Argument {
    if let Value::New(ty, args) = value
        && matches!(
            ty.as_str(),
            "Google.Cloud.PubSub.V1.TopicName"
                | "Google.Cloud.PubSub.V1.SubscriptionName"
                | "Google.Cloud.SecretManager.V1.SecretVersionName"
        )
        && let Some(name) = args.get(1)
    {
        return value_argument(code, path, name);
    }
    if let Value::Call(call) = value {
        for (class, constructor, method) in [
            (
                "google.cloud.pubsub_v1.PublisherClient",
                "PublisherClient",
                "topic_path",
            ),
            (
                "google.cloud.pubsub_v1.SubscriberClient",
                "SubscriberClient",
                "subscription_path",
            ),
        ] {
            let receiver = call.receiver.as_deref();
            let verified = external(call, class, method)
                || matches!(receiver, Some(Value::Call(root))
                    if external(root, "google.cloud.pubsub_v1", constructor));
            if crate::resolve::method(&call.callee) == method
                && verified
                && let Some(name) = call.args.get(1)
            {
                return value_argument(code, path, name);
            }
        }
        let named = [
            ("com.google.pubsub.v1.TopicName", 1),
            ("com.google.pubsub.v1.SubscriptionName", 1),
            ("com.google.cloud.secretmanager.v1.SecretVersionName", 1),
            ("com.google.cloud.storage.BlobId", 0),
        ]
        .iter()
        .find_map(|(class, index)| {
            external(call, class, "of")
                .then(|| call.args.get(*index))
                .flatten()
        });
        if let Some(name) = named {
            return value_argument(code, path, name);
        }
        for (class, method) in [
            ("Google.Cloud.PubSub.V1.TopicName", "FromProjectTopic"),
            (
                "Google.Cloud.PubSub.V1.SubscriptionName",
                "FromProjectSubscription",
            ),
        ] {
            if external(call, class, method)
                && let Some(name) = call.args.get(1)
            {
                return value_argument(code, path, name);
            }
        }
        if crate::resolve::method(&call.callee) == "build"
            && let Some(Value::Call(builder)) = call.receiver.as_deref()
            && external(builder, "com.google.cloud.storage.BlobInfo", "newBuilder")
            && let Some(blob) = builder.args.first()
        {
            return value_argument(code, path, blob);
        }
    }
    crate::contract::sdk::argument(code, path, value).unwrap_or(Argument::Unresolved)
}
