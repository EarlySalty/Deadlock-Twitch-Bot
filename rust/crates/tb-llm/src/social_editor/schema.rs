use jsonschema::{Draft, Retrieve, Uri, Validator};
use serde_json::Value;

use super::{EditorError, ErrorKind};

struct NoExternalReferences;

impl Retrieve for NoExternalReferences {
    fn retrieve(
        &self,
        _uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err(std::io::Error::other("External schemas are disabled").into())
    }
}

fn supported(schema: &Value, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    let Some(fields) = schema.as_object() else {
        return false;
    };
    for (key, value) in fields {
        match key.as_str() {
            "properties" => {
                let Some(properties) = value.as_object() else {
                    return false;
                };
                if !properties.values().all(|value| supported(value, depth + 1)) {
                    return false;
                }
            }
            "items" => {
                if !supported(value, depth + 1) {
                    return false;
                }
            }
            "additionalProperties" => {
                if value != false {
                    return false;
                }
            }
            "type" | "required" | "enum" | "const" | "minimum" | "maximum" | "exclusiveMinimum"
            | "exclusiveMaximum" | "multipleOf" | "minLength" | "maxLength" | "minItems"
            | "maxItems" | "minProperties" | "maxProperties" | "description" | "title" => (),
            _ => return false,
        }
    }
    let object_type = fields.get("type").and_then(Value::as_str) == Some("object")
        || fields
            .get("type")
            .and_then(Value::as_array)
            .is_some_and(|types| types.iter().any(|kind| kind.as_str() == Some("object")));
    !(object_type || fields.contains_key("properties"))
        || fields.get("additionalProperties") == Some(&Value::Bool(false))
}

pub(super) fn compile(schema: &Value) -> Result<Validator, EditorError> {
    if schema.get("type").and_then(Value::as_str) != Some("object") || !supported(schema, 0) {
        return Err(EditorError::new(ErrorKind::InvalidRequest));
    }
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_retriever(NoExternalReferences)
        .build(schema)
        .map_err(|_| EditorError::new(ErrorKind::InvalidRequest))
}

const WIRE_UNSUPPORTED: [&str; 10] = [
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "maxItems",
    "minProperties",
    "maxProperties",
];

pub(super) fn wire(schema: &Value) -> Value {
    let Some(fields) = schema.as_object() else {
        return schema.clone();
    };
    let mut out = serde_json::Map::new();
    let mut stripped = serde_json::Map::new();
    for (key, value) in fields {
        match key.as_str() {
            "properties" => {
                let properties = value
                    .as_object()
                    .map(|properties| {
                        properties
                            .iter()
                            .map(|(name, child)| (name.clone(), wire(child)))
                            .collect::<serde_json::Map<_, _>>()
                    })
                    .map_or_else(|| value.clone(), Value::Object);
                out.insert(key.clone(), properties);
            }
            "items" => {
                out.insert(key.clone(), wire(value));
            }
            "minItems" if value.as_u64().is_some_and(|n| n > 1) => {
                stripped.insert(key.clone(), value.clone());
            }
            name if WIRE_UNSUPPORTED.contains(&name) => {
                stripped.insert(key.clone(), value.clone());
            }
            _ => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    if !stripped.is_empty() {
        let hint = Value::Object(stripped).to_string();
        let description = match out.get("description").and_then(Value::as_str) {
            Some(text) => format!("{text}\n\n{hint}"),
            None => hint,
        };
        out.insert("description".into(), Value::String(description));
    }
    Value::Object(out)
}

const MAX_OPTIONAL_PARAMETERS: usize = 24;
const MAX_UNION_PARAMETERS: usize = 16;

#[derive(Default)]
struct WireShape {
    optional: usize,
    unions: usize,
}

fn scalar(value: &Value) -> bool {
    value.is_string() || value.is_number() || value.is_boolean() || value.is_null()
}

fn strings(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.iter().all(Value::is_string))
}

fn optional_count(fields: &serde_json::Map<String, Value>) -> usize {
    let Some(properties) = fields.get("properties").and_then(Value::as_object) else {
        return 0;
    };
    let required = fields.get("required").and_then(Value::as_array);
    properties
        .keys()
        .filter(|name| {
            !required.is_some_and(|names| names.iter().any(|v| v.as_str() == Some(name.as_str())))
        })
        .count()
}

fn wire_shape(schema: &Value, shape: &mut WireShape) -> bool {
    let Some(fields) = schema.as_object() else {
        return false;
    };
    let valid = fields.iter().all(|(key, value)| match key.as_str() {
        "properties" => value
            .as_object()
            .is_some_and(|properties| properties.values().all(|child| wire_shape(child, shape))),
        "items" => wire_shape(value, shape),
        "additionalProperties" => value == false,
        "minItems" => value.as_u64().is_some_and(|n| n <= 1),
        "enum" => value
            .as_array()
            .is_some_and(|values| !values.is_empty() && values.iter().all(scalar)),
        "const" => scalar(value),
        "type" => value.is_string() || strings(value),
        "required" => strings(value),
        "description" | "title" => value.is_string(),
        _ => false,
    });
    if fields.get("type").is_some_and(Value::is_array) {
        shape.unions += 1;
    }
    shape.optional += optional_count(fields);
    valid
}

pub(super) fn conformant_wire(schema: &Value) -> Result<Value, EditorError> {
    let wire = wire(schema);
    let mut shape = WireShape::default();
    if wire_shape(&wire, &mut shape)
        && shape.optional <= MAX_OPTIONAL_PARAMETERS
        && shape.unions <= MAX_UNION_PARAMETERS
    {
        Ok(wire)
    } else {
        Err(EditorError::new(ErrorKind::InvalidRequest))
    }
}
