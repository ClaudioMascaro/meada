//! Direct workload paths are checked during indexing. Queries use only these stored facts.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub(super) struct Code {
    pub address: String,
    pub root: String,
    pub path: String,
    pub reason: String,
}

fn safe(root: &Path, path: &str) -> bool {
    if path.starts_with(['/', '~']) || path.contains(['\\', ':', '$', '*']) {
        return false;
    }
    let mut at = root.to_path_buf();
    for step in path.split('/').filter(|p| !p.is_empty() && *p != ".") {
        if step == ".." {
            return false;
        }
        at.push(step);
        if !std::fs::symlink_metadata(&at).is_ok_and(|m| !m.file_type().is_symlink()) {
            return false;
        }
    }
    at.is_dir() || at.is_file()
}

/// Only an unambiguous existing file gives a handler source. Archives are never opened.
fn module(root: &Path, base: &str, handler: &str, only: Option<&str>) -> Option<String> {
    let (module, function) = handler.rsplit_once('.')?;
    if function.is_empty()
        || !function
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let mut candidates = std::collections::BTreeSet::new();
    for module in [module.to_string(), module.replace('.', "/")] {
        let module = crate::iac::workload::local_path(base, &module)?;
        for extension in ["ts", "js", "mjs", "cjs", "py"] {
            let path = format!("{module}.{extension}");
            if only.is_none_or(|only| only == path)
                && safe(root, &path)
                && root.join(&path).is_file()
            {
                candidates.insert(path);
            }
        }
    }
    (candidates.len() == 1).then(|| candidates.pop_first().unwrap())
}

fn handler(root: &Path, node: &crate::iac::Node) -> Option<String> {
    module(
        root,
        crate::config_values::parent(&node.path),
        &node.handler.as_ref()?.text,
        None,
    )
}

/// A safe unique handler module and its member, checked only while indexing. A ZIP is opaque.
pub(crate) fn checked_handler(
    root: &Path,
    node: &crate::iac::Node,
    room: usize,
) -> Option<(String, String)> {
    let code = checked(root, node, room)?;
    let handler = node.handler.as_ref()?;
    let (_, name) = handler.text.rsplit_once('.')?;
    if handler.overridable || node.build.as_ref().is_some_and(|b| b.overridable) {
        return None;
    }
    let path = if node.ty == "Serverless::Function" {
        code.path
    } else if root.join(&code.path).is_dir() {
        module(root, &code.path, &handler.text, None)?
    } else {
        module(
            root,
            crate::config_values::parent(&code.path),
            &handler.text,
            Some(&code.path),
        )?
    };
    (path.len() + name.len() <= room).then(|| (path, name.into()))
}

pub(super) fn checked(root: &Path, node: &crate::iac::Node, room: usize) -> Option<Code> {
    let (path, source, suffix): (Cow<'_, str>, &str, &str) = match node.ty.as_str() {
        "AWS::Serverless::Function" => {
            (Cow::Borrowed(&node.build.as_ref()?.text), "SAM CodeUri", "")
        }
        "aws_lambda_function" => (
            Cow::Borrowed(&node.build.as_ref()?.text),
            "Terraform Lambda code",
            "",
        ),
        "Serverless::Function" => (
            Cow::Owned(handler(root, node)?),
            "Serverless handler",
            ".handler",
        ),
        _ => {
            if node.ty == "service" || crate::iac::types::workload_of(node)?.code.is_none() {
                return None;
            }
            (
                Cow::Borrowed(&node.build.as_ref()?.text),
                node.ty.as_str(),
                "",
            )
        }
    };
    let code = node.provisioned.as_ref().and_then(|p| p.code.as_ref());
    let suffix = code.map_or_else(
        || Cow::Borrowed(suffix),
        |code| Cow::Owned(format!(".{}", code.key)),
    );
    let reason_bytes = source.len() + 1 + node.path.len() + 1 + node.block.len() + suffix.len();
    if node.address.len() + node.root.len() + path.len() + reason_bytes > room || !safe(root, &path)
    {
        return None;
    }
    let file_only = code.is_some_and(|code| code.file)
        || crate::iac::types::workload_of(node).is_some_and(|workload| workload.code_file);
    if file_only && !root.join(path.as_ref()).is_file() {
        return None;
    }
    Some(Code {
        address: node.address.clone(),
        root: node.root.clone(),
        path: path.into_owned(),
        reason: format!("{source} {}#{}{suffix}", node.path, node.block),
    })
}
