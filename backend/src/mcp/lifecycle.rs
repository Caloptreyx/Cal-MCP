use super::protocol::Era;
use serde_json::{Map, Value, json};

pub const MODERN: &str = "2026-07-28";
pub const LEGACY: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Cal MCP manages a Calagopus game panel with the permissions of the API key it was connected with. \
Use the server tools (list_servers, get_server, power_server, send_command, read_console, list_files, read_file, write_file) for everyday server work. \
Everything else the panel API offers is reachable in three steps: search_api to find the endpoint, describe_api to see its parameters and body, then read_api for GET or write_api for POST, PUT, PATCH and DELETE. \
Paths look like /api/client/servers/{server}/backups; replace every {placeholder} with a real value before calling. \
Admin endpoints under /api/admin only work when the key has admin permissions.";

pub fn supported_versions() -> Vec<&'static str> {
    std::iter::once(MODERN).chain(LEGACY).collect()
}

pub fn server_info() -> Value {
    json!({
        "name": "cal-mcp",
        "title": "Cal MCP",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn capabilities() -> Value {
    json!({ "tools": { "listChanged": false } })
}

pub fn initialize(params: &Map<String, Value>) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let version = LEGACY
        .iter()
        .find(|version| **version == requested)
        .copied()
        .unwrap_or(LEGACY[0]);

    json!({
        "protocolVersion": version,
        "capabilities": capabilities(),
        "serverInfo": server_info(),
        "instructions": INSTRUCTIONS,
    })
}

pub fn discover() -> Value {
    json!({
        "supportedVersions": supported_versions(),
        "capabilities": capabilities(),
        "instructions": INSTRUCTIONS,
    })
}

pub fn finish(era: &Era, result: Value) -> Value {
    match (era, result) {
        (Era::Modern, Value::Object(mut result)) => {
            result.insert("resultType".into(), json!("complete"));
            result.insert(
                "_meta".into(),
                json!({ "io.modelcontextprotocol/serverInfo": server_info() }),
            );
            Value::Object(result)
        }
        (_, result) => result,
    }
}
