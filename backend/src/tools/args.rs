use axum::http::Method;
use serde_json::{Map, Value};

pub fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("`{key}` is required and must be a non-empty string"))
}

pub fn optional_text<'a>(
    args: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(format!("`{key}` must be a string")),
    }
}

pub fn integer(
    args: &Map<String, Value>,
    key: &str,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_i64()
            .filter(|number| (min..=max).contains(number))
            .ok_or_else(|| format!("`{key}` must be an integer from {min} to {max}")),
    }
}

pub fn boolean(args: &Map<String, Value>, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("`{key}` must be true or false")),
    }
}

pub fn object<'a>(
    args: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a Map<String, Value>>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(value)) => Ok(Some(value)),
        Some(_) => Err(format!("`{key}` must be an object")),
    }
}

pub fn server(args: &Map<String, Value>) -> Result<&str, String> {
    let server = text(args, "server")?;
    if server.len() > 64
        || !server
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(format!(
            "`server` must be a server UUID or short ID, got \"{server}\""
        ));
    }
    Ok(server)
}

pub fn method(args: &Map<String, Value>, allowed: &[Method]) -> Result<Method, String> {
    let value = text(args, "method")?;
    allowed
        .iter()
        .find(|method| method.as_str().eq_ignore_ascii_case(value))
        .cloned()
        .ok_or_else(|| {
            let names: Vec<&str> = allowed.iter().map(Method::as_str).collect();
            format!("`method` must be one of {}", names.join(", "))
        })
}

pub fn to_map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
