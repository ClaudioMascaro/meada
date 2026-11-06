//! GCP Go clients, with Pub/Sub v2 and the explicit v1 compatibility forms.
use super::{Argument, Code, Frame, Handle, Role, Stage, argument, field};

pub(super) fn constructed<'a>(code: &Code<'a>, path: &'a str, index: usize) -> Option<Handle> {
    let call = code.call(path, index)?;
    let outside = code.outside(path, &call.scope, &call.callee, call.by_path)?;
    if !outside.direct {
        return None;
    }
    let stage = match outside.spelled.as_str() {
        "cloud.google.com/go/pubsub/v2.NewClient" => Stage::Bus,
        "cloud.google.com/go/pubsub.NewClient" => Stage::LegacyBus,
        "cloud.google.com/go/storage.NewClient" => Stage::Storage,
        "cloud.google.com/go/secretmanager/apiv1.NewClient" => Stage::Secret,
        _ => return None,
    };
    Some(Handle::new(stage))
}

pub(super) fn step<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    parent: Handle,
    frames: &[Frame<'a>],
) -> Option<Handle> {
    let call = code.call(path, index)?;
    let stage = match (parent.stage, crate::resolve::method(&call.callee)) {
        (Stage::Bus, "Publisher") | (Stage::LegacyBus, "Topic") => Stage::Topic,
        (Stage::Bus, "Subscriber") | (Stage::LegacyBus, "Subscription") => Stage::Subscription,
        (Stage::Storage, "Bucket") => Stage::Bucket,
        (Stage::Bucket, "Object") => return Some(Handle::named(Stage::Object, parent.name)),
        _ => return None,
    };
    Some(Handle::named(
        stage,
        argument(code, path, &call.scope, call.args.first(), frames),
    ))
}

pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "Publish" | "Receive" | "NewWriter" | "NewReader" | "AccessSecretVersion"
    )
}

pub(super) fn read<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    handle: Handle,
) -> Option<(Role, Stage, Argument)> {
    let call = code.call(path, index)?;
    Some(match (handle.stage, crate::resolve::method(&call.callee)) {
        (Stage::Topic, "Publish") => (Role::Provider, Stage::Topic, handle.name),
        (Stage::Subscription, "Receive") => (Role::Consumer, Stage::Subscription, handle.name),
        (Stage::Object, "NewWriter") => (Role::Provider, Stage::Object, handle.name),
        (Stage::Object, "NewReader") => (Role::Consumer, Stage::Object, handle.name),
        (Stage::Secret, "AccessSecretVersion") => {
            let request = call
                .args
                .get(1)
                .map(|v| code.value_within(path, &call.scope, v, &[], super::typed_name));
            (
                Role::Consumer,
                Stage::Secret,
                request
                    .as_ref()
                    .map(|v| field(code, path, v, "Name"))
                    .unwrap_or(Argument::Unresolved),
            )
        }
        _ => return None,
    })
}
