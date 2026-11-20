mod path;
mod query;

use crate::tools::Ctx;
use axum::{
    body::Body,
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
    ctx.access.check_request(&method, path)?;

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

    let request = builder
        .body(request_body)
        .map_err(|err| format!("Invalid request: {err}"))?;

    let response = tokio::time::timeout(TIMEOUT, ctx.upstream.send(request))
        .await
        .map_err(|_| "The panel did not answer within 120 seconds".to_string())??;

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
