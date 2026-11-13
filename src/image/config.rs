//! Build configurations, read during indexing; no query reads a checkout or an environment.

use super::evaluated;

// ponytail: a file's repeated long key paths cannot expand its stored image facts without bound.
const IMAGE_STORED_MAX: usize = 4 * crate::lang::MAX_BYTES;
use crate::config_values;
use crate::document::events::{self, Step, Syntax};
use crate::iac::actions::run;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Summary {
    pub builders: Vec<Builder>,
    pub runners: Vec<Runner>,
    #[serde(default)]
    pub code: Vec<super::code::Code>,
    #[serde(skip)]
    bytes: usize,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Builder {
    pub image: String,
    pub at: String,
    pub inferred: bool,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Runner {
    pub image: String,
    pub key: String,
    pub line: usize,
}

pub(super) fn is_file(path: &str) -> bool {
    legacy_file(path) || crate::iac::is_source(path) || crate::iac::cloudformation::is_source(path)
}

fn legacy_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    named_file(path)
        || (crate::config_values::helm_values(name)
            && (name.ends_with(".yaml") || name.ends_with(".yml")))
}

/// Whether the file at `path` is, by its name alone, a GitHub workflow, a Compose or Skaffold
/// file, or a Maven or Gradle build: the image reader reads each whole. A Helm values file it
/// reads too (`legacy_file`), for its `image` key alone.
pub(crate) fn named_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    (path.starts_with(".github/workflows/") && (path.ends_with(".yml") || path.ends_with(".yaml")))
        || crate::iac::compose::is_file(path)
        || matches!(
            name,
            "skaffold.yaml" | "skaffold.yml" | "pom.xml" | "build.gradle" | "build.gradle.kts"
        )
}

fn builder(out: &mut Summary, image: &str, at: &str, inferred: bool) {
    let cost = image
        .len()
        .saturating_add(at.len().saturating_mul(2))
        .saturating_add(128);
    if out.bytes.saturating_add(cost) > IMAGE_STORED_MAX {
        return;
    }
    if let Some(image) = evaluated(image) {
        out.bytes += cost;
        out.builders.push(Builder {
            inferred: inferred || image.contains('*'),
            image,
            at: at.into(),
        });
    }
}

pub(super) fn read(
    path: &str,
    src: &[u8],
    root: &Path,
    facts: &crate::contract::ConfigFacts<'_>,
) -> Summary {
    let mut out = Summary::default();
    if src.len() > crate::lang::MAX_BYTES {
        return out;
    }
    for node in facts.nodes {
        let room = IMAGE_STORED_MAX - out.bytes;
        let codes = match super::code::checked(root, node, room) {
            Some(code) => vec![code],
            None => super::dockerfile::executions(root, node, room),
        };
        for code in codes {
            out.bytes += code.address.len() + code.root.len() + code.path.len() + code.reason.len();
            out.code.push(code);
        }
    }
    if !legacy_file(path) {
        return out;
    }
    if path.ends_with("pom.xml") {
        maven(path, src, &mut out);
        return out;
    }
    if path.ends_with("build.gradle") || path.ends_with("build.gradle.kts") {
        for image in super::jib::gradle(src) {
            builder(&mut out, &image, &format!("{path}#jib.to.image"), false);
        }
        return out;
    }
    let mut image_line = 1;
    let Ok(docs) = events::read(Syntax::of(path), src, |s| {
        if s.document == 0
            && matches!(s.path, [Step::Key(a), Step::Key(b)] if a == "image" && b == "repository")
        {
            image_line = s.line;
        }
    }) else {
        return out;
    };
    let Some(doc) = docs.first() else {
        return out;
    };
    let name = path.rsplit('/').next().unwrap_or(path);
    if path.starts_with(".github/workflows/") {
        workflow(path, doc, &mut out);
    }
    if crate::config_values::helm_values(name)
        && let Some(written) = doc.get("image")
        && let Some(repository) = written.get("repository").and_then(Value::as_str)
        && let Some(image) = evaluated(&match (written.get("registry").and_then(Value::as_str))
            .map(|registry| registry.trim_end_matches('/'))
        {
            Some(registry)
                if !registry.is_empty() && !repository.starts_with(&format!("{registry}/")) =>
            {
                // A templated registry is an unresolved segment, as `${REGISTRY}` is.
                let registry = if registry.contains("{{") {
                    "${registry}"
                } else {
                    registry
                };
                format!("{registry}/{repository}")
            }
            _ => repository.into(),
        })
    {
        out.runners.push(Runner {
            image,
            key: "image.repository".into(),
            line: image_line,
        });
    }
    if matches!(name, "skaffold.yaml" | "skaffold.yml") {
        for (document, doc) in docs.iter().enumerate() {
            let prefix = if docs.len() > 1 {
                format!("[{document}].")
            } else {
                String::new()
            };
            for (i, artifact) in doc
                .get("build")
                .and_then(|v| v.get("artifacts"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                if let Some(image) = artifact.get("image").and_then(Value::as_str) {
                    builder(
                        &mut out,
                        image,
                        &format!("{path}#{prefix}build.artifacts[{i}].image"),
                        false,
                    );
                }
            }
        }
    }
    out
}

fn workflow(path: &str, doc: &Value, out: &mut Summary) {
    for (job, value) in doc
        .get("jobs")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        for (index, step) in value
            .get("steps")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let shell = step.get("shell").or_else(|| {
                [value, doc]
                    .into_iter()
                    .find_map(|scope| scope.pointer("/defaults/run/shell"))
            });
            let known_shell = shell.is_none_or(|shell| {
                shell
                    .as_str()
                    .is_some_and(|shell| matches!(shell, "bash" | "sh"))
            });
            // The gate of the command table says which lines are commands (ADR 0028).
            let commands = (step.get("run").and_then(Value::as_str))
                .filter(|script| known_shell && script.len() <= config_values::VALUE_MAX)
                .and_then(|script| run::commands(script, |_| None));
            // --bare makes the configured destination the complete image name. Other ko
            // naming modes add import paths and hashes, so an unresolved command is absent.
            for command in commands.iter().flatten() {
                let words: Option<Vec<&str>> = command.iter().map(|w| run::literal(w)).collect();
                let Some(words) = words else {
                    continue;
                };
                if !(3..=4).contains(&words.len())
                    || words[0] != "ko"
                    || !matches!(words[1], "build" | "publish")
                    || words[2..].iter().filter(|&&word| word == "--bare").count() != 1
                    || words[2..].iter().any(|word| {
                        *word != "--bare" && (word.starts_with('-') || word.contains("..."))
                    })
                {
                    continue;
                }
                let image = [step, value, doc]
                    .into_iter()
                    .find_map(|scope| scope.get("env").and_then(|v| v.get("KO_DOCKER_REPO")));
                if let Some(image) = image.and_then(Value::as_str) {
                    let at = format!(
                        "{path}#{}",
                        config_values::key_path(&[
                            Step::Key("jobs".into()),
                            Step::Key(job.clone()),
                            Step::Key("steps".into()),
                            Step::Index(index),
                            Step::Key("run".into())
                        ])
                    );
                    builder(out, image, &at, false);
                }
            }
            let action = step
                .get("uses")
                .and_then(Value::as_str)
                .unwrap_or("")
                .split('@')
                .next()
                .unwrap_or("");
            let key = match action {
                "docker/build-push-action" => "tags",
                "docker/metadata-action" => "images",
                _ => continue,
            };
            let Some(tags) = step
                .get("with")
                .and_then(|v| v.get(key))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let steps = [
                Step::Key("jobs".into()),
                Step::Key(job.clone()),
                Step::Key("steps".into()),
                Step::Index(index),
                Step::Key("with".into()),
                Step::Key(key.into()),
            ];
            let at = format!("{path}#{}", config_values::key_path(&steps));
            if key == "tags" {
                for tag in tags.split([',', '\n']).map(str::trim) {
                    builder(out, tag, &at, false);
                }
            } else {
                for line in tags.lines().map(str::trim) {
                    let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
                    if fields.iter().skip(1).all(|f| !f.contains('='))
                        && !fields[0].starts_with("name=")
                    {
                        for image in fields {
                            builder(out, image, &at, false);
                        }
                    } else if fields.iter().skip(1).all(|f| *f == "enable=true") {
                        let image = fields[0].strip_prefix("name=").unwrap_or(fields[0]);
                        builder(out, image, &at, false);
                    }
                }
            }
        }
    }
}

fn maven(path: &str, src: &[u8], out: &mut Summary) {
    let Ok(text) = std::str::from_utf8(src) else {
        return;
    };
    if !crate::xml::within_limits(text) {
        return;
    }
    let options = roxmltree::ParsingOptions {
        nodes_limit: crate::xml::XML_NODES,
        ..Default::default()
    };
    let Ok(doc) = roxmltree::Document::parse_with_options(text, options) else {
        return;
    };
    for plugin in doc.descendants().filter(|n| n.has_tag_name("plugin")) {
        if plugin
            .ancestors()
            .any(|n| n.has_tag_name("pluginManagement") || n.has_tag_name("profiles"))
        {
            continue;
        }
        let child = |name| {
            plugin
                .children()
                .find(|n| n.has_tag_name(name))
                .and_then(|n| n.text())
        };
        if child("groupId") != Some("com.google.cloud.tools")
            || child("artifactId") != Some("jib-maven-plugin")
        {
            continue;
        }
        if let Some(image) = plugin
            .children()
            .find(|n| n.has_tag_name("configuration"))
            .and_then(|n| n.children().find(|n| n.has_tag_name("to")))
            .and_then(|n| n.children().find(|n| n.has_tag_name("image")))
            .and_then(|n| n.text())
        {
            builder(
                out,
                image,
                &format!("{path}#project.build.plugins.jib.configuration.to.image"),
                false,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_long_builder_locations_stop_at_the_image_storage_ceiling() {
        let job = "j".repeat(900);
        let tags = (0..10_000)
            .map(|i| format!("            ghcr.io/acme/image-{i}\n"))
            .collect::<String>();
        let source = format!(
            "jobs:\n  {job}:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: |\n{tags}"
        );
        assert!(source.len() < crate::lang::MAX_BYTES);
        let start = std::time::Instant::now();
        let summary = read(
            ".github/workflows/build.yaml",
            source.as_bytes(),
            Path::new("."),
            &Default::default(),
        );
        assert!(!summary.builders.is_empty());
        assert!(summary.builders.len() < 10_000);
        assert!(serde_json::to_vec(&summary).unwrap().len() <= IMAGE_STORED_MAX);
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }
}
