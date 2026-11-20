mod api;
mod args;
mod definitions;
mod files;
pub mod output;
mod servers;
#[cfg(test)]
mod tests;

use crate::{access::Access, upstream::Upstream};
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub use definitions::definitions;

pub struct Ctx {
    pub upstream: Arc<dyn Upstream>,
    /// What this connection may do. The proxy enforces it on every request;
    /// tool listing and calling follow it so clients see the truth.
    pub access: Access,
}

fn read_only_hint(tool: &Value) -> bool {
    tool.pointer("/annotations/readOnlyHint")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The tools this connection may use, in a stable order.
pub fn list(access: Access) -> Vec<Value> {
    definitions()
        .iter()
        .filter(|tool| access.allows_tool(read_only_hint(tool)))
        .cloned()
        .collect()
}

pub async fn call(ctx: &Ctx, params: &Map<String, Value>) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("tools/call needs params.name")?;
    let empty = Map::new();
    let args = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(arguments)) => arguments,
        Some(_) => return Err("params.arguments must be an object".into()),
    };

    let Some(definition) = definitions()
        .iter()
        .find(|tool| tool["name"].as_str() == Some(name))
    else {
        return Err(format!("Unknown tool: {name}"));
    };
    if !ctx.access.allows_tool(read_only_hint(definition)) {
        return Err(format!(
            "Tool {name} is not available because this MCP connection is read-only"
        ));
    }

    let outcome = match name {
        "search_api" => api::search(ctx, args).await,
        "describe_api" => api::describe(ctx, args).await,
        "read_api" => api::read(ctx, args).await,
        "write_api" => api::write(ctx, args).await,
        "list_servers" => servers::list(ctx, args).await,
        "get_server" => servers::get(ctx, args).await,
        "power_server" => servers::power(ctx, args).await,
        "send_command" => servers::command(ctx, args).await,
        "read_console" => servers::console(ctx, args).await,
        "list_files" => files::list(ctx, args).await,
        "read_file" => files::read(ctx, args).await,
        "write_file" => files::write(ctx, args).await,
        _ => return Err(format!("Unknown tool: {name}")),
    };

    let (text, is_error) = match outcome {
        Ok(text) => (text, false),
        Err(text) => (text, true),
    };
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    }))
}
