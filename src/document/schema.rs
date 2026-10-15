//! JSON Schema to shape (spec #21): the field paths an OpenAPI schema lets a payload carry,
//! with local `$ref`s resolved and the parts Meada cannot list left open.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::qualify;
use crate::shape::{Shape, Source};

/// How many schemas reading one shape may visit. Local references can unfold a small document
/// into a huge tree; past the budget, every part not yet read is open.
const BUDGET: usize = 10_000;

/// The keywords that say something about a payload's structure. A schema with none of them
/// may be anything.
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

/// The shape `schema`, a schema of the document `doc`, gives, with `source` as its source, and
/// every external `$ref` met, whose part is open.
///
/// `properties` give fields and `items` the `[]` elements; `allOf`, `oneOf` and `anyOf` add
/// their members' fields. An object is open when its `additionalProperties` is `true` or a
/// schema, or when it lists no fields and does not forbid others. Local `$ref`s resolve; a
/// cycle, a missing target and an external reference leave their part open.
pub fn read(doc: &Value, schema: &Value, source: Source) -> (Shape, BTreeSet<String>) {
    let mut reader = Reader {
        doc,
        stack: Vec::new(),
        budget: BUDGET,
        fields: BTreeSet::new(),
        open: BTreeSet::new(),
        external: BTreeSet::new(),
    };
    reader.schema(schema, "", false);
    let shape = Shape {
        source,
        fields: reader.fields,
        open: reader.open,
    };
    (shape, reader.external)
}

struct Reader<'a> {
    doc: &'a Value,
    /// The local references being read, outermost first.
    stack: Vec<&'a str>,
    budget: usize,
    fields: BTreeSet<String>,
    open: BTreeSet<String>,
    external: BTreeSet<String>,
}

impl<'a> Reader<'a> {
    /// Reads `schema` at `path`. An `allOf` member (`member`) only narrows the others, so a
    /// member that says nothing of its structure adds nothing.
    fn schema(&mut self, schema: &'a Value, path: &str, member: bool) {
        if self.budget == 0 {
            return self.open(path);
        }
        self.budget -= 1;
        let schema = match schema {
            Value::Object(schema) => schema,
            // `true` allows anything; `false` (nothing) and malformed schemas add nothing.
            Value::Bool(true) if !member => return self.open(path),
            _ => return,
        };
        if !STRUCTURE.iter().any(|&k| schema.contains_key(k)) {
            if !member {
                self.open(path);
            }
            return;
        }
        if let Some(reference) = schema.get("$ref") {
            self.reference(reference, path);
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (name, property) in properties.into_iter().flatten() {
            if *property == Value::Bool(false) {
                continue;
            }
            let field = qualify(path, name);
            self.fields.insert(field.clone());
            self.schema(property, &field, false);
        }
        let typed = |t: &str| match schema.get("type") {
            Some(Value::String(s)) => s == t,
            Some(Value::Array(types)) => types.iter().any(|s| s == t),
            _ => false,
        };
        let elements = format!("{path}[]");
        if let Some(items) = schema.get("items") {
            self.fields.insert(elements.clone());
            self.schema(items, &elements, false);
        } else if typed("array") {
            self.fields.insert(elements.clone());
            self.open(&elements);
        }
        for key in ["allOf", "oneOf", "anyOf"] {
            let members = schema.get(key).and_then(Value::as_array);
            for member in members.into_iter().flatten() {
                self.schema(member, path, key == "allOf");
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

    /// Reads the schema `reference` points to at `path`: a local one in place, unless it is
    /// already being read (a cycle); anything else leaves `path` open.
    fn reference(&mut self, reference: &'a Value, path: &str) {
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
                self.schema(target, path, false);
                self.stack.pop();
            }
            _ => self.open(path),
        }
    }

    fn open(&mut self, path: &str) {
        self.open.insert(path.to_string());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::read;
    use crate::shape::Source;

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
            // `allOf` merges; a member that only constrains adds nothing.
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
            // Types, formats, `required` and `nullable` are not recorded.
            (
                json!({"type": "object", "required": ["id"], "properties": {
                    "id": {"type": "string", "format": "uuid", "nullable": true}}}),
                &["id"],
                &[],
                &[],
            ),
            // A cycle stops at the repeated schema, open there.
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
            // A schema that says nothing of its structure may be anything.
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

    #[test]
    fn a_schema_that_unfolds_without_end_stops_open() {
        // Each level refers to the next twice: 2^40 nodes if read in full.
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
