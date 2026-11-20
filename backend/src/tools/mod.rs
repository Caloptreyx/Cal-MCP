mod api;
mod args;
mod definitions;
mod files;
mod manage;
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

    let outcome: Result<Output, String> = match name {
        "search_api" => api::search(ctx, args).await.map(Output::from),
        "describe_api" => api::describe(ctx, args).await.map(Output::from),
        "read_api" => api::read(ctx, args).await,
        "write_api" => api::write(ctx, args).await.map(Output::from),
        "list_servers" => servers::list(ctx, args).await,
        "get_server" => servers::get(ctx, args).await,
        "power_server" => servers::power(ctx, args).await.map(Output::from),
        "send_command" => servers::command(ctx, args).await.map(Output::from),
        "run_command" => servers::run_command(ctx, args).await.map(Output::from),
        "read_console" => servers::console(ctx, args).await.map(Output::from),
        "list_files" => files::list(ctx, args).await,
        "read_file" => files::read(ctx, args).await,
        "write_file" => files::write(ctx, args).await.map(Output::from),
        "edit_file" => files::edit(ctx, args).await.map(Output::from),
        "list_backups" => manage::list_backups(ctx, args).await.map(Output::from),
        "create_backup" => manage::create_backup(ctx, args).await.map(Output::from),
        "restore_backup" => manage::restore_backup(ctx, args).await.map(Output::from),
        "delete_backup" => manage::delete_backup(ctx, args).await.map(Output::from),
        "list_schedules" => manage::list_schedules(ctx, args).await.map(Output::from),
        "create_schedule" => manage::create_schedule(ctx, args).await.map(Output::from),
        "run_schedule" => manage::run_schedule(ctx, args).await.map(Output::from),
        "list_startup_variables" => manage::list_variables(ctx, args).await.map(Output::from),
        "update_startup_variables" => manage::update_variables(ctx, args).await.map(Output::from),
        _ => return Err(format!("Unknown tool: {name}")),
    };

    Ok(match outcome {
        Ok(output) => output.into_result(),
        Err(text) => json!({
            "content": [{ "type": "text", "text": text }],
            "isError": true,
        }),
    })
}

/// What a tool produced.
pub struct Output {
    pub text: String,
    /// A second text block, such as which part of a file `text` is.
    pub note: Option<String>,
    /// The result as JSON, for tools that declare an `outputSchema`.
    pub structured: Option<Value>,
}

impl From<String> for Output {
    fn from(text: String) -> Self {
        Self {
            text,
            note: None,
            structured: None,
        }
    }
}

impl Output {
    /// A panel reply that is a JSON object: the text stays for clients without
    /// structured output, the object becomes `structuredContent`.
    pub fn structured(text: String, json: Option<Value>) -> Self {
        Self {
            text,
            note: None,
            structured: json.filter(Value::is_object),
        }
    }

    fn into_result(self) -> Value {
        let mut content = vec![json!({ "type": "text", "text": self.text })];
        if let Some(note) = self.note {
            content.push(json!({ "type": "text", "text": note }));
        }
        let mut result = json!({ "content": content, "isError": false });
        if let Some(structured) = self.structured {
            result["structuredContent"] = structured;
        }
        result
    }
}
