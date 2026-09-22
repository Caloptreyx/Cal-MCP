mod dispatch;
mod error;
mod lifecycle;
mod protocol;

use crate::tools::Ctx;
use axum::{
    Json,
    body::Bytes,
    extract::ConnectInfo,
    http::{HeaderMap, HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use error::{INVALID_REQUEST, Rejection};
use serde_json::json;
use shared::{
    GetState, State,
    models::user::{AuthMethod, GetAuthMethod, GetUser},
};
use std::net::SocketAddr;
use utoipa_axum::router::OpenApiRouter;

const FORWARDED_HEADERS: [HeaderName; 3] = [
    HeaderName::from_static("x-forwarded-for"),
    HeaderName::from_static("x-real-ip"),
    header::USER_AGENT,
];

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .route("/mcp", post(handle))
        .with_state(state.clone())
}

async fn handle(
    state: GetState,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    user: GetUser,
    auth: GetAuthMethod,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(rejection) = check_origin(&state, &headers).await {
        return rejection.into_response();
    }

    if !matches!(auth.0.as_ref(), AuthMethod::ApiKey(_)) {
        return Rejection::new(
            StatusCode::FORBIDDEN,
            None,
            INVALID_REQUEST,
            "Connect with a panel API key sent as \"Authorization: Bearer <key>\"",
        )
        .into_response();
    }

    let prepared = match protocol::prepare(&headers, &body) {
        Ok(prepared) => prepared,
        Err(rejection) => return rejection.into_response(),
    };

    let Some(id) = prepared.id.clone() else {
        return StatusCode::ACCEPTED.into_response();
    };

    let ctx = Ctx {
        state: state.0.clone(),
        user: user.0,
        auth: auth.0.as_ref().clone(),
        addr,
        forward: FORWARDED_HEADERS
            .iter()
            .filter_map(|name| headers.get(name).map(|value| (name.clone(), value.clone())))
            .collect(),
    };

    match dispatch::dispatch(&ctx, &prepared).await {
        Ok(result) => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": lifecycle::finish(&prepared.era, result),
        }))
        .into_response(),
        Err(rejection) => rejection.into_response(),
    }
}

async fn check_origin(state: &State, headers: &HeaderMap) -> Option<Box<Rejection>> {
    let origin = headers.get(header::ORIGIN)?;

    let allowed = match state.settings.get().await {
        Ok(settings) => url::Url::parse(&settings.app.url)
            .ok()
            .map(|url| url.origin().ascii_serialization()),
        Err(_) => None,
    };

    match (origin.to_str().ok(), allowed) {
        (Some(origin), Some(allowed)) if origin == allowed => None,
        _ => Some(Rejection::new(
            StatusCode::FORBIDDEN,
            None,
            INVALID_REQUEST,
            "Origin not allowed",
        )),
    }
}
