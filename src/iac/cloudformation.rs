//! CloudFormation and SAM templates as IaC nodes (#653, ADR 0025): each resource of a template is
//! a node addressed by its logical id, its type (`AWS::SQS::Queue`, `AWS::Serverless::Function`)
//! as the node's type, with the references its intrinsic functions make and its physical name
//! resolved within the template.
//!
//! A template is a YAML (with the short-form tags `!Ref`, `!Sub`, `!GetAtt`, ... read as their
//! `Fn::` maps by `events::read_keyed`) or a JSON file with a `Resources` mapping that carries
//! `AWSTemplateFormatVersion` or the SAM transform (`AWS::Serverless-2016-10-31`). Each template
//! is an evaluation unit of its own, labelled by its path as its root, so a changed template is
//! evaluated again alone.
//!
//! References, each `exact` and walked by `trace`: `Ref`, `Fn::GetAtt` (with the attribute in its
//! `via`), the variables of `Fn::Sub` and `DependsOn`, anywhere in the resource.
//!
//! Evaluation resolves the property that holds a type's physical name (`NAMED`) with `Ref`,
//! `Fn::Sub`, `Fn::GetAtt` and `Fn::Join` over the template's own values: a parameter's default
//! (marked `Overridable`, since a deployer overrides it), another resource's name where the type
//! says `Ref` or `Fn::GetAtt` gives it, and literals. A pseudo parameter (`AWS::Region`), an
//! import, any other function, a parameter with no default or with `NoEcho`, and a resource whose
//! name is generated are unresolved parts: a `*`, and the name a pattern.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::Value;

use super::{NODES_MAX, Node, Overridable, REFERENCES_MAX, ROOT_STORED_MAX, Reference, compose};
use crate::config_values;
use crate::document::events::{self, Step, Syntax};

/// The transform that makes a template a SAM one.
const SAM: &str = "AWS::Serverless-2016-10-31";

/// How deep intrinsic functions, and resources naming each other, nest in a name; a deeper one
/// is unresolved.
// ponytail: a name built from a few functions deep is real; the reader's nesting limit is 128
// and every level is a call of the evaluator, so 32 bounds the stack.
const INTRINSIC_DEPTH_MAX: usize = 32;

/// The longest physical name kept, in bytes; a name that grows past it is unresolved.
// ponytail: names are 255 bytes at most (a Secrets Manager name 512); 1 KiB leaves room for a
// pattern's wildcards, and stops a `Fn::Join` of joins from building megabytes.
const PHYSICAL_NAME_MAX: usize = 1024;

/// What the physical name of a resource type is: the property that holds it, and whether `Ref`
/// returns it (S3, DynamoDB, Lambda) rather than an ARN, a URL or an id.
// ponytail: a short table of the types the contract kinds will read; the type table (spec #642)
// replaces it with the whole of AWS, and until then a type not here has no physical name.
const NAMED: &[(&str, &str, bool)] = &[
    ("AWS::DynamoDB::GlobalTable", "TableName", true),
    ("AWS::DynamoDB::Table", "TableName", true),
    ("AWS::Events::EventBus", "Name", true),
    ("AWS::Events::Rule", "Name", false),
    ("AWS::IAM::Role", "RoleName", true),
    ("AWS::Kinesis::Stream", "Name", true),
    ("AWS::Lambda::Function", "FunctionName", true),
    ("AWS::Logs::LogGroup", "LogGroupName", true),
    ("AWS::RDS::DBInstance", "DBInstanceIdentifier", true),
    ("AWS::S3::Bucket", "BucketName", true),
    ("AWS::SecretsManager::Secret", "Name", false),
    ("AWS::Serverless::Function", "FunctionName", true),
    ("AWS::Serverless::SimpleTable", "TableName", true),
    ("AWS::SNS::Topic", "TopicName", false),
    ("AWS::SQS::Queue", "QueueName", false),
    ("AWS::SSM::Parameter", "Name", true),
    (
        "AWS::StepFunctions::StateMachine",
        "StateMachineName",
        false,
    ),
];

/// Whether the file at `path` may hold a template: a YAML or JSON one.
pub(crate) fn is_source(path: &str) -> bool {
    [".yaml", ".yml", ".json"].iter().any(|e| path.ends_with(e))
}

/// Whether the file `path` with bytes `src` is read for a template: it says
/// `AWSTemplateFormatVersion` or the SAM transform somewhere. The rest cannot hold one.
pub(crate) fn wanted(path: &str, src: &[u8]) -> bool {
    let has = |needle: &str| src.windows(needle.len()).any(|w| w == needle.as_bytes());
    is_source(path) && (has("AWSTemplateFormatVersion") || has(SAM))
}

/// Whether `doc` is a template: a mapping with `Resources` that carries `AWSTemplateFormatVersion`
/// or the SAM transform (a string, or a list holding it).
pub(crate) fn is_template(doc: &Value) -> bool {
    let Value::Object(map) = doc else {
        return false;
    };
    let sam = match map.get("Transform") {
        Some(Value::String(t)) => t == SAM,
        Some(Value::Array(list)) => list.iter().any(|t| t.as_str() == Some(SAM)),
        _ => false,
    };
    matches!(map.get("Resources"), Some(Value::Object(_)))
        && (map.contains_key("AWSTemplateFormatVersion") || sam)
}

/// Whether the parameter `name` of the template `doc` says `NoEcho`: true, or a string that reads
/// as true (`"true"`, and in any case `yes`, `on` and `y`, which CloudFormation's YAML 1.1 types
/// as booleans when written plain and this reader cannot tell from a quoted one). Its default is
/// a secret, redacted among the config values and used for no name; reading too much as true is
/// the safe direction. `false` for a document that is no template.
pub(crate) fn no_echo(doc: &Value, name: &str) -> bool {
    is_template(doc)
        && match &doc["Parameters"][name]["NoEcho"] {
            Value::Bool(b) => *b,
            Value::String(s) => ["true", "yes", "on", "y"].contains(&s.to_lowercase().as_str()),
            _ => false,
        }
}

/// A parameter of the template.
struct Param {
    /// The default as text; `None` for no default, one that is a collection, or a `NoEcho` one.
    default: Option<String>,
    /// The line of its `Default`.
    line: usize,
    /// Whether `Ref` gives no name from the default: a list parameter, or an SSM parameter value.
    list: bool,
}

/// A resource, as read.
struct Resource {
    ty: String,
    body: serde_json::Map<String, Value>,
    line: usize,
    end_line: usize,
}

/// What a name evaluates to: its text with a `*` for each unresolved part, and the parameter
/// defaults it went through.
#[derive(Clone)]
struct Part {
    text: String,
    pattern: bool,
    through: BTreeSet<Overridable>,
}

impl Part {
    fn literal(text: impl Into<String>) -> Part {
        Part {
            text: text.into(),
            pattern: false,
            through: BTreeSet::new(),
        }
    }

    fn unknown() -> Part {
        Part {
            text: "*".into(),
            pattern: true,
            through: BTreeSet::new(),
        }
    }

    /// `parts` joined by `between`; unresolved when it grows past `PHYSICAL_NAME_MAX`.
    fn join(parts: Vec<Part>, between: &str) -> Part {
        let mut out = Part::literal("");
        for (i, p) in parts.into_iter().enumerate() {
            if i > 0 {
                out.text += between;
            }
            out.text += &p.text;
            out.pattern |= p.pattern;
            out.through.extend(p.through);
            if out.text.len() > PHYSICAL_NAME_MAX {
                return Part::unknown();
            }
        }
        out
    }
}

/// The template's values the evaluation reads.
struct Eval<'t> {
    path: &'t str,
    params: BTreeMap<String, Param>,
    resources: BTreeMap<String, Resource>,
    /// The name of each resource already evaluated.
    memo: HashMap<String, Option<Part>>,
    evaluating: HashSet<String>,
}

/// The property that holds the physical name of the resource type `ty`, and whether `Ref`
/// returns it.
fn named(ty: &str) -> Option<(&'static str, bool)> {
    NAMED.iter().find(|n| n.0 == ty).map(|n| (n.1, n.2))
}

/// The text of a scalar a parameter's `Default` or a `Fn::Sub` takes: a string, number or boolean.
fn scalar_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The resource and attribute `Fn::GetAtt` names: `[Resource, Attribute]` or `Resource.Attribute`
/// (the attribute may hold dots). The attribute is `None` when it is computed (`[Resource, !Ref
/// Param]`, the one function CloudFormation lets it use): the resource is still known.
fn get_att(arg: &Value) -> Option<(&str, Option<String>)> {
    match arg {
        Value::String(s) => s.split_once('.').map(|(r, a)| (r, Some(a.to_string()))),
        Value::Array(list) => match &list[..] {
            [Value::String(r), Value::String(a)] => Some((r, Some(a.clone()))),
            [Value::String(r), Value::Object(_)] => Some((r, None)),
            _ => None,
        },
        _ => None,
    }
}

/// The text and the variable map of a `Fn::Sub` argument: a string, or `[string, {variables}]`.
fn sub_arg(arg: &Value) -> Option<(&str, Option<&serde_json::Map<String, Value>>)> {
    match arg {
        Value::String(text) => Some((text, None)),
        Value::Array(list) => match &list[..] {
            [Value::String(text), Value::Object(vars)] => Some((text, Some(vars))),
            _ => None,
        },
        _ => None,
    }
}

/// The `${...}` variables of a `Fn::Sub` string, and the text between them: `(literal, variable)`
/// pairs, the last with no variable. `${!Text}` is the literal `${Text}`.
fn sub_parts(text: &str) -> Vec<(String, Option<String>)> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut rest = text;
    while let Some(open) = rest.find("${") {
        literal.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find('}') else {
            literal.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let inner = &after[..close];
        match inner.strip_prefix('!') {
            Some(text) => {
                literal.push_str("${");
                literal.push_str(text);
                literal.push('}');
            }
            None => parts.push((std::mem::take(&mut literal), Some(inner.trim().to_string()))),
        }
        rest = &after[close + 1..];
    }
    literal.push_str(rest);
    parts.push((literal, None));
    parts
}

impl Eval<'_> {
    /// The name of the resource `id`, memoized: `None` for a type with no physical name, a
    /// resource that sets none (the name is generated), or one that is no scalar.
    fn name_of(&mut self, id: &str, depth: usize) -> Option<Part> {
        if let Some(done) = self.memo.get(id) {
            return done.clone();
        }
        if !self.evaluating.insert(id.to_string()) {
            return Some(Part::unknown());
        }
        let found = self.resources.get(id).and_then(|r| {
            let (property, _) = named(&r.ty)?;
            r.body.get("Properties")?.get(property).cloned()
        });
        let name = match found {
            Some(Value::Null | Value::Array(_)) | None => None,
            Some(value) => Some(self.eval(&value, depth)),
        };
        self.evaluating.remove(id);
        // ponytail: the result is kept even when a cycle or `INTRINSIC_DEPTH_MAX` cut it, so a
        // resource named through one gets that partial name in logical id order; evaluating again
        // would cost a call per path of a diamond of references, exponential in its depth.
        self.memo.insert(id.to_string(), name.clone());
        name
    }

    /// `Ref name`: a parameter's default, the name of a resource whose `Ref` is its name, and
    /// anything else (a pseudo parameter, an id, an ARN, a URL) unresolved.
    fn reference(&mut self, name: &str, depth: usize) -> Part {
        if let Some(p) = self.params.get(name) {
            let (Some(default), false) = (&p.default, p.list) else {
                return Part::unknown();
            };
            let mut part = Part::literal(default.clone());
            part.through.insert(Overridable {
                path: self.path.to_string(),
                line: p.line,
                variable: format!("Parameters.{name}"),
                tfvars: false,
            });
            return part;
        }
        let by_ref = (self.resources.get(name))
            .and_then(|r| named(&r.ty))
            .map(|n| n.1);
        match by_ref {
            Some(true) => self.name_of(name, depth + 1).unwrap_or_else(Part::unknown),
            _ => Part::unknown(),
        }
    }

    /// `Fn::GetAtt resource.attribute`: the resource's name when the attribute is the property
    /// that holds it, else unresolved.
    fn attribute(&mut self, resource: &str, attribute: &str, depth: usize) -> Part {
        let property = (self.resources.get(resource))
            .and_then(|r| named(&r.ty))
            .map(|n| n.0);
        match property {
            Some(p) if p == attribute => self
                .name_of(resource, depth + 1)
                .unwrap_or_else(Part::unknown),
            _ => Part::unknown(),
        }
    }

    /// The value of the expression `v` as a name.
    fn eval(&mut self, v: &Value, depth: usize) -> Part {
        if depth > INTRINSIC_DEPTH_MAX {
            return Part::unknown();
        }
        let Value::Object(map) = v else {
            return scalar_text(v).map_or_else(Part::unknown, Part::literal);
        };
        let [(function, arg)] = map.iter().collect::<Vec<_>>()[..] else {
            return Part::unknown();
        };
        match (function.as_str(), arg) {
            ("Ref", Value::String(name)) => self.reference(name, depth),
            ("Fn::GetAtt", arg) => match get_att(arg) {
                Some((resource, Some(attribute))) => self.attribute(resource, &attribute, depth),
                _ => Part::unknown(),
            },
            ("Fn::Sub", arg) => match sub_arg(arg) {
                Some((text, vars)) => self.sub(text, vars, depth),
                None => Part::unknown(),
            },
            ("Fn::Join", Value::Array(list)) => match &list[..] {
                [delimiter, Value::Array(items)] => {
                    let delimiter = self.eval(delimiter, depth + 1);
                    if delimiter.pattern {
                        return Part::unknown();
                    }
                    let parts = (items.iter())
                        .map(|i| self.eval(i, depth + 1))
                        .collect::<Vec<_>>();
                    let mut joined = Part::join(parts, &delimiter.text);
                    joined.through.extend(delimiter.through);
                    joined
                }
                _ => Part::unknown(),
            },
            _ => Part::unknown(),
        }
    }

    /// `Fn::Sub text` with the variables `vars` it defines: each `${name}` is a variable it
    /// defines, a `Ref`, or a `resource.attribute`.
    fn sub(
        &mut self,
        text: &str,
        vars: Option<&serde_json::Map<String, Value>>,
        depth: usize,
    ) -> Part {
        let mut parts = Vec::new();
        for (literal, variable) in sub_parts(text) {
            parts.push(Part::literal(literal));
            let Some(name) = variable else { continue };
            parts.push(
                match (vars.and_then(|v| v.get(&name)), name.split_once('.')) {
                    (Some(value), _) => self.eval(value, depth + 1),
                    (None, Some((resource, attribute))) => {
                        self.attribute(resource, attribute, depth)
                    }
                    (None, None) => self.reference(&name, depth),
                },
            );
        }
        Part::join(parts, "")
    }
}

/// The resources `body`'s intrinsic functions reference, with the `via` of each: `Ref`,
/// `Fn::GetAtt.<attribute>`, `Fn::Sub` and `DependsOn`; only those `known` names, never `id`.
fn referenced(
    id: &str,
    body: &serde_json::Map<String, Value>,
    known: &BTreeMap<String, Resource>,
) -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    let mut add = |target: &str, via: String| {
        if target != id && known.contains_key(target) {
            found.insert((target.to_string(), via));
        }
    };
    match body.get("DependsOn") {
        Some(Value::String(t)) => add(t, "DependsOn".into()),
        Some(Value::Array(list)) => {
            for t in list.iter().filter_map(Value::as_str) {
                add(t, "DependsOn".into());
            }
        }
        _ => {}
    }
    // An explicit stack: a template nests as deep as the reader allows.
    let mut pending: Vec<&Value> = (body.iter())
        .filter(|(k, _)| *k != "DependsOn")
        .map(|(_, v)| v)
        .collect();
    while let Some(v) = pending.pop() {
        match v {
            Value::Array(list) => pending.extend(list),
            Value::Object(map) => {
                if let [(function, arg)] = map.iter().collect::<Vec<_>>()[..] {
                    match (function.as_str(), arg) {
                        ("Ref", Value::String(name)) => add(name, "Ref".into()),
                        ("Fn::GetAtt", arg) => {
                            if let Some((resource, attribute)) = get_att(arg) {
                                let via = attribute
                                    .map_or("Fn::GetAtt".into(), |a| format!("Fn::GetAtt.{a}"));
                                add(resource, via);
                            }
                        }
                        ("Fn::Sub", arg) => {
                            let (text, vars) = sub_arg(arg).unzip();
                            let vars = vars.flatten();
                            for (_, variable) in text.map_or_else(Vec::new, sub_parts) {
                                let Some(name) = variable else { continue };
                                if vars.is_some_and(|v| v.contains_key(&name)) {
                                    continue;
                                }
                                let resource = name.split_once('.').map_or(name.as_str(), |s| s.0);
                                add(resource, "Fn::Sub".into());
                            }
                        }
                        _ => {}
                    }
                }
                pending.extend(map.values());
            }
            _ => {}
        }
    }
    found
}

/// The IaC nodes and references of the file `src` at `path`: none unless it is a template. The
/// template is its own root, labelled by `path`.
pub(super) fn evaluate(path: &str, src: &[u8]) -> (Vec<Node>, Vec<Reference>) {
    // The first and last line of the scalars under each resource, and each parameter's default's.
    let mut spans: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut defaults: BTreeMap<String, usize> = BTreeMap::new();
    let documents = events::read_keyed(Syntax::of(path), src, true, |scalar| match scalar.path {
        [Step::Key(top), Step::Key(id), rest @ ..] if scalar.document == 0 => {
            match (top.as_str(), rest) {
                ("Resources", _) => {
                    let span = spans
                        .entry(id.clone())
                        .or_insert((scalar.line, scalar.line));
                    span.0 = span.0.min(scalar.line);
                    span.1 = span.1.max(scalar.line);
                }
                ("Parameters", [Step::Key(d)]) if d == "Default" => {
                    defaults.entry(id.clone()).or_insert(scalar.line);
                }
                _ => {}
            }
        }
        _ => {}
    });
    let Ok((documents, key_lines)) = documents else {
        return (Vec::new(), Vec::new());
    };
    let Some(doc) = documents.first().filter(|d| is_template(d)) else {
        return (Vec::new(), Vec::new());
    };
    let mut params = BTreeMap::new();
    if let Some(Value::Object(declared)) = doc.get("Parameters") {
        for (name, p) in declared {
            let no_echo = no_echo(doc, name);
            let ty = p.get("Type").and_then(Value::as_str).unwrap_or("String");
            params.insert(
                name.clone(),
                Param {
                    default: p.get("Default").and_then(scalar_text).filter(|_| !no_echo),
                    line: defaults.get(name).copied().unwrap_or(1),
                    // A list, or an SSM parameter's key: `Ref` returns a list, or what the
                    // parameter store holds, not the default.
                    list: ty.starts_with("List<")
                        || ty == "CommaDelimitedList"
                        || ty.starts_with("AWS::SSM::Parameter::Value<"),
                },
            );
        }
    }
    let text = String::from_utf8_lossy(src);
    let lines: Vec<&str> = text.lines().collect();
    let key_line = compose::KeyLines::new(&lines, "Resources", &key_lines);
    let mut resources = BTreeMap::new();
    if let Some(Value::Object(declared)) = doc.get("Resources") {
        for (id, r) in declared.iter().take(NODES_MAX) {
            let Value::Object(body) = r else { continue };
            // A type is `Provider::Service::Resource` or a custom `Custom::Name`; the `::` also
            // tells these rows from the Kubernetes kinds.
            let Some(ty) = body
                .get("Type")
                .and_then(Value::as_str)
                .filter(|t| t.contains("::"))
            else {
                continue;
            };
            let (first, last) = spans.get(id).copied().unwrap_or((1, 1));
            let line = key_line.of(id, first);
            let end_line = key_line.end_of(line, last);
            resources.insert(
                id.clone(),
                Resource {
                    ty: ty.to_string(),
                    body: body.clone(),
                    line,
                    end_line,
                },
            );
        }
    }
    let mut eval = Eval {
        path,
        params,
        resources,
        memo: HashMap::new(),
        evaluating: HashSet::new(),
    };
    let ids: Vec<String> = eval.resources.keys().cloned().collect();
    let (mut nodes, mut references) = (Vec::new(), Vec::new());
    let mut room = ROOT_STORED_MAX;
    let mut take = |bytes: usize| match room.checked_sub(bytes) {
        Some(r) => {
            room = r;
            true
        }
        None => false,
    };
    for id in ids {
        let name = eval.name_of(&id, 0);
        let r = &eval.resources[&id];
        let size = id.len() + name.as_ref().map_or(0, |n| n.text.len());
        if !take(size) {
            break;
        }
        nodes.push(Node {
            address: id.clone(),
            root: path.to_string(),
            ty: r.ty.clone(),
            pattern: name.as_ref().is_some_and(|n| n.pattern),
            overridable: (name.iter())
                .flat_map(|n| n.through.iter().cloned())
                .collect(),
            name: name.map(|n| n.text),
            path: path.to_string(),
            line: r.line,
            end_line: r.end_line,
            block: config_values::key_path(&[Step::Key("Resources".into()), Step::Key(id.clone())]),
            image: None,
            build: None,
        });
        let targets = referenced(&id, &r.body, &eval.resources);
        for (target, via) in targets.into_iter().take(REFERENCES_MAX) {
            if !take(id.len() + target.len() + via.len()) {
                break;
            }
            references.push(Reference {
                root: path.to_string(),
                source: id.clone(),
                target,
                via: Some(via),
            });
        }
    }
    (nodes, references)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(path: &str, text: &str) -> (Vec<Node>, Vec<Reference>) {
        evaluate(path, text.as_bytes())
    }

    fn template(resources: &str) -> String {
        format!(
            "AWSTemplateFormatVersion: '2010-09-09'\nParameters:\n  Env:\n    Type: String\n    Default: prod\n  Secret:\n    Type: String\n    NoEcho: true\n    Default: hunter2\n  Zones:\n    Type: CommaDelimitedList\n    Default: a,b\n  Bare:\n    Type: String\nResources:\n{resources}"
        )
    }

    /// The name and whether it is a pattern of the resource `N` that `property` names.
    fn name(ty: &str, property: &str, value: &str) -> (Option<String>, bool) {
        let text = template(&format!(
            "  N:\n    Type: {ty}\n    Properties:\n      {property}: {value}\n  Other:\n    Type: AWS::S3::Bucket\n    Properties:\n      BucketName: other-bucket\n"
        ));
        let (nodes, _) = unit("t.yaml", &text);
        let n = nodes.iter().find(|n| n.address == "N").unwrap();
        (n.name.clone(), n.pattern)
    }

    fn queue(value: &str) -> (Option<String>, bool) {
        name("AWS::SQS::Queue", "QueueName", value)
    }

    fn known(text: &str) -> (Option<String>, bool) {
        (Some(text.into()), false)
    }

    fn pattern(text: &str) -> (Option<String>, bool) {
        (Some(text.into()), true)
    }

    #[test]
    fn a_template_is_a_file_with_resources_and_a_version_or_the_sam_transform() {
        let one = "Resources:\n  Q:\n    Type: AWS::SQS::Queue\n";
        for (text, nodes) in [
            (format!("AWSTemplateFormatVersion: '2010-09-09'\n{one}"), 1),
            (format!("Transform: AWS::Serverless-2016-10-31\n{one}"), 1),
            (
                format!("Transform: [AWS::Serverless-2016-10-31, Other]\n{one}"),
                1,
            ),
            (format!("Transform: AWS::Include\n{one}"), 0),
            (one.to_string(), 0),
            ("AWSTemplateFormatVersion: '2010-09-09'\n".to_string(), 0),
            ("AWSTemplateFormatVersion: [\n".to_string(), 0),
        ] {
            assert_eq!(unit("t.yaml", &text).0.len(), nodes, "{text}");
        }
        let json =
            r#"{"AWSTemplateFormatVersion": "x", "Resources": {"Q": {"Type": "AWS::SQS::Queue"}}}"#;
        assert_eq!(unit("t.json", json).0.len(), 1);
        assert!(wanted("a/t.json", json.as_bytes()) && !wanted("a/t.tf", json.as_bytes()));
        assert!(!wanted("a/t.yaml", one.as_bytes()));
        // A resource with no `Type`, or one with no `::`, is no node.
        let text = "AWSTemplateFormatVersion: x\nResources:\n  A: {}\n  B:\n    Type: pod\n  C:\n    Type: Custom::Thing\n";
        let (nodes, _) = unit("t.yaml", text);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].ty, "Custom::Thing");
    }

    #[test]
    fn a_parameters_default_names_a_resource_and_is_overridable() {
        assert_eq!(queue("!Ref Env"), known("prod"));
        assert_eq!(queue("!Sub '${Env}-jobs'"), known("prod-jobs"));
        let text = template(
            "  N:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: !Ref Env\n",
        );
        let (nodes, _) = unit("t.yaml", &text);
        let through = &nodes[0].overridable;
        assert_eq!(through.len(), 1);
        assert_eq!(
            (
                through[0].variable.as_str(),
                through[0].line,
                through[0].tfvars
            ),
            ("Parameters.Env", 5, false)
        );
        // A literal name went through nothing.
        let (nodes, _) = unit(
            "t.yaml",
            &template("  N:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: plain\n"),
        );
        assert!(nodes[0].overridable.is_empty());
    }

    #[test]
    fn no_echo_a_list_and_a_parameter_with_no_default_are_unresolved_parts() {
        assert_eq!(queue("!Ref Secret"), pattern("*"));
        assert_eq!(queue("!Sub 'x-${Secret}'"), pattern("x-*"));
        assert_eq!(queue("!Ref Zones"), pattern("*"));
        assert_eq!(queue("!Ref Bare"), pattern("*"));
        assert_eq!(queue("!Ref Missing"), pattern("*"));
        assert!(no_echo(
            &serde_json::json!({"AWSTemplateFormatVersion": "x", "Resources": {}, "Parameters": {"P": {"NoEcho": "true"}}}),
            "P"
        ));
        assert!(!no_echo(
            &serde_json::json!({"Parameters": {"P": {"NoEcho": true}}}),
            "P"
        ));
    }

    #[test]
    fn pseudo_parameters_imports_and_other_functions_are_unresolved_parts() {
        assert_eq!(queue("!Ref AWS::Region"), pattern("*"));
        assert_eq!(queue("!Sub '${AWS::StackName}-q'"), pattern("*-q"));
        assert_eq!(queue("!Sub '${AWS::AccountId}-${Env}'"), pattern("*-prod"));
        assert_eq!(queue("!ImportValue shared"), pattern("*"));
        assert_eq!(
            queue("!Sub ['${Imported}-q', {Imported: !ImportValue x}]"),
            pattern("*-q")
        );
        assert_eq!(queue("!Select [0, [a, b]]"), pattern("*"));
        assert_eq!(queue("!FindInMap [A, B, C]"), pattern("*"));
        assert_eq!(queue("!If [Cond, a, b]"), pattern("*"));
        assert_eq!(queue("{a: b, c: d}"), pattern("*"));
        // A name the template leaves out, or sets to a list or nothing, has none.
        assert_eq!(queue("[a]"), (None, false));
        assert_eq!(queue("~"), (None, false));
    }

    #[test]
    fn sub_takes_its_variables_from_its_map_a_ref_or_a_get_att_and_escapes_with_a_bang() {
        assert_eq!(queue("!Sub ['${A}-${Env}', {A: x}]"), known("x-prod"));
        assert_eq!(queue("!Sub ['${A}', {A: !Ref Env}]"), known("prod"));
        assert_eq!(queue("!Sub 'a-${!Literal}-b'"), known("a-${Literal}-b"));
        assert_eq!(queue("!Sub 'open-${unclosed'"), known("open-${unclosed"));
        assert_eq!(queue("!Sub 'no variables'"), known("no variables"));
        assert_eq!(
            queue("!Sub '${Other.BucketName}-q'"),
            known("other-bucket-q")
        );
        assert_eq!(queue("!Sub '${Other.Arn}-q'"), pattern("*-q"));
        assert_eq!(queue("!Sub '${Other}-q'"), known("other-bucket-q"));
        assert_eq!(queue("{Fn::Sub: 'long-${Env}'}"), known("long-prod"));
        assert_eq!(queue("!Sub [1]"), pattern("*"));
    }

    #[test]
    fn get_att_gives_the_name_a_type_says_and_ref_the_name_only_where_the_type_returns_it() {
        // A bucket's `Ref` is its name; a queue's is its URL, a topic's its ARN.
        assert_eq!(queue("!Ref Other"), known("other-bucket"));
        assert_eq!(queue("!GetAtt Other.Arn"), pattern("*"));
        assert_eq!(queue("!GetAtt [Other, BucketName]"), known("other-bucket"));
        assert_eq!(queue("!GetAtt Other.BucketName"), known("other-bucket"));
        assert_eq!(queue("!GetAtt Missing.BucketName"), pattern("*"));
        assert_eq!(queue("!GetAtt [Other]"), pattern("*"));
        let text = template(
            "  Q:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: jobs\n  N:\n    Type: AWS::SSM::Parameter\n    Properties:\n      Name: !Join ['', [!Ref Q, '|', !GetAtt Q.QueueName, '|', !GetAtt Q.Arn]]\n",
        );
        let (nodes, _) = unit("t.yaml", &text);
        let n = nodes.iter().find(|n| n.address == "N").unwrap();
        assert_eq!(n.name.as_deref(), Some("*|jobs|*"));
        // A type that holds its name in a `Ref` resolves through it.
        assert_eq!(
            name("AWS::SSM::Parameter", "Name", "!Ref Env"),
            known("prod")
        );
    }

    #[test]
    fn join_takes_a_resolved_delimiter_and_a_list_of_items() {
        assert_eq!(queue("!Join ['-', [a, !Ref Env, b]]"), known("a-prod-b"));
        assert_eq!(queue("!Join ['', [a, b]]"), known("ab"));
        assert_eq!(queue("!Join ['-', [a, !Ref AWS::Region]]"), pattern("a-*"));
        assert_eq!(queue("!Join [!Ref AWS::Region, [a, b]]"), pattern("*"));
        assert_eq!(queue("!Join ['-', !Ref Zones]"), pattern("*"));
        assert_eq!(
            queue("!Join ['-', [a, !Join ['.', [b, c]]]]"),
            known("a-b.c")
        );
        assert_eq!(queue("!Join [a]"), pattern("*"));
        assert_eq!(queue("!Join ['-', [1, true]]"), known("1-true"));
    }

    #[test]
    fn references_are_ref_get_att_sub_and_depends_on_wherever_they_sit() {
        let text = template(
            "  A:\n    Type: AWS::SQS::Queue\n  B:\n    Type: AWS::SQS::Queue\n  C:\n    Type: AWS::SNS::Topic\n  D:\n    Type: AWS::SNS::Topic\n  E:\n    Type: AWS::Lambda::Function\n    DependsOn: [A, Missing]\n    Properties:\n      Environment:\n        Variables:\n          P: !Ref B\n          Q: !GetAtt C.Arn\n          R: !GetAtt [D, TopicName]\n          S: !Sub ['${A.Arn} ${Local} ${AWS::Region} ${!Esc}', {Local: !Ref B}]\n          T: !Ref Env\n          U: !Ref E\n          V: !Sub '${C}'\n",
        );
        let (nodes, references) = unit("t.yaml", &text);
        assert_eq!(nodes.len(), 5);
        let shown: Vec<String> = (references.iter())
            .map(|r| {
                format!(
                    "{} -> {} via {}",
                    r.source,
                    r.target,
                    r.via.as_deref().unwrap()
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                "E -> A via DependsOn",
                "E -> A via Fn::Sub",
                "E -> B via Ref",
                "E -> C via Fn::GetAtt.Arn",
                "E -> C via Fn::Sub",
                "E -> D via Fn::GetAtt.TopicName",
            ]
        );
        assert!(references.iter().all(|r| r.root == "t.yaml"));
    }

    #[test]
    fn a_json_template_reads_the_long_forms() {
        let json = r#"{
  "AWSTemplateFormatVersion": "2010-09-09",
  "Parameters": {"N": {"Type": "String", "Default": "audit"}},
  "Resources": {
    "Q": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": {"Fn::Join": ["-", [{"Ref": "N"}, "q"]]}}},
    "T": {"Type": "AWS::SNS::Topic", "DependsOn": "Q", "Properties": {"TopicName": {"Fn::Sub": "${N}-t"}, "X": {"Fn::GetAtt": "Q.Arn"}}}
  }
}"#;
        let (nodes, references) = unit("t.json", json);
        let names: Vec<_> = nodes
            .iter()
            .map(|n| (n.address.as_str(), n.name.as_deref().unwrap(), n.line))
            .collect();
        assert_eq!(names, [("Q", "audit-q", 5), ("T", "audit-t", 6)]);
        let via: Vec<_> = references
            .iter()
            .map(|r| r.via.as_deref().unwrap())
            .collect();
        assert_eq!(via, ["DependsOn", "Fn::GetAtt.Arn"]);
    }

    #[test]
    fn a_yaml_alias_to_a_tagged_node_repeats_the_intrinsic() {
        let text = template(
            "  A:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: &n !Sub '${Env}-a'\n  B:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: *n\n",
        );
        let (nodes, _) = unit("t.yaml", &text);
        let names: Vec<_> = nodes.iter().map(|n| n.name.as_deref().unwrap()).collect();
        assert_eq!(names, ["prod-a", "prod-a"]);
    }

    #[test]
    fn a_resource_naming_itself_or_around_a_cycle_is_an_unresolved_part() {
        let text = template(
            "  A:\n    Type: AWS::S3::Bucket\n    Properties:\n      BucketName: !Ref B\n  B:\n    Type: AWS::S3::Bucket\n    Properties:\n      BucketName: !Sub '${A}-b'\n  C:\n    Type: AWS::S3::Bucket\n    Properties:\n      BucketName: !Ref C\n",
        );
        let (nodes, references) = unit("t.yaml", &text);
        let names: Vec<_> = nodes
            .iter()
            .map(|n| (n.name.as_deref().unwrap(), n.pattern))
            .collect();
        // The cycle is cut where it comes back: `A` names `B`, which names `A` as unresolved.
        assert_eq!(names, [("*-b", true), ("*-b", true), ("*", true)]);
        assert!(references.iter().all(|r| r.source != r.target));
    }

    #[test]
    fn nodes_have_the_logical_id_type_template_root_and_lines_of_their_block() {
        let text = template(
            "  A:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: q\n\n  \"B\":\n    Type: AWS::SQS::Queue\n",
        );
        let (nodes, _) = unit("stacks/t.yaml", &text);
        let shown: Vec<_> = (nodes.iter())
            .map(|n| {
                (
                    n.address.as_str(),
                    n.ty.as_str(),
                    n.root.as_str(),
                    n.path.as_str(),
                    n.line,
                    n.end_line,
                    n.block.as_str(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                (
                    "A",
                    "AWS::SQS::Queue",
                    "stacks/t.yaml",
                    "stacks/t.yaml",
                    16,
                    19,
                    "Resources.A"
                ),
                (
                    "B",
                    "AWS::SQS::Queue",
                    "stacks/t.yaml",
                    "stacks/t.yaml",
                    21,
                    22,
                    "Resources.B"
                ),
            ]
        );
    }

    #[test]
    fn a_sam_resource_type_is_a_node_like_any_other() {
        let text = "Transform: AWS::Serverless-2016-10-31\nResources:\n  F:\n    Type: AWS::Serverless::Function\n    Properties:\n      FunctionName: fn\n      Events:\n        E:\n          Type: SQS\n          Properties:\n            Queue: !GetAtt Q.Arn\n  Q:\n    Type: AWS::SQS::Queue\n  Api:\n    Type: AWS::Serverless::HttpApi\n  Layer:\n    Type: AWS::Serverless::LayerVersion\n";
        let (nodes, references) = unit("template.yaml", text);
        let types: Vec<_> = nodes
            .iter()
            .map(|n| (n.address.as_str(), n.ty.as_str()))
            .collect();
        assert_eq!(
            types,
            [
                ("Api", "AWS::Serverless::HttpApi"),
                ("F", "AWS::Serverless::Function"),
                ("Layer", "AWS::Serverless::LayerVersion"),
                ("Q", "AWS::SQS::Queue"),
            ]
        );
        assert_eq!(nodes[1].name.as_deref(), Some("fn"));
        assert_eq!(references.len(), 1);
    }

    #[test]
    fn intrinsics_nested_past_the_ceiling_are_unresolved() {
        let nest = |n: usize| "{Fn::Join: ['', [".repeat(n) + "x" + &"]]}".repeat(n);
        assert_eq!(queue(&nest(INTRINSIC_DEPTH_MAX / 2 - 1)), known("x"));
        assert_eq!(queue(&nest(INTRINSIC_DEPTH_MAX + 2)), pattern("*"));
        // Resources naming each other in a chain are cut at the same depth: `R000` names `R001`
        // and so on, and is evaluated first.
        let chain = |n: usize| -> String {
            let mut text = String::new();
            for i in 0..n {
                text += &format!(
                    "  R{i:03}:\n    Type: AWS::S3::Bucket\n    Properties:\n      BucketName: !Ref R{:03}\n",
                    i + 1
                );
            }
            text + &format!(
                "  R{n:03}:\n    Type: AWS::S3::Bucket\n    Properties:\n      BucketName: root\n"
            )
        };
        let ends = |n: usize| {
            let (nodes, _) = unit("t.yaml", &template(&chain(n)));
            (nodes[0].name.clone(), nodes[0].pattern)
        };
        assert_eq!(ends(INTRINSIC_DEPTH_MAX / 2), known("root"));
        assert_eq!(ends(INTRINSIC_DEPTH_MAX * 2), pattern("*"));
    }

    #[test]
    fn a_name_grown_past_the_ceiling_is_unresolved() {
        let long = "x".repeat(PHYSICAL_NAME_MAX / 2 + 1);
        assert_eq!(
            queue(&format!("!Join ['', [{long}, {long}]]")),
            pattern("*")
        );
        assert_eq!(
            queue(&format!("!Sub ['${{A}}${{A}}', {{A: {long}}}]")),
            pattern("*")
        );
        let short = "x".repeat(PHYSICAL_NAME_MAX / 2);
        assert_eq!(
            queue(&format!("!Join ['', [{short}, {short}]]"))
                .0
                .map(|n| n.len()),
            Some(PHYSICAL_NAME_MAX)
        );
        // A name built from many items stops as it passes the ceiling.
        let items = vec!["abcdefgh"; 100_000].join(", ");
        assert_eq!(queue(&format!("!Join ['-', [{items}]]")), pattern("*"));
    }

    #[test]
    fn a_template_past_the_node_ceiling_keeps_the_first_in_logical_id_order() {
        let resources: String = (0..NODES_MAX + 3)
            .map(|i| format!("  R{i:06}:\n    Type: AWS::SQS::Queue\n"))
            .collect();
        let (nodes, _) = unit("t.yaml", &template(&resources));
        assert_eq!(nodes.len(), NODES_MAX);
        assert_eq!(
            nodes[NODES_MAX - 1].address,
            format!("R{:06}", NODES_MAX - 1)
        );
    }

    #[test]
    fn a_resource_past_the_reference_ceiling_keeps_the_first_in_address_order() {
        let n = REFERENCES_MAX + 5;
        let mut text = String::from("  Hub:\n    Type: AWS::Lambda::Function\n    DependsOn:\n");
        for i in 0..n {
            text += &format!("      - R{i:03}\n");
        }
        for i in 0..n {
            text += &format!("  R{i:03}:\n    Type: AWS::SQS::Queue\n");
        }
        let (nodes, references) = unit("t.yaml", &template(&text));
        assert_eq!(nodes.len(), n + 1);
        let from_hub: Vec<_> = references.iter().filter(|r| r.source == "Hub").collect();
        assert_eq!(from_hub.len(), REFERENCES_MAX);
        assert_eq!(from_hub[0].target, "R000");
    }

    #[test]
    fn a_template_past_its_stored_budget_leaves_the_rest_out() {
        // A parameter's default as long as a name may be, named by a short resource each: the
        // names store far more than the template holds.
        let default = "n".repeat(PHYSICAL_NAME_MAX - 16);
        let count = ROOT_STORED_MAX / PHYSICAL_NAME_MAX + 50;
        let resources: String = (0..count)
            .map(|i| format!("  R{i:06}:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: !Ref Long\n"))
            .collect();
        let text = format!(
            "AWSTemplateFormatVersion: x\nParameters:\n  Long:\n    Default: {default}\nResources:\n{resources}"
        );
        let (nodes, _) = unit("t.yaml", &text);
        assert!(!nodes.is_empty() && nodes.len() < count);
        let stored: usize = (nodes.iter())
            .map(|n| n.address.len() + n.name.as_ref().map_or(0, String::len))
            .sum();
        assert!(stored <= ROOT_STORED_MAX);
    }

    #[test]
    fn a_file_that_does_not_parse_adds_no_node() {
        assert!(
            unit("t.yaml", "AWSTemplateFormatVersion: x\nResources: [\n")
                .0
                .is_empty()
        );
        assert!(
            unit("t.json", "{\"AWSTemplateFormatVersion\": ")
                .0
                .is_empty()
        );
        assert!(evaluate("t.yaml", &[0xff, 0xfe]).0.is_empty());
    }

    #[test]
    fn no_echo_reads_yaml_one_one_booleans_in_any_case_as_true() {
        let doc = |v: &str| {
            let text = format!(
                "AWSTemplateFormatVersion: x\nParameters:\n  P:\n    NoEcho: {v}\n    Default: d\nResources: {{}}\n"
            );
            events::first(Syntax::Yaml, text.as_bytes()).unwrap()
        };
        for v in [
            "true", "\"true\"", "\"True\"", "yes", "Yes", "on", "y", "\"ON\"",
        ] {
            assert!(no_echo(&doc(v), "P"), "{v}");
        }
        for v in ["false", "no", "\"false\"", "off", "~", "1"] {
            assert!(!no_echo(&doc(v), "P"), "{v}");
        }
    }

    #[test]
    fn an_ssm_parameter_value_and_an_events_rule_give_no_name_through_ref() {
        let text = "AWSTemplateFormatVersion: x\nParameters:\n  Key:\n    Type: AWS::SSM::Parameter::Value<String>\n    Default: /app/queue\nResources:\n  A:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: !Ref Key\n  R:\n    Type: AWS::Events::Rule\n    Properties:\n      Name: rule\n  B:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: !Ref R\n";
        let (nodes, _) = unit("t.yaml", text);
        let name = |id: &str| nodes.iter().find(|n| n.address == id).unwrap().name.clone();
        assert_eq!(name("A").as_deref(), Some("*"));
        assert_eq!(name("B").as_deref(), Some("*"));
        assert_eq!(name("R").as_deref(), Some("rule"));
    }

    #[test]
    fn a_type_outside_the_table_is_a_node_with_no_physical_name() {
        let text = "AWSTemplateFormatVersion: x\nResources:\n  X:\n    Type: AWS::Foo::Bar\n    Properties:\n      Name: x\n      QueueName: y\n";
        let (nodes, _) = unit("t.yaml", text);
        assert_eq!((nodes.len(), nodes[0].name.clone()), (1, None));
    }

    #[test]
    fn a_get_att_with_a_computed_attribute_still_references_its_resource() {
        let text = template(
            "  T:\n    Type: AWS::SQS::Queue\n  A:\n    Type: AWS::SQS::Queue\n    Properties:\n      Other: !GetAtt [T, !Ref Env]\n      Name: !GetAtt [T, !Ref Env]\n",
        );
        let (_, references) = unit("t.yaml", &text);
        let via: Vec<_> = references
            .iter()
            .map(|r| {
                (
                    r.source.as_str(),
                    r.target.as_str(),
                    r.via.as_deref().unwrap(),
                )
            })
            .collect();
        assert_eq!(via, [("A", "T", "Fn::GetAtt")]);
    }

    #[test]
    fn a_property_spelled_like_its_logical_id_does_not_move_the_nodes_lines() {
        let text = template(
            "  Other:\n    Type: AWS::S3::Bucket\n  Bucket:\n    Properties:\n      Bucket: !Ref Other\n      Policy: {}\n    Type: AWS::S3::BucketPolicy\n",
        );
        let (nodes, _) = unit("t.yaml", &text);
        let n = nodes.iter().find(|n| n.address == "Bucket").unwrap();
        // The helper's header is 15 lines, `Other` takes two: the key is line 18, its last scalar 22.
        assert_eq!((n.line, n.end_line), (18, 22));
    }

    #[test]
    fn a_json_key_whose_colon_is_on_the_next_line_still_starts_its_node() {
        let json = "{\"AWSTemplateFormatVersion\": \"x\",\n \"Resources\": {\n  \"Odd\"\n  :\n  {\"Type\": \"AWS::SQS::Queue\",\n   \"DependsOn\": \"Odd\"}}}";
        let (nodes, _) = unit("t.json", json);
        assert_eq!((nodes[0].line, nodes[0].end_line), (3, 6));
    }

    #[test]
    fn a_resources_section_that_is_an_alias_starts_at_the_key_lines_of_its_anchor() {
        let text = "AWSTemplateFormatVersion: x\nx: &r\n  A:\n    Type: AWS::SQS::Queue\n  B:\n    Type: AWS::SQS::Queue\nResources: *r\n";
        let (nodes, _) = unit("t.yaml", text);
        let at: Vec<_> = nodes
            .iter()
            .map(|n| (n.address.as_str(), n.line, n.end_line))
            .collect();
        assert_eq!(at, [("A", 3, 6), ("B", 5, 6)]);
    }
}
