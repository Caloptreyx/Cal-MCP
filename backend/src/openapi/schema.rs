use super::{Index, Operation, index::reference_name};
use serde_json::{Value, json};

const SCHEMA_DEPTH: usize = 6;

impl Index {
    pub fn find(&self, method: &str, path: &str) -> Option<&Operation> {
        let path = path.split(['?', '#']).next().unwrap_or(path);
        let segments: Vec<&str> = path.split('/').collect();

        let mut fallback = None;
        for operation in self
            .operations
            .iter()
            .filter(|operation| operation.method.eq_ignore_ascii_case(method))
        {
            if operation.path == path {
                return Some(operation);
            }

            let template: Vec<&str> = operation.path.split('/').collect();
            if fallback.is_none()
                && template.len() == segments.len()
                && template.iter().zip(&segments).all(|(t, s)| {
                    t == s || (t.starts_with('{') && t.ends_with('}') && !s.is_empty())
                })
            {
                fallback = Some(operation);
            }
        }
        fallback
    }

    pub fn resolve(&self, schema: &Value) -> Value {
        self.resolve_inner(schema, SCHEMA_DEPTH, &mut Vec::new())
    }

    fn resolve_inner(&self, schema: &Value, depth: usize, seen: &mut Vec<String>) -> Value {
        if let Some(name) = reference_name(schema) {
            let target = self.schemas.get(name);
            if depth == 0 || seen.iter().any(|s| s == name) || target.is_none() {
                return json!({ "$ref": name });
            }

            seen.push(name.to_string());
            let resolved = self.resolve_inner(target.unwrap_or(schema), depth - 1, seen);
            seen.pop();
            return resolved;
        }

        match schema {
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(key, value)| (key.clone(), self.resolve_inner(value, depth, seen)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| self.resolve_inner(item, depth, seen))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    pub fn describe(&self, operation: &Operation) -> Value {
        let parameters: Vec<Value> = operation
            .params
            .iter()
            .map(|param| {
                json!({
                    "name": param.name,
                    "in": param.location,
                    "required": param.required,
                    "schema": self.resolve(&param.schema),
                    "description": param.description,
                })
            })
            .collect();

        let mut description = json!({
            "method": operation.method,
            "path": operation.path,
            "operationId": operation.id,
            "parameters": parameters,
        });
        if let (Some(content_type), Some(schema)) = (&operation.body_type, &operation.body) {
            description["requestBody"] = json!({
                "contentType": content_type,
                "schema": self.resolve(schema),
            });
        }
        if let Some(schema) = &operation.response {
            description["response"] = self.resolve(schema);
        }

        description
    }
}

impl Operation {
    pub fn summary_line(&self) -> String {
        let mut parts = Vec::new();
        for location in ["path", "query"] {
            let names: Vec<String> = self
                .params
                .iter()
                .filter(|param| param.location == location)
                .map(|param| {
                    if param.required && location == "query" {
                        format!("{}*", param.name)
                    } else {
                        param.name.clone()
                    }
                })
                .collect();
            if !names.is_empty() {
                parts.push(format!("{location}: {}", names.join(", ")));
            }
        }

        if let Some(content_type) = &self.body_type {
            if content_type.contains("json") && !self.body_fields.is_empty() {
                parts.push(format!("body: {}", self.body_fields.join(", ")));
            } else {
                parts.push(format!("body: {content_type}"));
            }
        }

        if parts.is_empty() {
            format!("{} {}", self.method, self.path)
        } else {
            format!("{} {}  ({})", self.method, self.path, parts.join("; "))
        }
    }
}
