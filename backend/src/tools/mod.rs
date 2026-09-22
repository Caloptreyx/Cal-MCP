mod api;
mod args;
mod definitions;
mod files;
mod output;
mod servers;

use axum::http::{HeaderName, HeaderValue};
use serde_json::{Map, Value, json};
use shared::{
    State,
    models::user::{AuthMethod, User},
};
use std::net::SocketAddr;

pub use definitions::definitions;

pub struct Ctx {
    pub state: State,
    pub user: User,
    pub auth: AuthMethod,
    pub addr: SocketAddr,
    pub forward: Vec<(HeaderName, HeaderValue)>,
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
