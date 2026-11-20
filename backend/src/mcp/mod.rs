mod dispatch;
mod error;
mod lifecycle;
mod protocol;

use crate::{oauth::OAuthGrant, tools::Ctx, upstream::LiveUpstream};
use axum::{
    Extension, Json,
    body::Bytes,
    extract::ConnectInfo,
    http::{HeaderMap, HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use error::{INTERNAL_ERROR, INVALID_REQUEST, Rejection};
use serde_json::json;
use shared::{
    GetState, State,
    models::user::{AuthMethod, GetAuthMethod, GetUser},
};
use std::{net::SocketAddr, sync::Arc};
use utoipa_axum::router::OpenApiRouter;

const FORWARDED_HEADERS: [HeaderName; 3] = [
    HeaderName::from_static("x-forwarded-for"),
    HeaderName::from_static("x-real-ip"),
    header::USER_AGENT,
];

/// Mounted on the client API, behind the panel's auth middleware. Clients connect to
/// [`crate::oauth::MCP_PATH`], which forwards here.
pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .route("/mcp/inner", post(handle))
        .with_state(state.clone())
}

async fn handle(
    state: GetState,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    user: GetUser,
    auth: GetAuthMethod,
    oauth: Option<Extension<OAuthGrant>>,
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
            "Connect with OAuth or with a panel API key sent as \"Authorization: Bearer <key>\"",
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

    // Restrictions come from the settings, so a settings store that cannot be
    // read must not fall back to an unrestricted default.
    let extension_settings = match crate::settings::load_result(&state).await {
        Ok(settings) => settings,
        Err(err) => {
            tracing::error!("failed to load Cal MCP settings: {err:#}");
            return Rejection::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                Some(id.clone()),
                INTERNAL_ERROR,
                "Could not load the MCP settings",
            )
            .into_response();
        }
    };

    let auth = auth.0.as_ref().clone();
    let api_key = match &auth {
        AuthMethod::ApiKey(key) => Some(key.uuid),
        _ => None,
    };
    // An OAuth grant runs as its backing API key, so the key's restrictions apply as well as
    // the scope the user approved.
    let mut access = extension_settings.access_for(api_key);
    if let Some(Extension(grant)) = oauth {
        access = access.union(grant.scope.access());
    }
    let ctx = Ctx {
        access,
        upstream: Arc::new(LiveUpstream {
            state: state.0.clone(),
            user: user.0,
            auth,
            addr,
            forward: FORWARDED_HEADERS
                .iter()
                .filter_map(|name| headers.get(name).map(|value| (name.clone(), value.clone())))
                .collect(),
        }),
    };

    match dispatch::dispatch(&ctx, &prepared, &extension_settings).await {
        Ok(result) => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": lifecycle::finish(&prepared.era, result, &extension_settings),
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
