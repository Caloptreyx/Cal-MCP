use super::{
    Ctx,
    args::{integer, method, object, optional_text, text},
    output::{request, truncate},
};
use crate::openapi::{self, Scope};
use axum::http::Method;
use serde_json::{Map, Value};

pub async fn search(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let query = text(args, "query")?;
    let scope = Scope::parse(optional_text(args, "scope")?)?;
    let limit = integer(args, "limit", 15, 1, 50)? as usize;
    let index = openapi::load(&ctx.state).await?;
    let found = index.search(query, scope, limit);

    if found.is_empty() {
        return Ok(format!(
            "No endpoints match \"{query}\". Try other words, for example the resource name in plural."
        ));
    }

    Ok(found
        .iter()
        .map(|operation| operation.summary_line())
        .collect::<Vec<_>>()
        .join("\n"))
}

pub async fn describe(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let method = text(args, "method")?;
    let path = text(args, "path")?;
    let index = openapi::load(&ctx.state).await?;
    let operation = index.find(method, path).ok_or_else(|| {
        format!(
            "No {} endpoint at {path}. Use search_api to find the right path.",
            method.to_ascii_uppercase()
        )
    })?;

    serde_json::to_string_pretty(&index.describe(operation))
        .map(truncate)
        .map_err(|err| err.to_string())
}

pub async fn read(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    request(
        ctx,
        Method::GET,
        text(args, "path")?,
        object(args, "query")?,
        None,
    )
    .await
}

pub async fn write(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let method = method(
        args,
        &[Method::POST, Method::PUT, Method::PATCH, Method::DELETE],
    )?;
    request(
        ctx,
        method,
        text(args, "path")?,
        object(args, "query")?,
        args.get("body"),
    )
    .await
}
