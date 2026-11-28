use std::collections::BTreeSet;
use std::ops::Bound;

use serde_json::Value;

use crate::qualify;
use crate::shape::{Shape, Source};

const BUDGET: usize = 10_000;

// ponytail: real schemas nest a few dozen levels at most; past 128 the shape is open.
const MAX_DEPTH: usize = 128;

// ponytail: one budget per document; 64 MiB holds the shapes of the largest public API
// descriptions, a few MiB each, with room to spare.
pub(super) const WORK: usize = 64 << 20;

const COST: usize = 32;

pub struct Work(usize);

impl Default for Work {
    fn default() -> Self {
        Work(WORK)
    }
}

impl Work {
    fn take(&mut self, cost: usize) -> bool {
        match self.0.checked_sub(cost) {
            Some(left) => {
                self.0 = left;
                true
            }
            None => {
                self.0 = 0;
                false
            }
        }
    }
}

const STRUCTURE: &[&str] = &[
    "type",
    "properties",
    "additionalProperties",
    "items",
    "allOf",
    "oneOf",
    "anyOf",
    "$ref",
];

fn scalar(schema: &serde_json::Map<String, Value>) -> bool {
    const SCALARS: &[&str] = &["string", "number", "integer", "boolean"];
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(types)) => types.iter().filter_map(Value::as_str).collect(),
        _ => return false,
    };
    types.iter().all(|t| SCALARS.contains(t) || *t == "null")
        && types.iter().any(|t| SCALARS.contains(t))
        && !STRUCTURE
            .iter()
            .any(|&k| k != "type" && schema.contains_key(k))
}

pub fn read(
    doc: &Value,
    schema: &Value,
    source: Source,
    work: &mut Work,
) -> (Shape, BTreeSet<String>) {
    let mut reader = Reader {
        doc,
        stack: Vec::new(),
        budget: BUDGET,
        work,
        fields: BTreeSet::new(),
        open: BTreeSet::new(),
        scalars: BTreeSet::new(),
        external: BTreeSet::new(),
    };
    reader.schema(schema, "", false, 0);
    let (fields, open) = (&reader.fields, &reader.open);
    let below = |set: &BTreeSet<String>, path: &str| {
        [".", "["].iter().any(|step| {
            let start = format!("{path}{step}");
            let next = set.range::<str, _>((Bound::Included(start.as_str()), Bound::Unbounded));
            next.into_iter()
                .next()
                .is_some_and(|p| p.starts_with(&start))
        })
    };
    let scalars = (reader.scalars.iter())
        .filter(|p| !open.contains(*p) && !below(fields, p) && !below(open, p))
        .filter(|p| !p.is_empty() || (fields.is_empty() && open.is_empty()))
        .cloned()
        .collect();
    let mut shape = Shape::new(source);
    shape.scalars = scalars;
    shape.fields = reader.fields;
    shape.open = reader.open;
    (shape, reader.external)
}

struct Reader<'a, 'w> {
    doc: &'a Value,
    stack: Vec<&'a str>,
    budget: usize,
    work: &'w mut Work,
    fields: BTreeSet<String>,
    open: BTreeSet<String>,
    scalars: BTreeSet<String>,
    external: BTreeSet<String>,
}

impl<'a> Reader<'a, '_> {
    fn schema(&mut self, schema: &'a Value, path: &str, member: bool, depth: usize) {
        if self.budget == 0 || depth >= MAX_DEPTH || !self.work.take(COST) {
            return self.open(path);
        }
        self.budget -= 1;
        let depth = depth + 1;
        let schema = match schema {
            Value::Object(schema) => schema,
            Value::Bool(true) if !member => return self.open(path),
            _ => return,
        };
        if !STRUCTURE.iter().any(|&k| schema.contains_key(k)) {
            if !member {
                self.open(path);
            }
            return;
        }
        if scalar(schema) && self.work.take(path.len()) {
            self.scalars.insert(path.to_string());
        }
        if let Some(reference) = schema.get("$ref") {
            self.reference(reference, path, depth);
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (name, property) in properties.into_iter().flatten() {
            if *property == Value::Bool(false) {
                continue;
            }
            let field = qualify(path, name);
            if !self.field(&field) {
                return self.open(path);
            }
            self.schema(property, &field, false, depth);
        }
        let typed = |t: &str| match schema.get("type") {
            Some(Value::String(s)) => s == t,
            Some(Value::Array(types)) => types.iter().any(|s| s == t),
            _ => false,
        };
        let elements = format!("{path}[]");
        if let Some(items) = schema.get("items") {
            if !self.field(&elements) {
                return self.open(path);
            }
            self.schema(items, &elements, false, depth);
        } else if typed("array") {
            if !self.field(&elements) {
                return self.open(path);
            }
            self.open(&elements);
        }
        for key in ["allOf", "oneOf", "anyOf"] {
            let members = schema.get(key).and_then(Value::as_array);
            for member in members.into_iter().flatten() {
                self.schema(member, path, key == "allOf", depth);
            }
        }
        let lists = ["properties", "allOf", "oneOf", "anyOf", "$ref"];
        match schema.get("additionalProperties") {
            Some(Value::Bool(false)) => {}
            Some(_) => self.open(path),
            None if typed("object") && !lists.iter().any(|&k| schema.contains_key(k)) => {
                self.open(path)
            }
            None => {}
        }
    }

    fn reference(&mut self, reference: &'a Value, path: &str, depth: usize) {
        let Some(reference) = reference.as_str() else {
            return self.open(path);
        };
        let Some(pointer) = reference.strip_prefix('#') else {
            self.external.insert(reference.to_string());
            return self.open(path);
        };
        match self.doc.pointer(pointer) {
            Some(target) if !self.stack.contains(&reference) => {
                self.stack.push(reference);
                self.schema(target, path, false, depth);
                self.stack.pop();
            }
            _ => self.open(path),
        }
    }

    fn field(&mut self, field: &str) -> bool {
        let paid = self.work.take(field.len());
        if paid {
            self.fields.insert(field.to_string());
        }
        paid
    }

    fn open(&mut self, path: &str) {
        self.open.insert(path.to_string());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{Work, read as read_with};
    use crate::shape::{Shape, Source};

    fn read(
        doc: &Value,
        schema: &Value,
        source: Source,
    ) -> (Shape, std::collections::BTreeSet<String>) {
        read_with(doc, schema, source, &mut Work::default())
    }

    fn components() -> Value {
        json!({
            "components": {
                "schemas": {
                    "Base": {"type": "object", "properties": {"id": {"type": "string"}}},
                    "Wrapper": {"$ref": "#/components/schemas/Base"},
                    "Node": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "children": {
                                "type": "array",
                                "items": {"$ref": "#/components/schemas/Node"}
                            }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn primitive_types_are_scalar_leaves() {
        let schema = json!({"type": "array", "items": {"type": "object", "properties": {
            "id": {"type": "string"},
            "count": {"type": ["integer", "null"]},
            "paid": {"type": "boolean"},
            "tags": {"type": "array", "items": {"type": "number"}},
            "either": {"oneOf": [
                {"type": "string"},
                {"type": "object", "properties": {"code": {"type": "string"}}}
            ]},
            "any": {},
            "nothing": {"type": "null"}
        }}});
        let (shape, _) = read(&json!({}), &schema, Source::Document("o".into()));
        let scalars: Vec<&str> = shape.scalars.iter().map(String::as_str).collect();
        assert_eq!(
            scalars,
            [
                "[].count",
                "[].either.code",
                "[].id",
                "[].paid",
                "[].tags[]"
            ]
        );
        let (root, _) = read(
            &json!({}),
            &json!({"type": "string"}),
            Source::Document("o".into()),
        );
        assert_eq!(root.scalars.iter().collect::<Vec<_>>(), [""]);
    }

    #[test]
    fn a_schema_gives_its_field_paths_and_open_parts() {
        let doc = components();
        let base = "#/components/schemas/Base";
        for (schema, fields, open, external) in [
            (
                json!({"type": "object", "properties": {
                    "id": {"type": "string"}, "total": {"type": "number"}}}),
                &["id", "total"][..],
                &[][..],
                &[][..],
            ),
            (
                json!({"type": "array", "items": {
                    "type": "object", "properties": {"id": {"type": "integer"}}}}),
                &["[]", "[].id"],
                &[],
                &[],
            ),
            (
                json!({"properties": {"items": {"type": "array", "items": {
                    "properties": {"sku": {"type": "string"}}}}}}),
                &["items", "items[]", "items[].sku"],
                &[],
                &[],
            ),
            (
                json!({"allOf": [
                    {"$ref": base},
                    {"properties": {"total": {"type": "number"}}},
                    {"required": ["id"]}]}),
                &["id", "total"],
                &[],
                &[],
            ),
            (
                json!({"oneOf": [
                    {"properties": {"card": {"type": "string"}}},
                    {"properties": {"iban": {"type": "string"}}}]}),
                &["card", "iban"],
                &[],
                &[],
            ),
            (
                json!({"anyOf": [
                    {"properties": {"card": {"type": "string"}}},
                    {"properties": {"iban": {"type": "string"}}}]}),
                &["card", "iban"],
                &[],
                &[],
            ),
            (
                json!({"properties": {"id": {"type": "string"}}, "additionalProperties": true}),
                &["id"],
                &[""],
                &[],
            ),
            (
                json!({"type": "object", "additionalProperties": {"type": "string"}}),
                &[],
                &[""],
                &[],
            ),
            (
                json!({"properties": {"meta": {"type": "object"}}}),
                &["meta"],
                &["meta"],
                &[],
            ),
            (
                json!({"type": "object", "additionalProperties": false}),
                &[],
                &[],
                &[],
            ),
            (
                json!({"type": "object", "required": ["id"], "properties": {
                    "id": {"type": "string", "format": "uuid", "nullable": true}}}),
                &["id"],
                &[],
                &[],
            ),
            (
                json!({"$ref": "#/components/schemas/Node"}),
                &["children", "children[]", "name"],
                &["children[]"],
                &[],
            ),
            (
                json!({"$ref": "#/components/schemas/Wrapper"}),
                &["id"],
                &[],
                &[],
            ),
            (
                json!({"properties": {"customer": {"$ref": "customer.json#/Customer"}}}),
                &["customer"],
                &["customer"],
                &["customer.json#/Customer"],
            ),
            (
                json!({"$ref": "#/components/schemas/Gone"}),
                &[],
                &[""],
                &[],
            ),
            (json!({"description": "anything"}), &[], &[""], &[]),
            (json!({"type": ["object", "null"]}), &[], &[""], &[]),
            (
                json!({"properties": {"tags": {"type": "array"}}}),
                &["tags", "tags[]"],
                &["tags[]"],
                &[],
            ),
            (json!({"properties": {"x": true}}), &["x"], &["x"], &[]),
            (
                json!({"properties": {"name": {"type": "string"}, "gone": false}}),
                &["name"],
                &[],
                &[],
            ),
        ] {
            let (shape, refs) = read(&doc, &schema, Source::Document("openapi.yaml".into()));
            let got =
                |set: &std::collections::BTreeSet<String>| set.iter().cloned().collect::<Vec<_>>();
            assert_eq!(got(&shape.fields), fields, "fields of {schema}");
            assert_eq!(got(&shape.open), open, "open parts of {schema}");
            assert_eq!(got(&refs), external, "external refs of {schema}");
        }
    }

    fn chain(n: usize, step: impl Fn(Value) -> Value) -> Value {
        let mut schemas = serde_json::Map::new();
        for i in 0..n {
            let next = json!({"$ref": format!("#/components/schemas/S{}", i + 1)});
            schemas.insert(format!("S{i}"), step(next));
        }
        schemas.insert(format!("S{n}"), json!({"type": "string"}));
        json!({"components": {"schemas": schemas}})
    }

    #[test]
    fn a_schema_nested_past_the_depth_limit_is_open_there() {
        let source = || Source::Document("openapi.yaml".into());
        let start = json!({"$ref": "#/components/schemas/S0"});
        let doc = chain(10_000, |next| next);
        let (shape, _) = read(&doc, &start, source());
        assert!(shape.fields.is_empty());
        assert_eq!(shape.open.iter().collect::<Vec<_>>(), [""]);
        let doc = chain(10_000, |next| json!({"properties": {"a": next}}));
        let (shape, _) = read(&doc, &start, source());
        let deepest = vec!["a"; 64].join(".");
        assert_eq!(shape.fields.len(), 64);
        assert!(shape.fields.contains(&deepest));
        assert_eq!(shape.open.iter().collect::<Vec<_>>(), [&deepest]);
    }

    #[test]
    fn a_document_reads_its_shapes_within_one_work_budget() {
        let fields: serde_json::Map<String, Value> = (0..5_000)
            .map(|n| (format!("{n:01024}"), json!({"type": "string"})))
            .collect();
        let doc = json!({"components": {"schemas": {"Wide": {"properties": fields}}}});
        let schema = json!({"$ref": "#/components/schemas/Wide"});
        let mut work = Work::default();
        let mut read_in_full = 0;
        let mut text = 0;
        for _ in 0..100 {
            let (shape, _) = read_with(&doc, &schema, Source::Document("x".into()), &mut work);
            read_in_full += usize::from(shape.open.is_empty());
            text += shape.fields.iter().map(String::len).sum::<usize>();
        }
        assert!(read_in_full > 0 && read_in_full < 100, "{read_in_full}");
        assert!(text <= super::WORK, "{text}");
    }

    #[test]
    fn a_schema_that_unfolds_without_end_stops_open() {
        let mut schemas = serde_json::Map::new();
        for n in 0..40 {
            let next = json!({"$ref": format!("#/components/schemas/S{}", n + 1)});
            schemas.insert(
                format!("S{n}"),
                json!({"properties": {"a": next.clone(), "b": next}}),
            );
        }
        schemas.insert("S40".into(), json!({"type": "string"}));
        let doc = json!({"components": {"schemas": schemas}});
        let schema = json!({"$ref": "#/components/schemas/S0"});

        let (shape, _) = read(&doc, &schema, Source::Document("openapi.yaml".into()));

        assert!(!shape.open.is_empty());
        assert!(shape.fields.contains("a.a.a"));
    }
}
