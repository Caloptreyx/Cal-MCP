use super::{Index, Operation, Param, search::tokenize};
use serde_json::{Map, Value};

pub const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

pub fn reference_name(schema: &Value) -> Option<&str> {
    schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/components/schemas/"))
}

fn schema_fields(schema: &Value, schemas: &Map<String, Value>) -> Vec<String> {
    let schema = match reference_name(schema) {
        Some(name) => schemas.get(name).unwrap_or(schema),
        None => schema,
    };

    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

fn first_schema(content: Option<&Value>) -> Option<(String, Value)> {
    let content = content?.as_object()?;
    let (content_type, media) = content.iter().next()?;
    Some((
        content_type.clone(),
        media.get("schema").cloned().unwrap_or(Value::Null),
    ))
}

fn parse_param(param: &Value) -> Option<Param> {
    Some(Param {
        name: param.get("name")?.as_str()?.to_string(),
        location: param.get("in")?.as_str()?.to_string(),
        required: param
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        schema: param.get("schema").cloned().unwrap_or(Value::Null),
        description: param
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn method_rank(method: &str) -> usize {
    METHODS
        .iter()
        .position(|m| m.eq_ignore_ascii_case(method))
        .unwrap_or(METHODS.len())
}

impl Index {
    pub fn new(spec: &Value) -> Self {
        let schemas = spec
            .pointer("/components/schemas")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let mut operations = Vec::new();

        if let Some(paths) = spec.get("paths").and_then(Value::as_object) {
            for (path, item) in paths {
                if !(path.starts_with("/api/client/") || path.starts_with("/api/admin/")) {
                    continue;
                }

                for method in METHODS {
                    if let Some(operation) = item.get(method) {
                        operations.push(build_operation(path, method, item, operation, &schemas));
                    }
                }
            }
        }

        operations.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| method_rank(&a.method).cmp(&method_rank(&b.method)))
        });

        Self {
            operations,
            schemas,
        }
    }
}

fn build_operation(
    path: &str,
    method: &str,
    item: &Value,
    operation: &Value,
    schemas: &Map<String, Value>,
) -> Operation {
    let params: Vec<Param> = item
        .get("parameters")
        .and_then(Value::as_array)
        .into_iter()
        .chain(operation.get("parameters").and_then(Value::as_array))
        .flatten()
        .filter_map(parse_param)
        .collect();

    let body = first_schema(operation.pointer("/requestBody/content"));
    let response = operation
        .get("responses")
        .and_then(Value::as_object)
        .and_then(|responses| {
            ["200", "201", "202"]
                .iter()
                .find_map(|status| responses.get(*status))
        })
        .and_then(|response| first_schema(response.get("content")))
        .map(|(_, schema)| schema);

    let id = operation
        .get("operationId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let body_fields = body
        .as_ref()
        .map(|(_, schema)| schema_fields(schema, schemas))
        .unwrap_or_default();
    let response_fields = response
        .as_ref()
        .map(|schema| schema_fields(schema, schemas))
        .unwrap_or_default();

    let mut fields = String::new();
    for param in &params {
        fields.push(' ');
        fields.push_str(&param.name);
        fields.push(' ');
        fields.push_str(&param.description);
    }
    for field in body_fields.iter().chain(&response_fields) {
        fields.push(' ');
        fields.push_str(field);
    }

    let path_words = tokenize(&format!("{path} {id}")).into_iter().collect();
    let field_words = tokenize(&fields).into_iter().collect();

    Operation {
        method: method.to_uppercase(),
        path: path.to_string(),
        id,
        params,
        body_type: body.as_ref().map(|(content_type, _)| content_type.clone()),
        body: body.map(|(_, schema)| schema),
        body_fields,
        response,
        path_words,
        field_words,
    }
}
