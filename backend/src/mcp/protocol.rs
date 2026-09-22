use super::{
    error::{INVALID_PARAMS, INVALID_REQUEST, PARSE_ERROR, Rejection},
    lifecycle::{LEGACY, MODERN},
};
use axum::http::{HeaderMap, StatusCode};
use base64::Engine;
use serde_json::{Map, Value};

const PROTOCOL_VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const CAPABILITIES_KEY: &str = "io.modelcontextprotocol/clientCapabilities";

#[derive(Debug, PartialEq, Eq)]
pub enum Era {
    Modern,
    Legacy,
}

#[derive(Debug)]
pub struct Prepared {
    pub id: Option<Value>,
    pub method: String,
    pub params: Map<String, Value>,
    pub era: Era,
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

pub fn decode_header(value: &str) -> Option<String> {
    match value
        .strip_prefix("=?base64?")
        .and_then(|rest| rest.strip_suffix("?="))
    {
        Some(encoded) => base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok()),
        None => Some(value.to_string()),
    }
}

fn bad_request(id: Option<Value>, code: i64, message: impl Into<String>) -> Box<Rejection> {
    Rejection::new(StatusCode::BAD_REQUEST, id, code, message)
}

pub fn prepare(headers: &HeaderMap, body: &[u8]) -> Result<Prepared, Box<Rejection>> {
    let Ok(message) = serde_json::from_slice::<Value>(body) else {
        return Err(bad_request(None, PARSE_ERROR, "Parse error"));
    };

    let Value::Object(mut message) = message else {
        return Err(bad_request(
            None,
            INVALID_REQUEST,
            "Expected a single JSON-RPC message object",
        ));
    };

    let id = match message.remove("id") {
        None => None,
        Some(id @ (Value::String(_) | Value::Number(_))) => Some(id),
        Some(_) => {
            return Err(bad_request(
                None,
                INVALID_REQUEST,
                "The request id must be a string or a number",
            ));
        }
    };

    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(bad_request(id, INVALID_REQUEST, "jsonrpc must be \"2.0\""));
    }

    let method = match message.remove("method") {
        Some(Value::String(method)) => method,
        _ => {
            return Err(bad_request(
                id,
                INVALID_REQUEST,
                "Only JSON-RPC requests and notifications are accepted",
            ));
        }
    };

    let params = match message.remove("params") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(params)) => params,
        Some(_) => return Err(bad_request(id, INVALID_PARAMS, "params must be an object")),
    };

    let header_version = header_str(headers, "mcp-protocol-version");
    let meta_version = params
        .get("_meta")
        .and_then(|meta| meta.get(PROTOCOL_VERSION_KEY))
        .and_then(Value::as_str);

    if method != "initialize"
        && let Some(version) = meta_version
    {
        validate_modern(headers, &id, &method, &params, version, header_version)?;

        return Ok(Prepared {
            id,
            method,
            params,
            era: Era::Modern,
        });
    }

    if method != "initialize" {
        match header_version {
            None => {}
            Some(version) if LEGACY.contains(&version) => {}
            Some(MODERN) => {
                return Err(bad_request(
                    id,
                    INVALID_PARAMS,
                    format!("Missing _meta \"{PROTOCOL_VERSION_KEY}\""),
                ));
            }
            Some(version) => return Err(Rejection::unsupported(id, version)),
        }
    }

    Ok(Prepared {
        id,
        method,
        params,
        era: Era::Legacy,
    })
}

fn validate_modern(
    headers: &HeaderMap,
    id: &Option<Value>,
    method: &str,
    params: &Map<String, Value>,
    version: &str,
    header_version: Option<&str>,
) -> Result<(), Box<Rejection>> {
    if version != MODERN {
        return Err(Rejection::unsupported(id.clone(), version));
    }

    match header_version {
        None => {
            return Err(Rejection::mismatch(
                id,
                "Missing MCP-Protocol-Version header",
            ));
        }
        Some(value) if value != version => {
            return Err(Rejection::mismatch(
                id,
                format!(
                    "MCP-Protocol-Version header value '{value}' does not match body value '{version}'"
                ),
            ));
        }
        Some(_) => {}
    }

    match header_str(headers, "mcp-method") {
        None => return Err(Rejection::mismatch(id, "Missing Mcp-Method header")),
        Some(value) if value != method => {
            return Err(Rejection::mismatch(
                id,
                format!("Mcp-Method header value '{value}' does not match body value '{method}'"),
            ));
        }
        Some(_) => {}
    }

    let target = match method {
        "tools/call" | "prompts/get" => params.get("name").and_then(Value::as_str),
        "resources/read" => params.get("uri").and_then(Value::as_str),
        _ => None,
    };

    if let Some(target) = target {
        match header_str(headers, "mcp-name").map(decode_header) {
            None => return Err(Rejection::mismatch(id, "Missing Mcp-Name header")),
            Some(None) => return Err(Rejection::mismatch(id, "Mcp-Name header is not valid")),
            Some(Some(value)) if value != target => {
                return Err(Rejection::mismatch(
                    id,
                    format!("Mcp-Name header value '{value}' does not match body value '{target}'"),
                ));
            }
            Some(Some(_)) => {}
        }
    }

    let has_capabilities = params
        .get("_meta")
        .and_then(|meta| meta.get(CAPABILITIES_KEY))
        .is_some_and(Value::is_object);

    if !has_capabilities {
        return Err(bad_request(
            id.clone(),
            INVALID_PARAMS,
            format!("Missing _meta \"{CAPABILITIES_KEY}\""),
        ));
    }

    Ok(())
}
