use super::{
    Ctx,
    args::{boolean, integer, optional_text, server, text, to_map},
    output::{request, request_text, strip_ansi, tail, truncate},
};
use axum::http::Method;
use serde_json::{Map, Value, json};

const POWER_ACTIONS: [&str; 4] = ["start", "stop", "restart", "kill"];

async fn state(ctx: &Ctx, server: &str) -> Option<String> {
    let resources = request(
        ctx,
        Method::GET,
        &format!("/api/client/servers/{server}/resources"),
        None,
        None,
    )
    .await
    .ok()?;

    serde_json::from_str::<Value>(&resources)
        .ok()?
        .pointer("/resources/state")?
        .as_str()
        .map(str::to_string)
}

pub async fn list(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let mut query = to_map(json!({
        "page": integer(args, "page", 1, 1, i64::MAX)?,
        "per_page": integer(args, "per_page", 25, 1, 100)?,
        "other": boolean(args, "other")?,
    }));
    if let Some(search) = optional_text(args, "search")?.filter(|search| !search.is_empty()) {
        query.insert("search".into(), json!(search));
    }

    request(ctx, Method::GET, "/api/client/servers", Some(&query), None).await
}

pub async fn get(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let base = format!("/api/client/servers/{}", server(args)?);
    let details = request(ctx, Method::GET, &base, None, None).await?;
    let resources = request(ctx, Method::GET, &format!("{base}/resources"), None, None).await;

    let parse = |text: String| serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    let mut combined = to_map(parse(details));
    combined.insert(
        "resources".into(),
        match resources {
            Ok(text) => parse(text),
            Err(err) => json!({ "error": err }),
        },
    );

    Ok(truncate(Value::Object(combined).to_string()))
}

pub async fn power(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let action = text(args, "action")?;
    if !POWER_ACTIONS.contains(&action) {
        return Err("`action` must be start, stop, restart or kill".into());
    }

    request(
        ctx,
        Method::POST,
        &format!("/api/client/servers/{server}/power"),
        None,
        Some(&json!({ "action": action })),
    )
    .await?;
    Ok(format!("Sent {action} to server {server}."))
}

pub async fn command(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let command = text(args, "command")?;
    if command.chars().count() > 1024 {
        return Err("`command` must be at most 1024 characters".into());
    }

    if let Err(err) = request(
        ctx,
        Method::POST,
        &format!("/api/client/servers/{server}/command"),
        None,
        Some(&json!({ "command": command })),
    )
    .await
    {
        return Err(match state(ctx, server).await.as_deref() {
            Some("offline") => {
                format!("Server {server} is offline. Start it with power_server first.")
            }
            _ => err,
        });
    }

    Ok(format!(
        "Sent \"{command}\" to server {server}. Use read_console to see the output."
    ))
}

pub async fn console(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let lines = integer(args, "lines", 100, 1, 2000)? as usize;
    let log = request_text(ctx, &format!("/api/client/servers/{server}/logs"), None).await?;

    let output = tail(&strip_ansi(&log), lines);
    Ok(if output.trim().is_empty() {
        "The console has no output yet.".into()
    } else {
        output
    })
}
