use super::{
    ISSUER_PATH, MCP_PATH, Scope, Urls,
    flow::{self, ApproveError, AuthorizeError, OAuthError},
    gate,
    params::{self, Params},
    pg::PgStore,
    store::Store,
    validate,
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use shared::{
    GetIp, GetState, State,
    models::user::{AuthMethod, GetAuthMethod, GetPermissionManager, GetUser, GetUserImpersonator},
    models::user_activity::GetUserActivityLogger,
    response::{ApiResponse, ApiResponseResult},
};
use utoipa_axum::router::OpenApiRouter;
use uuid::Uuid;

const OAUTH: &str = "/extensions/dev.caloptreyx.calmcp/oauth";

pub fn global_router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .route(MCP_PATH, post(gate::handle).get(gate::handle).delete(gate::handle))
        .route(
            &format!("/.well-known/oauth-protected-resource{MCP_PATH}"),
            get(protected_resource),
        )
        .route(
            &format!("/.well-known/oauth-authorization-server{ISSUER_PATH}"),
            get(authorization_server),
        )
        .with_state(state.clone())
}

pub fn auth_router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .route(&format!("{OAUTH}/register"), post(register).options(preflight))
        .route(&format!("{OAUTH}/token"), post(token).options(preflight))
        .route(&format!("{OAUTH}/revoke"), post(revoke).options(preflight))
        .with_state(state.clone())
}

pub fn client_router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .route(
            &format!("{OAUTH}/authorize"),
            get(consent).post(decide),
        )
        .route(&format!("{OAUTH}/grants"), get(list_grants))
        .route(&format!("{OAUTH}/grants/{{grant}}"), delete(revoke_grant))
        .with_state(state.clone())
}

/// The metadata documents and the registration, token and revocation endpoints take no cookies,
/// so browser-based clients may call them from any origin.
fn allow_any_origin(response: &mut Response) {
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
}

async fn preflight() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    allow_any_origin(&mut response);
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Authorization, Content-Type"),
    );
    headers.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
    response
}

fn metadata(document: Value) -> Response {
    let mut response = Json(document).into_response();
    allow_any_origin(&mut response);
    response
}

fn scopes_supported() -> Vec<&'static str> {
    Scope::ALL.iter().map(|scope| scope.as_str()).collect()
}

/// RFC 9728 protected resource metadata.
pub fn protected_resource_document(urls: &Urls, resource_name: &str) -> Value {
    json!({
        "resource": urls.resource,
        "authorization_servers": [urls.issuer],
        "scopes_supported": scopes_supported(),
        "bearer_methods_supported": ["header"],
        "resource_name": resource_name,
        "resource_documentation": "https://github.com/Caloptreyx/Cal-MCP",
    })
}

/// RFC 8414 authorization server metadata.
pub fn authorization_server_document(urls: &Urls) -> Value {
    let auth_methods = ["none", "client_secret_basic", "client_secret_post"];

    json!({
        "issuer": urls.issuer,
        "authorization_endpoint": urls.authorization_endpoint,
        "token_endpoint": urls.token_endpoint,
        "registration_endpoint": urls.registration_endpoint,
        "revocation_endpoint": urls.revocation_endpoint,
        "scopes_supported": scopes_supported(),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": auth_methods,
        "revocation_endpoint_auth_methods_supported": auth_methods,
        "code_challenge_methods_supported": ["S256"],
        "authorization_response_iss_parameter_supported": true,
    })
}

async fn protected_resource(state: GetState) -> Response {
    let urls = match Urls::load(&state).await {
        Ok(urls) => urls,
        Err(err) => return ApiResponse::from(err).into_response(),
    };
    let settings = crate::settings::load(&state).await;

    metadata(protected_resource_document(&urls, &settings.display_title))
}

async fn authorization_server(state: GetState) -> Response {
    match Urls::load(&state).await {
        Ok(urls) => metadata(authorization_server_document(&urls)),
        Err(err) => ApiResponse::from(err).into_response(),
    }
}

fn oauth_error(error: OAuthError, basic: bool) -> Response {
    let mut response = (error.status, Json(error.body())).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if error.status == StatusCode::UNAUTHORIZED && basic {
        headers.insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"Cal MCP\""),
        );
    }
    allow_any_origin(&mut response);
    response
}

fn no_store(status: StatusCode, body: Value) -> Response {
    let mut response = (status, Json(body)).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    allow_any_origin(&mut response);
    response
}

async fn ratelimit(state: &State, ip: &GetIp, name: &str, hits: u64, window: u64) -> Option<Response> {
    state
        .cache
        .ratelimit(
            format!("dev.caloptreyx.calmcp::oauth-{name}"),
            hits,
            window,
            ip.0.to_string(),
        )
        .await
        .err()
        .map(IntoResponse::into_response)
}

async fn register(state: GetState, ip: GetIp, body: Bytes) -> Response {
    if let Some(limited) = ratelimit(&state, &ip, "register", 20, 60 * 60).await {
        return limited;
    }

    let metadata: Value = match serde_json::from_slice(&body) {
        Ok(metadata) => metadata,
        Err(_) => {
            return oauth_error(
                OAuthError {
                    status: StatusCode::BAD_REQUEST,
                    error: "invalid_client_metadata",
                    description: "the body must be a JSON object".into(),
                },
                false,
            );
        }
    };

    match flow::register(&PgStore::new(&state), &metadata, chrono::Utc::now()).await {
        Ok(client) => no_store(StatusCode::CREATED, client),
        Err(error) => oauth_error(error, false),
    }
}

/// Form parameters and Basic credentials of a token or revocation request.
fn form(headers: &HeaderMap, body: &Bytes) -> Result<(Params, Option<(String, String)>), OAuthError> {
    let form_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/x-www-form-urlencoded"))
        });
    if !form_type {
        return Err(OAuthError::invalid_request(
            "send the parameters as application/x-www-form-urlencoded",
        ));
    }

    let body = std::str::from_utf8(body)
        .map_err(|_| OAuthError::invalid_request("the body is not UTF-8"))?;
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(params::basic_credentials);

    Ok((Params::parse(body), basic))
}

async fn token(state: GetState, ip: GetIp, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(limited) = ratelimit(&state, &ip, "token", 120, 60).await {
        return limited;
    }

    let (params, basic) = match form(&headers, &body) {
        Ok(form) => form,
        Err(error) => return oauth_error(error, false),
    };
    let used_basic = basic.is_some();
    let urls = match Urls::load(&state).await {
        Ok(urls) => urls,
        Err(err) => return oauth_error(OAuthError::server_error(err), false),
    };

    match flow::token(&PgStore::new(&state), &urls, basic, &params, chrono::Utc::now()).await {
        Ok(tokens) => no_store(StatusCode::OK, tokens),
        Err(error) => oauth_error(error, used_basic),
    }
}

async fn revoke(state: GetState, ip: GetIp, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(limited) = ratelimit(&state, &ip, "token", 120, 60).await {
        return limited;
    }

    let (params, basic) = match form(&headers, &body) {
        Ok(form) => form,
        Err(error) => return oauth_error(error, false),
    };
    let used_basic = basic.is_some();

    match flow::revoke(&PgStore::new(&state), basic, &params).await {
        Ok(()) => no_store(StatusCode::OK, json!({})),
        Err(error) => oauth_error(error, used_basic),
    }
}

/// Consent and grant management need the user's own browser session: an API key or an OAuth
/// token must not be able to create or manage OAuth grants, and neither may an admin who is
/// impersonating the user.
fn require_session(auth: &GetAuthMethod, impersonator: &GetUserImpersonator) -> Result<(), ApiResponse> {
    if !matches!(auth.0.as_ref(), AuthMethod::Session(_)) {
        return Err(ApiResponse::error("this action needs a signed-in browser session")
            .with_status(StatusCode::FORBIDDEN));
    }
    if impersonator.is_some() {
        return Err(ApiResponse::error("this action is not available while impersonating a user")
            .with_status(StatusCode::FORBIDDEN));
    }

    Ok(())
}

/// Whether a browser request came from another site or origin. The panel's JSON-only payloads
/// already need a CORS preflight it never grants; this also stops pages on sibling subdomains,
/// which count as the same site for the session cookie.
pub fn cross_origin(headers: &HeaderMap, urls: &Urls) -> bool {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());

    if header("sec-fetch-site").is_some_and(|site| site != "same-origin" && site != "none") {
        return true;
    }

    let app_origin = url::Url::parse(&urls.issuer)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_default();
    header("origin").is_some_and(|origin| origin != app_origin)
}

fn require_same_origin(headers: &HeaderMap, urls: &Urls) -> Result<(), ApiResponse> {
    if cross_origin(headers, urls) {
        return Err(ApiResponse::error("this request must come from the panel itself")
            .with_status(StatusCode::FORBIDDEN));
    }

    Ok(())
}

fn fatal(message: String) -> ApiResponse {
    ApiResponse::error(message).with_status(StatusCode::BAD_REQUEST)
}

async fn consent(
    state: GetState,
    auth: GetAuthMethod,
    impersonator: GetUserImpersonator,
    user: GetUser,
    RawQuery(query): RawQuery,
) -> ApiResponseResult {
    require_session(&auth, &impersonator)?;

    let urls = Urls::load(&state).await?;
    let store = PgStore::new(&state);

    match flow::authorize_request(&store, &urls, query.as_deref().unwrap_or_default()).await {
        Ok(request) => {
            let admin = user.admin
                || user
                    .role
                    .as_ref()
                    .is_some_and(|role| !role.admin_permissions.is_empty());

            ApiResponse::new_serialized(json!({
                "redirect": null,
                "consent": {
                    "client_name": request.client.name,
                    "redirect_uri": request.redirect_uri,
                    "redirect_host": validate::redirect_host(&request.redirect_uri),
                    "loopback": validate::is_loopback_redirect(&request.redirect_uri),
                    "requested_scope": request.requested,
                    "admin": admin,
                },
            }))
            .ok()
        }
        Err(AuthorizeError::Redirect(redirect)) => {
            ApiResponse::new_serialized(json!({ "redirect": redirect, "consent": null })).ok()
        }
        Err(AuthorizeError::Fatal(message)) => Err(fatal(message)),
    }
}

#[derive(Deserialize)]
struct Decision {
    /// The query string of the authorization request, as the consent page received it.
    query: String,
    approve: bool,
    scope: Option<Scope>,
}

async fn decide(
    state: GetState,
    auth: GetAuthMethod,
    impersonator: GetUserImpersonator,
    permissions: GetPermissionManager,
    user: GetUser,
    activity_logger: GetUserActivityLogger,
    headers: HeaderMap,
    shared::Payload(decision): shared::Payload<Decision>,
) -> ApiResponseResult {
    require_session(&auth, &impersonator)?;

    let urls = Urls::load(&state).await?;
    require_same_origin(&headers, &urls)?;
    let store = PgStore::new(&state);

    let request = match flow::authorize_request(&store, &urls, &decision.query).await {
        Ok(request) => request,
        Err(AuthorizeError::Redirect(redirect)) => {
            return ApiResponse::new_serialized(json!({ "redirect": redirect })).ok();
        }
        Err(AuthorizeError::Fatal(message)) => return Err(fatal(message)),
    };

    if !decision.approve {
        return ApiResponse::new_serialized(json!({ "redirect": flow::deny(&urls, &request) }))
            .ok();
    }

    // Every grant is a panel API key, so the same right applies.
    permissions.has_user_permission("api-keys.create")?;

    let scope = decision.scope.unwrap_or(request.requested);
    let redirect = match flow::approve(&store, &urls, &request, user.uuid, scope, chrono::Utc::now())
        .await
    {
        Ok(redirect) => redirect,
        Err(ApproveError::TooManyGrants) => {
            return ApiResponse::error(format!(
                "You already have {} connected MCP clients. Disconnect one under Account > MCP first.",
                super::MAX_GRANTS_PER_USER
            ))
            .with_status(StatusCode::CONFLICT)
            .ok();
        }
        Err(ApproveError::Store(message)) => {
            return ApiResponse::error(message)
                .with_status(StatusCode::INTERNAL_SERVER_ERROR)
                .ok();
        }
    };

    activity_logger
        .log(
            "calmcp:oauth.authorize",
            json!({
                "client": request.client.name,
                "redirect_uri": request.redirect_uri,
                "scope": scope,
            }),
        )
        .await;

    ApiResponse::new_serialized(json!({ "redirect": redirect })).ok()
}

async fn list_grants(
    state: GetState,
    auth: GetAuthMethod,
    impersonator: GetUserImpersonator,
    user: GetUser,
) -> ApiResponseResult {
    require_session(&auth, &impersonator)?;

    let grants = PgStore::new(&state).list_grants(user.uuid).await?;

    ApiResponse::new_serialized(json!({ "grants": grants })).ok()
}

async fn revoke_grant(
    state: GetState,
    auth: GetAuthMethod,
    impersonator: GetUserImpersonator,
    user: GetUser,
    activity_logger: GetUserActivityLogger,
    Path(grant): Path<Uuid>,
) -> ApiResponseResult {
    require_session(&auth, &impersonator)?;

    let store = PgStore::new(&state);
    let Some(grant) = store.user_grant(user.uuid, grant).await? else {
        return ApiResponse::error("connected client not found")
            .with_status(StatusCode::NOT_FOUND)
            .ok();
    };

    store.revoke(grant.uuid).await?;

    activity_logger
        .log("calmcp:oauth.revoke", json!({ "grant": grant.uuid }))
        .await;

    ApiResponse::new_serialized(json!({})).ok()
}
