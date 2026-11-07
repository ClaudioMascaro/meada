//! Azure identity policy shared by config, IaC, environment and SDK readers (#669).
//! A namespace/account selector is discarded only after validating the whole resource shape.

use std::sync::LazyLock;

pub(crate) mod sdk;

/// Bound splitting and validation before allocating resource-name components.
// ponytail: ARM identifiers fit within 1 KiB; longer source strings remain unresolved.
const AZURE_IDENTIFIER_MAX: usize = 1024;
// ponytail: Azure resource group selectors contain at most 90 Unicode characters.
const RESOURCE_GROUP_NAME_MAX: usize = 90;
// ponytail: Azure's queue/topic name ceiling, before projection.
const SERVICEBUS_ENTITY_MAX: usize = 260;
// ponytail: a subscription is a single topic-scoped Azure name of at most 50 characters.
const SERVICEBUS_SUBSCRIPTION_MAX: usize = 50;
// ponytail: Azure Event Hub entity names are at most 256 characters.
const EVENTHUB_NAME_MAX: usize = 256;
// ponytail: Azure storage account names have at least three characters.
const STORAGE_ACCOUNT_NAME_MIN: usize = 3;
// ponytail: Azure storage account names have at most 24 characters.
const STORAGE_ACCOUNT_NAME_MAX: usize = 24;
// ponytail: Azure blob container names have at least three characters.
const BLOB_CONTAINER_NAME_MIN: usize = 3;
// ponytail: Azure blob container names have at most 63 characters.
const BLOB_CONTAINER_NAME_MAX: usize = 63;
// ponytail: Azure Key Vault secret names are at most 127 characters.
const KEYVAULT_SECRET_MAX: usize = 127;

// The platform permits these Unicode categories, rather than every Unicode numeric character.
static RESOURCE_GROUP_NAME: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[\p{L}\p{Nd}_().-]+\z").unwrap());

fn bounded(text: &str) -> bool {
    !text.is_empty() && text.len() <= AZURE_IDENTIFIER_MAX
}

fn entity(text: &str, slash: bool) -> bool {
    bounded(text)
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b) || (slash && b == b'/'))
        && text
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

fn resource_group(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= RESOURCE_GROUP_NAME_MAX
        && !text.ends_with('.')
        && RESOURCE_GROUP_NAME.is_match(text)
}

fn arm(text: &str) -> Option<(&str, Vec<&str>)> {
    if !bounded(text) {
        return None;
    }
    let parts: Vec<_> = text.strip_prefix('/')?.split('/').collect();
    if parts.len() < 8
        || !parts[0].eq_ignore_ascii_case("subscriptions")
        || !parts[2].eq_ignore_ascii_case("resourceGroups")
        || !parts[4].eq_ignore_ascii_case("providers")
        || !entity(parts[1], false)
        || !resource_group(parts[3])
        || parts.iter().any(|p| p.is_empty())
    {
        return None;
    }
    Some((parts[5], parts[6..].to_vec()))
}

/// Plain queue/topic names retain valid slashes. Subscription markers are never plain names.
pub(crate) fn servicebus(text: &str) -> Option<String> {
    if text.starts_with('/') {
        let (provider, parts) = arm(text)?;
        if !provider.eq_ignore_ascii_case("Microsoft.ServiceBus")
            || parts.len() < 4
            || !parts[0].eq_ignore_ascii_case("namespaces")
            || !entity(parts[1], false)
            || !(parts[2].eq_ignore_ascii_case("queues") || parts[2].eq_ignore_ascii_case("topics"))
        {
            return None;
        }
        // A topics/.../subscriptions/... identifier names a subscription, not a topic.
        if parts[2].eq_ignore_ascii_case("topics")
            && parts[3..]
                .iter()
                .any(|p| p.eq_ignore_ascii_case("subscriptions"))
        {
            return None;
        }
        let name = parts[3..].join("/");
        return (name.len() <= SERVICEBUS_ENTITY_MAX && entity(&name, true)).then_some(name);
    }
    (text.len() <= SERVICEBUS_ENTITY_MAX && entity(text, true)).then(|| text.to_string())
}

pub(crate) fn subscription_target(topic: &str, subscription: &str) -> Option<String> {
    (topic.len() <= SERVICEBUS_ENTITY_MAX
        && subscription.len() <= SERVICEBUS_SUBSCRIPTION_MAX
        && entity(topic, true)
        && entity(subscription, false))
    .then(|| format!("subscription:{topic}/{subscription}"))
}

/// A typed subscription is topic-qualified; an ARM subscription retains both names too.
pub(crate) fn subscription(text: &str) -> Option<String> {
    if !bounded(text) {
        return None;
    }
    if text.starts_with('/') {
        let (provider, parts) = arm(text)?;
        if !provider.eq_ignore_ascii_case("Microsoft.ServiceBus")
            || parts.len() < 6
            || !parts[0].eq_ignore_ascii_case("namespaces")
            || !entity(parts[1], false)
            || !parts[2].eq_ignore_ascii_case("topics")
            || !parts[parts.len() - 2].eq_ignore_ascii_case("subscriptions")
        {
            return None;
        }
        return subscription_target(&parts[3..parts.len() - 2].join("/"), parts[parts.len() - 1]);
    }
    let (topic, sub) = text.rsplit_once('/')?;
    subscription_target(topic, sub)
}

fn simple_arm(text: &str, provider: &str, labels: &[&str]) -> Option<String> {
    let (found, parts) = arm(text)?;
    if !found.eq_ignore_ascii_case(provider) || parts.len() != labels.len() * 2 {
        return None;
    }
    for (i, label) in labels.iter().enumerate() {
        if !parts[i * 2].eq_ignore_ascii_case(label) || !entity(parts[i * 2 + 1], false) {
            return None;
        }
    }
    Some(parts.last()?.to_string())
}

pub(crate) fn eventhub(text: &str) -> Option<String> {
    let name = if text.starts_with('/') {
        simple_arm(text, "Microsoft.EventHub", &["namespaces", "eventhubs"])
    } else {
        entity(text, false).then(|| text.to_string())
    }?;
    (name.len() <= EVENTHUB_NAME_MAX).then_some(name)
}

pub(crate) fn account(text: &str) -> Option<String> {
    if !bounded(text) {
        return None;
    }
    let name = if text.starts_with('/') {
        simple_arm(text, "Microsoft.Storage", &["storageAccounts"])?
    } else {
        text.into()
    };
    ((STORAGE_ACCOUNT_NAME_MIN..=STORAGE_ACCOUNT_NAME_MAX).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
    .then_some(name)
}

fn url_path<'a>(text: &'a str, suffix: &str) -> Option<(&'a str, &'a str)> {
    if !bounded(text) {
        return None;
    }
    let (host, path) = text.strip_prefix("https://")?.split_once('/')?;
    let selector = host.strip_suffix(suffix)?;
    (entity(selector, false)
        && selector
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !selector.starts_with('-')
        && !selector.ends_with('-'))
    .then_some((selector, path))
}

pub(crate) fn container(text: &str) -> Option<String> {
    if !bounded(text) {
        return None;
    }
    let name = if text.starts_with('/') {
        let (provider, parts) = arm(text)?;
        if !provider.eq_ignore_ascii_case("Microsoft.Storage")
            || parts.len() != 6
            || !parts[0].eq_ignore_ascii_case("storageAccounts")
            || account(parts[1]).is_none()
            || !parts[2].eq_ignore_ascii_case("blobServices")
            || !parts[3].eq_ignore_ascii_case("default")
            || !parts[4].eq_ignore_ascii_case("containers")
        {
            return None;
        }
        parts[5].to_string()
    } else if text.starts_with("https://") {
        let (selector, path) = url_path(text, ".blob.core.windows.net")?;
        account(selector)?;
        path.split('/').next()?.to_string()
    } else {
        text.to_string()
    };
    ((BLOB_CONTAINER_NAME_MIN..=BLOB_CONTAINER_NAME_MAX).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--"))
    .then_some(name)
}

pub(crate) fn secret(text: &str) -> Option<String> {
    if !bounded(text) {
        return None;
    }
    let name = if text.starts_with('/') {
        simple_arm(text, "Microsoft.KeyVault", &["vaults", "secrets"])?
    } else if text.starts_with("https://") {
        let (_, path) = url_path(text, ".vault.azure.net")?;
        let parts: Vec<_> = path.split('/').collect();
        if !(parts.len() == 2 || parts.len() == 3)
            || parts[0] != "secrets"
            || parts.iter().any(|p| !entity(p, false))
        {
            return None;
        }
        parts[1].to_string()
    } else {
        text.into()
    };
    (name.len() <= KEYVAULT_SECRET_MAX
        && bounded(&name)
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    .then_some(name)
}

pub(crate) fn identified(text: &str) -> Option<String> {
    if text.starts_with('/') {
        let (provider, parts) = arm(text)?;
        if provider.eq_ignore_ascii_case("Microsoft.ServiceBus") {
            let name = if parts.len() >= 6
                && parts[2].eq_ignore_ascii_case("topics")
                && parts[parts.len() - 2].eq_ignore_ascii_case("subscriptions")
            {
                subscription(text)?
            } else {
                servicebus(text)?
            };
            return Some(format!("channel::servicebus::{name}"));
        }
        if provider.eq_ignore_ascii_case("Microsoft.EventHub") {
            return Some(format!("channel::eventhubs::{}", eventhub(text)?));
        }
        if provider.eq_ignore_ascii_case("Microsoft.Storage") {
            return if parts.len() == 2 {
                Some(format!("resource::azurestorage::{}", account(text)?))
            } else {
                Some(format!("resource::azureblob::{}", container(text)?))
            };
        }
        if provider.eq_ignore_ascii_case("Microsoft.KeyVault") {
            return Some(format!("resource::keyvault::{}", secret(text)?));
        }
    }
    if text.starts_with("https://") {
        if let Some(name) = container(text) {
            return Some(format!("resource::azureblob::{name}"));
        }
        if let Some(name) = secret(text) {
            return Some(format!("resource::keyvault::{name}"));
        }
    }
    None
}

pub(crate) fn typed(alias: &str, text: &str) -> Option<String> {
    let (prefix, name) = match alias {
        "servicebus_queue" | "servicebus_topic" => ("channel::servicebus", servicebus(text)?),
        "servicebus_subscription" => ("channel::servicebus", subscription(text)?),
        "eventhubs_hub" => ("channel::eventhubs", eventhub(text)?),
        "azurestorage_account" => ("resource::azurestorage", account(text)?),
        "azureblob_container" => ("resource::azureblob", container(text)?),
        "keyvault_secret" => ("resource::keyvault", secret(text)?),
        _ => return None,
    };
    Some(format!("{prefix}::{name}"))
}

/// App settings read secret contents at deployment time. Keep a readable reference only.
pub(crate) fn keyvault_reference(text: &str) -> Option<String> {
    let rest = text.strip_prefix("@Microsoft.KeyVault(")?;
    let name = bounded(text)
        .then_some(rest)
        .and_then(|rest| rest.strip_suffix(')'))
        .and_then(|rest| rest.strip_prefix("SecretUri="))
        .and_then(secret);
    Some(name.map_or_else(
        || "Key Vault reference (redacted)".into(),
        |name| format!("Key Vault secret {name} (redacted)"),
    ))
}

pub(crate) const ALIASES: &[&str] = &[
    "servicebus_queue",
    "servicebus_topic",
    "servicebus_subscription",
    "eventhubs_hub",
    "azurestorage_account",
    "azureblob_container",
    "keyvault_secret",
];

/// Patterns retain physical names; unknown selectors never become structured exact IDs.
pub(crate) fn pattern(ty: &str, text: &str) -> Option<String> {
    if !bounded(text)
        || text.starts_with('/')
        || text.contains("://")
        || !crate::iac::pattern::supported(text)
    {
        return None;
    }
    let literal = text.replace('*', "a");
    let valid = match ty {
        "azurerm_servicebus_queue" | "azurerm_servicebus_topic" => servicebus(&literal),
        "azurerm_servicebus_subscription" => (literal.len() <= SERVICEBUS_SUBSCRIPTION_MAX
            && entity(&literal, false))
        .then_some(literal),
        "azurerm_eventhub" => eventhub(&literal),
        "azurerm_storage_account" => account(&literal),
        "azurerm_storage_container" => container(&literal),
        "azurerm_key_vault_secret" => secret(&literal),
        _ => None,
    };
    valid.map(|_| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_identifiers_are_rejected_before_splitting() {
        let text = format!("/subscriptions/{}", "a/".repeat(AZURE_IDENTIFIER_MAX));
        assert!(identified(&text).is_none());
        let long = "x".repeat(AZURE_IDENTIFIER_MAX + 1);
        for alias in ALIASES {
            assert!(typed(alias, &long).is_none(), "{alias}");
        }
        assert!(servicebus(&"q".repeat(SERVICEBUS_ENTITY_MAX)).is_some());
        assert!(servicebus(&"q".repeat(SERVICEBUS_ENTITY_MAX + 1)).is_none());
        assert!(subscription_target("topic", &"s".repeat(SERVICEBUS_SUBSCRIPTION_MAX)).is_some());
        assert!(
            subscription_target("topic", &"s".repeat(SERVICEBUS_SUBSCRIPTION_MAX + 1)).is_none()
        );
        assert!(eventhub(&"h".repeat(EVENTHUB_NAME_MAX)).is_some());
        assert!(eventhub(&"h".repeat(EVENTHUB_NAME_MAX + 1)).is_none());
        assert!(account("aa").is_none());
        assert!(account("aaa").is_some());
        assert!(account(&"a".repeat(24)).is_some());
        assert!(account(&"a".repeat(25)).is_none());
        assert!(container("aa").is_none());
        assert!(container("aaa").is_some());
        assert!(container(&"a".repeat(63)).is_some());
        assert!(container(&"a".repeat(64)).is_none());
        assert!(secret(&"k".repeat(KEYVAULT_SECRET_MAX)).is_some());
        assert!(secret(&"k".repeat(KEYVAULT_SECRET_MAX + 1)).is_none());
    }

    #[test]
    fn subscription_markers_do_not_invent_physical_pattern_evidence() {
        for physical in ["*", "a-*", "audit*", "au*dit"] {
            assert!(pattern("azurerm_servicebus_subscription", physical).is_none());
        }
        assert_eq!(
            pattern("azurerm_servicebus_subscription", "audit-*"),
            Some("audit-*".into())
        );
    }

    #[test]
    fn resource_group_selectors_accept_only_the_platform_unicode_categories() {
        assert!(resource_group("rg(prod)"));
        assert!(resource_group("équipe_部署-١"));
        assert!(resource_group(&"界".repeat(90)));
        assert!(!resource_group(&"界".repeat(91)));
        for invalid in [
            "",
            "rg.",
            "rg#prod",
            "rg prod",
            "rg²",
            "rgⅣ",
            "rge\u{301}",
            "rg😀",
        ] {
            assert!(!resource_group(invalid), "{invalid}");
        }
    }
}
