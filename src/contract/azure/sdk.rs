//! Azure SDK policy: known constructors and child handles retain only identity arguments.
//! The shared complete SDK walk owns exact returns, alternatives and evaluation budgets.
use crate::contract::sdk::origins::{Evidence, Policy};
use crate::contract::{EnvironmentScope, EnvironmentValue, Found, ReceiverIdentity, Role, Site};
use crate::lang::Value;
use crate::names::Argument;
use crate::resolve::{Code, Frame};

mod csharp;
mod environment;
mod go;
mod java;
mod python;
mod ts;
use environment::subscription_name;
pub(crate) use environment::{environment_identity, environment_name};

#[derive(Clone, Copy)]
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
    Sender,
    Receiver,
    Subscription,
    EventProducer,
    EventConsumer,
    EventPartition,
    Storage,
    Container,
    Blob,
    BlockBlob,
    Secret,
    BusBuilder,
    SenderBuilder,
    ReceiverBuilder,
    ReceiverQueueBuilder,
    ReceiverTopicBuilder,
    EventBuilder,
    StorageBuilder,
    SecretBuilder,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Handle {
    stage: Stage,
    name: Argument,
    subscription: Option<Argument>,
}
impl Handle {
    fn new(stage: Stage) -> Self {
        Self::named(stage, Argument::Unresolved)
    }
    fn named(stage: Stage, name: Argument) -> Self {
        Self {
            stage,
            name,
            subscription: None,
        }
    }
    fn subscription(name: Argument, subscription: Argument) -> Self {
        Self {
            stage: Stage::Subscription,
            name,
            subscription: Some(subscription),
        }
    }
    fn stage(self, stage: Stage) -> Self {
        Self { stage, ..self }
    }
}

struct Azure(Language);
impl<'a> Policy<'a> for Azure {
    type Evidence = Handle;
    type Argument = std::rc::Rc<crate::contract::sdk::Identity<Argument>>;
    fn constructed(
        &self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        ty: &'a str,
        args: &'a [Value],
        frames: &[Frame<'a>],
    ) -> Option<Handle> {
        match self.0 {
            Language::Ts => ts::constructed(code, path, scope, ty, args, frames),
            Language::Python | Language::Go => None,
            Language::Java => java::constructed(code, path, scope, ty),
            Language::Csharp => csharp::constructed(code, path, scope, ty, args, frames),
        }
    }
    fn made(
        &self,
        code: &Code<'a>,
        path: &'a str,
        index: usize,
        frames: &[Frame<'a>],
        _awaited: bool,
    ) -> Option<Handle> {
        match self.0 {
            Language::Ts => {
                let call = code.call(path, index)?;
                let known = imported(code, path, &call.scope, &call.callee)?;
                (known == "@azure/storage-blob.BlobServiceClient.fromConnectionString")
                    .then(|| Handle::new(Stage::Storage))
            }
            Language::Python => python::constructed(code, path, index, frames),
            Language::Go => go::constructed(code, path, index, frames),
            Language::Csharp => {
                let call = code.call(path, index)?;
                call.constructs
                    .then(|| {
                        csharp::constructed(
                            code,
                            path,
                            &call.scope,
                            &call.callee,
                            &call.args,
                            frames,
                        )
                    })
                    .flatten()
            }
            Language::Java => {
                let call = code.call(path, index)?;
                call.constructs
                    .then(|| java::constructed(code, path, &call.scope, &call.callee))
                    .flatten()
            }
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
            Language::Java => java::step(code, path, index, parent, frames),
            Language::Go => go::step(code, path, index, parent, frames),
            Language::Csharp => csharp::step(code, path, index, parent, frames),
        }
    }
    fn argument(
        &self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        frames: &[Frame<'a>],
        site: (&'a str, usize),
    ) -> Self::Argument {
        let value = code.value_within_at(
            path,
            scope,
            value,
            frames,
            Some(site),
            |_| false,
            crate::contract::sdk::mutations::writes,
        );
        std::rc::Rc::new(crate::contract::sdk::Identity::read(&value, &|v| {
            Some(value_argument(code, path, v)).filter(|name| *name != Argument::Unresolved)
        }))
    }
}

fn argument<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: Option<&'a Value>,
    frames: &[Frame<'a>],
    site: Option<(&'a str, usize)>,
) -> Argument {
    value
        .map(|v| {
            code.value_within_at(
                path,
                scope,
                v,
                frames,
                site,
                |_| false,
                crate::contract::sdk::mutations::writes,
            )
        })
        .map(|v| value_argument(code, path, &v))
        .unwrap_or(Argument::Unresolved)
}
fn value_argument<'a>(code: &Code<'a>, path: &'a str, value: &Value) -> Argument {
    crate::contract::sdk::argument(code, path, value).unwrap_or(Argument::Unresolved)
}
fn imported<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> Option<String> {
    let (package, name) = code.imported(path, scope, name)?;
    Some(format!("{package}.{name}"))
}
fn physical(handle: &Handle) -> Option<String> {
    let Argument::Literal(name) = &handle.name else {
        return None;
    };
    match handle.stage {
        Stage::Sender | Stage::Receiver => super::servicebus(name),
        Stage::Subscription => {
            let topic = super::servicebus(name)?;
            let Argument::Literal(subscription) = handle.subscription.as_ref()? else {
                return None;
            };
            subscription_name(&topic, subscription)
        }
        Stage::EventProducer | Stage::EventConsumer | Stage::EventPartition => {
            super::eventhub(name)
        }
        Stage::Container | Stage::Blob | Stage::BlockBlob | Stage::Storage => {
            super::container(name)
        }
        Stage::Secret => super::secret(name),
        _ => None,
    }
}

pub(crate) fn extract(code: &Code, channels: bool) -> Vec<Found> {
    let language = if code.pack().extensions().contains(&"ts") {
        Language::Ts
    } else if code.pack().extensions().contains(&"py") {
        Language::Python
    } else if code.pack().extensions().contains(&"java") {
        Language::Java
    } else if code.pack().extensions().contains(&"go") {
        Language::Go
    } else if code.pack().extensions().contains(&"cs") {
        Language::Csharp
    } else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        let mut evidence = Evidence::new(code, Azure(language));
        for (index, call) in facts.calls.iter().enumerate() {
            let method = crate::resolve::method(&call.callee);
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
            let Some((role, handle)) = read else {
                continue;
            };
            let (prefix, kind) = match handle.stage {
                Stage::Sender | Stage::Receiver | Stage::Subscription if channels => {
                    ("channel", "servicebus")
                }
                Stage::EventProducer | Stage::EventConsumer | Stage::EventPartition if channels => {
                    ("channel", "eventhubs")
                }
                Stage::Container | Stage::Blob | Stage::BlockBlob | Stage::Storage if !channels => {
                    ("resource", "azureblob")
                }
                Stage::Secret if !channels => ("resource", "keyvault"),
                _ => continue,
            };
            let name = physical(&handle);
            let via = [&handle.name]
                .into_iter()
                .chain(handle.subscription.iter())
                .find_map(|v| match v {
                    Argument::Environment(env) => Some(format!("env:{env}")),
                    _ => None,
                });
            found.push(Found {
                identity: matches!(
                    handle.stage,
                    Stage::Sender | Stage::Receiver | Stage::Subscription
                )
                .then(|| environment::identity(&handle.name, handle.subscription.as_ref())),
                role,
                site: if role == Role::Consumer && channels {
                    Site::Def {
                        path: path.into(),
                        name: call.scope.clone(),
                    }
                } else {
                    Site::Call {
                        path: path.into(),
                        index,
                    }
                },
                contract: Some(name.map_or_else(
                    || format!("{prefix}::{kind}"),
                    |n| format!("{prefix}::{kind}::{n}"),
                )),
                via,
                method: Some(method.into()),
                fragments: Vec::new(),
                shape: None,
            });
        }
    }
    found
}
