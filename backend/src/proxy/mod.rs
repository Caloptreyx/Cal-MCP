mod path;
mod query;

use crate::tools::Ctx;
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Method, Request, header},
};
use serde_json::{Map, Value};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_BODY: usize = 16 * 1024 * 1024;

pub struct Reply {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

pub async fn call(
    ctx: &Ctx,
    method: Method,
    path: &str,
    query: Option<&Map<String, Value>>,
    body: Option<&Value>,
) -> Result<Reply, String> {
    path::check(path)?;

    let mut uri = path.to_string();
    if let Some(query) = query {
        let encoded = query::encode(query)?;
        if !encoded.is_empty() {
            uri.push('?');
            uri.push_str(&encoded);
        }
    }

    let mut builder = Request::builder()
        .method(method)
        .uri(&uri)
        .header(header::ACCEPT, "application/json");
    for (name, value) in &ctx.forward {
        builder = builder.header(name, value);
    }

    let request_body = match body {
        None | Some(Value::Null) => Body::empty(),
        Some(Value::String(text)) => {
            builder = builder.header(header::CONTENT_TYPE, "text/plain; charset=utf-8");
            Body::from(text.clone())
        }
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(value).map_err(|err| err.to_string())?)
        }
    };

    let mut request = builder
        .body(request_body)
        .map_err(|err| format!("Invalid request: {err}"))?;
    request.extensions_mut().insert(ConnectInfo(ctx.addr));

    let response = tokio::time::timeout(
        TIMEOUT,
        ctx.state
            .send_authenticated_router_oneshot(request, ctx.user.clone(), ctx.auth.clone()),
    )
    .await
    .map_err(|_| "The panel did not answer within 120 seconds".to_string())?
    .map_err(|err| err.to_string())?;

    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = axum::body::to_bytes(response.into_body(), MAX_BODY)
        .await
        .map_err(|_| "The response is larger than 16 MB".to_string())?;

    Ok(Reply {
        status,
        content_type,
        body: body.to_vec(),
    })
}
