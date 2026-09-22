use super::{
    Ctx,
    args::{integer, optional_text, server, text, to_map},
    output::{request, request_text},
};
use axum::http::Method;
use serde_json::{Map, Value, json};

pub async fn list(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let directory = optional_text(args, "directory")?
        .filter(|directory| !directory.is_empty())
        .unwrap_or("/");
    let query = to_map(json!({
        "directory": directory,
        "page": integer(args, "page", 1, 1, i64::MAX)?,
        "per_page": integer(args, "per_page", 100, 1, 100)?,
    }));

    request(
        ctx,
        Method::GET,
        &format!("/api/client/servers/{server}/files/list"),
        Some(&query),
        None,
    )
    .await
}

pub async fn read(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let query = to_map(json!({ "file": text(args, "file")? }));

    request_text(
        ctx,
        &format!("/api/client/servers/{server}/files/contents"),
        Some(&query),
    )
    .await
}

pub async fn write(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let file = text(args, "file")?;
    let Some(Value::String(content)) = args.get("content") else {
        return Err("`content` is required and must be a string".into());
    };
    let query = to_map(json!({ "file": file }));

    request(
        ctx,
        Method::POST,
        &format!("/api/client/servers/{server}/files/write"),
        Some(&query),
        Some(&Value::String(content.clone())),
    )
    .await?;
    Ok(format!(
        "Wrote {} bytes to {file} on server {server}.",
        content.len()
    ))
}
