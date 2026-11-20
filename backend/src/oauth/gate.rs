//! The public MCP endpoint. It sorts each request by credential:
//! - none: 401 with the OAuth challenge (RFC 9728 section 5.1), so clients can discover OAuth;
//! - an OAuth access token: the request runs as the grant's panel API key;
//! - anything else (panel API keys, cookies): forwarded unchanged so the panel authenticates it
//!   exactly as before OAuth existed.
//!
//! Both paths end at the client API route [`INNER_PATH`], behind the panel's own auth middleware.

use super::{INNER_PATH, Scope, Urls, flow, params, pg::PgStore};
use axum::{
    extract::{ConnectInfo, Request},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;
use shared::{
    GetState,
    models::{
        ByUuid,
        user::{AuthMethod, User},
        user_api_key::UserApiKey,
    },
};
use std::net::SocketAddr;

/// Set on requests that arrive with an OAuth access token. Request extensions cannot be set from
/// outside the process, so the MCP handler can trust it.
#[derive(Clone, Copy, Debug)]
pub struct OAuthGrant {
    pub scope: Scope,
}

/// The `WWW-Authenticate` challenge (RFC 6750 section 3, RFC 9728 section 5.1).
pub fn challenge(urls: &Urls, error: Option<(&str, &str)>) -> String {
    let mut value = format!(
        "Bearer resource_metadata=\"{}\", scope=\"{}\"",
        urls.resource_metadata,
        Scope::DEFAULT.as_str()
    );
    if let Some((error, description)) = error {
        value.push_str(&format!(
            ", error=\"{error}\", error_description=\"{description}\""
        ));
    }
    value
}

fn unauthorized(urls: &Urls, error: Option<(&str, &str)>) -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        axum::Json(json!({
            "errors": [error.map_or("authorization required", |(_, description)| description)]
        })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&challenge(urls, error)) {
        response.headers_mut().insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

fn internal(err: anyhow::Error) -> Response {
    tracing::error!("Cal MCP OAuth: {err:#}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(json!({ "errors": ["internal server error"] })),
    )
        .into_response()
}

/// Rebuilds the request for the inner route, keeping method, headers, body and peer address.
fn inner_request(
    request: Request,
    addr: SocketAddr,
    strip_credentials: bool,
) -> Result<Request, anyhow::Error> {
    let (parts, body) = request.into_parts();

    let mut builder = axum::http::Request::builder()
        .method(parts.method)
        .uri(INNER_PATH);
    for (name, value) in &parts.headers {
        if strip_credentials && (name == header::AUTHORIZATION || name == header::COOKIE) {
            continue;
        }
        builder = builder.header(name, value);
    }

    let mut request = builder.body(body)?;
    request.extensions_mut().insert(ConnectInfo(addr));
    Ok(request)
}

pub async fn handle(
    state: GetState,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let urls = match Urls::load(&state).await {
        Ok(urls) => urls,
        Err(err) => return internal(err),
    };

    let bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(params::bearer)
        .map(str::to_string);

    match bearer {
        Some(token) if token.starts_with(super::ACCESS_PREFIX) => {
            oauth(&state, &urls, addr, request, &token).await
        }
        None if !request.headers().contains_key(header::AUTHORIZATION)
            && !request.headers().contains_key(header::COOKIE) =>
        {
            unauthorized(&urls, None)
        }
        _ => forward(&state, &urls, addr, request).await,
    }
}

async fn oauth(
    state: &shared::State,
    urls: &Urls,
    addr: SocketAddr,
    request: Request,
    token: &str,
) -> Response {
    let invalid = || {
        unauthorized(
            urls,
            Some(("invalid_token", "The access token is invalid or expired")),
        )
    };

    let store = PgStore::new(state);
    let grant = match flow::access(&store, urls, token, chrono::Utc::now()).await {
        Ok(Some(grant)) => grant,
        Ok(None) => return invalid(),
        Err(err) => return internal(err),
    };

    let user = match User::by_uuid_optional_cached(&state.database, grant.user_uuid).await {
        Ok(Some(user)) => user,
        Ok(None) => return invalid(),
        Err(err) => return internal(err),
    };
    let api_key =
        match UserApiKey::by_user_uuid_uuid(&state.database, grant.user_uuid, grant.api_key_uuid)
            .await
        {
            Ok(Some(api_key)) => api_key,
            Ok(None) => return invalid(),
            Err(err) => return internal(err.into()),
        };

    let mut request = match inner_request(request, addr, true) {
        Ok(request) => request,
        Err(err) => return internal(err),
    };
    request
        .extensions_mut()
        .insert(OAuthGrant { scope: grant.scope });

    // The panel's client middleware takes this pair instead of a header, then applies the same
    // account and key checks as for any API key.
    match state
        .send_authenticated_router_oneshot(request, user, AuthMethod::ApiKey(api_key))
        .await
    {
        Ok(response) => response,
        Err(err) => internal(err),
    }
}

async fn forward(
    state: &shared::State,
    urls: &Urls,
    addr: SocketAddr,
    request: Request,
) -> Response {
    let request = match inner_request(request, addr, false) {
        Ok(request) => request,
        Err(err) => return internal(err),
    };

    match state.send_router_oneshot(request).await {
        Ok(mut response) => {
            if response.status() == StatusCode::UNAUTHORIZED
                && let Ok(value) = HeaderValue::from_str(&challenge(
                    urls,
                    Some(("invalid_token", "The credentials are invalid")),
                ))
            {
                response
                    .headers_mut()
                    .insert(header::WWW_AUTHENTICATE, value);
            }
            response
        }
        Err(err) => internal(err),
    }
}
