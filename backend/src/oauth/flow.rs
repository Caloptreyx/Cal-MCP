//! The OAuth flow itself, free of HTTP and database details so it can be tested end to end
//! against an in-memory [`Store`].

use super::{
    ACCESS_PREFIX, ACCESS_TTL_SECONDS, CODE_PREFIX, CODE_TTL_SECONDS, MAX_GRANTS_PER_USER,
    REFRESH_PREFIX, REFRESH_TTL_SECONDS, SECRET_PREFIX, Scope, Urls, digest, generate,
    params::Params,
    store::{
        AuthMethod, Client, Code, CodeOutcome, Grant, NewGrant, RefreshOutcome, Store, Token,
    },
    validate,
};
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

/// An error response of the token, revocation or registration endpoint (RFC 6749 section 5.2,
/// RFC 7591 section 3.2.2).
#[derive(Debug, PartialEq, Eq)]
pub struct OAuthError {
    pub status: StatusCode,
    pub error: &'static str,
    pub description: String,
}

impl OAuthError {
    fn new(status: StatusCode, error: &'static str, description: impl Into<String>) -> Self {
        Self {
            status,
            error,
            description: description.into(),
        }
    }

    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", description)
    }

    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "invalid_client", description)
    }

    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_grant", description)
    }

    fn invalid_scope(description: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_scope", description)
    }

    fn invalid_target(description: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_target", description)
    }

    fn unsupported_grant_type(description: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "unsupported_grant_type", description)
    }

    fn invalid_redirect_uri(description: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_redirect_uri", description)
    }

    fn invalid_client_metadata(description: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_client_metadata", description)
    }

    pub fn server_error(err: anyhow::Error) -> Self {
        tracing::error!("Cal MCP OAuth: {err:#}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "The authorization server could not complete the request",
        )
    }

    pub fn body(&self) -> Value {
        json!({ "error": self.error, "error_description": self.description })
    }
}

impl From<anyhow::Error> for OAuthError {
    fn from(err: anyhow::Error) -> Self {
        Self::server_error(err)
    }
}

const GRANT_TYPES: [&str; 2] = ["authorization_code", "refresh_token"];

fn string_field<'a>(
    metadata: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, OAuthError> {
    match metadata.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(OAuthError::invalid_client_metadata(format!(
            "{name} must be a string"
        ))),
    }
}

fn strings_field(
    metadata: &serde_json::Map<String, Value>,
    name: &str,
    default: &[&str],
) -> Result<Vec<String>, OAuthError> {
    match metadata.get(name) {
        None | Some(Value::Null) => Ok(default.iter().map(|value| value.to_string()).collect()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value.as_str().map(str::to_string).ok_or_else(|| {
                    OAuthError::invalid_client_metadata(format!("{name} must list strings"))
                })
            })
            .collect(),
        Some(_) => Err(OAuthError::invalid_client_metadata(format!(
            "{name} must be an array"
        ))),
    }
}

/// Dynamic client registration (RFC 7591). Returns the client information response.
pub async fn register(
    store: &dyn Store,
    metadata: &Value,
    now: DateTime<Utc>,
) -> Result<Value, OAuthError> {
    let metadata = metadata
        .as_object()
        .ok_or_else(|| OAuthError::invalid_client_metadata("the body must be a JSON object"))?;

    let redirect_uris: Vec<String> = match metadata.get("redirect_uris") {
        Some(Value::Array(uris)) if !uris.is_empty() => uris
            .iter()
            .map(|uri| {
                uri.as_str().map(str::to_string).ok_or_else(|| {
                    OAuthError::invalid_redirect_uri("redirect_uris must be strings")
                })
            })
            .collect::<Result<_, _>>()?,
        _ => {
            return Err(OAuthError::invalid_redirect_uri(
                "redirect_uris must list at least one URI",
            ));
        }
    };
    if redirect_uris.len() > validate::MAX_REDIRECT_URIS {
        return Err(OAuthError::invalid_redirect_uri(format!(
            "at most {} redirect URIs are allowed",
            validate::MAX_REDIRECT_URIS
        )));
    }
    for uri in &redirect_uris {
        validate::redirect_uri(uri).map_err(OAuthError::invalid_redirect_uri)?;
    }

    let name = validate::client_name(string_field(metadata, "client_name")?)
        .map_err(OAuthError::invalid_client_metadata)?;

    // RFC 7591 section 2: the default is client_secret_basic.
    let auth_method = match string_field(metadata, "token_endpoint_auth_method")? {
        None => AuthMethod::ClientSecretBasic,
        Some(value) => AuthMethod::parse(value).ok_or_else(|| {
            OAuthError::invalid_client_metadata(format!(
                "token_endpoint_auth_method {value} is not supported; use none, client_secret_basic or client_secret_post"
            ))
        })?,
    };

    let grant_types = strings_field(metadata, "grant_types", &["authorization_code"])?;
    if let Some(unsupported) = grant_types
        .iter()
        .find(|grant_type| !GRANT_TYPES.contains(&grant_type.as_str()))
    {
        return Err(OAuthError::invalid_client_metadata(format!(
            "grant type {unsupported} is not supported"
        )));
    }
    if !grant_types.iter().any(|grant_type| grant_type == "authorization_code") {
        return Err(OAuthError::invalid_client_metadata(
            "grant_types must include authorization_code",
        ));
    }

    let response_types = strings_field(metadata, "response_types", &["code"])?;
    if response_types.iter().any(|response_type| response_type != "code") {
        return Err(OAuthError::invalid_client_metadata(
            "only the code response type is supported",
        ));
    }

    let secret = (auth_method != AuthMethod::None).then(|| generate(SECRET_PREFIX));
    let client = Client {
        uuid: Uuid::new_v4(),
        name,
        redirect_uris,
        auth_method,
        secret_digest: secret.as_deref().map(digest),
    };
    store.insert_client(&client).await?;

    let mut response = json!({
        "client_id": client.uuid.to_string(),
        "client_id_issued_at": now.timestamp(),
        "client_name": client.name,
        "redirect_uris": client.redirect_uris,
        "grant_types": grant_types,
        "response_types": ["code"],
        "token_endpoint_auth_method": auth_method.as_str(),
    });
    if let Some(secret) = secret {
        response["client_secret"] = json!(secret);
        response["client_secret_expires_at"] = json!(0);
    }

    Ok(response)
}

/// A validated authorization request, ready for the consent page.
#[derive(Clone, Debug)]
pub struct AuthorizeRequest {
    pub client: Client,
    pub redirect_uri: String,
    pub redirect_uri_explicit: bool,
    pub state: Option<String>,
    pub code_challenge: String,
    /// What the client asked for; the user may pick anything on the consent page.
    pub requested: Scope,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthorizeError {
    /// The client or redirect URI cannot be trusted, so the user sees the error and is not
    /// redirected (RFC 6749 section 4.1.2.1).
    Fatal(String),
    /// The error goes back to the client through its redirect URI.
    Redirect(String),
}

/// Appends parameters to a redirect URI, keeping its own query.
pub fn redirect_with(redirect_uri: &str, pairs: &[(&str, &str)]) -> String {
    match url::Url::parse(redirect_uri) {
        Ok(mut url) => {
            {
                let mut query = url.query_pairs_mut();
                for (name, value) in pairs {
                    query.append_pair(name, value);
                }
            }
            url.to_string()
        }
        Err(_) => redirect_uri.to_string(),
    }
}

fn error_redirect(
    urls: &Urls,
    redirect_uri: &str,
    state: Option<&str>,
    error: &str,
    description: &str,
) -> String {
    let mut pairs = vec![("error", error), ("error_description", description)];
    if let Some(state) = state {
        pairs.push(("state", state));
    }
    pairs.push(("iss", urls.issuer.as_str()));
    redirect_with(redirect_uri, &pairs)
}

/// Validates the query of an authorization request (RFC 6749 section 4.1.1 with PKCE and
/// RFC 8707 resource indicators).
pub async fn authorize_request(
    store: &dyn Store,
    urls: &Urls,
    query: &str,
) -> Result<AuthorizeRequest, AuthorizeError> {
    let params = Params::parse(query);

    if params.is_repeated("client_id") || params.is_repeated("redirect_uri") {
        return Err(AuthorizeError::Fatal(
            "The request repeats client_id or redirect_uri.".into(),
        ));
    }

    let client = match params.get("client_id").and_then(|id| Uuid::parse_str(id).ok()) {
        Some(uuid) => store
            .client(uuid)
            .await
            .map_err(|err| {
                tracing::error!("Cal MCP OAuth: {err:#}");
                AuthorizeError::Fatal("The panel could not load the client.".into())
            })?
            .ok_or_else(|| {
                AuthorizeError::Fatal(
                    "This client is not registered with the panel. Reconnect it from the app you started in.".into(),
                )
            })?,
        None => {
            return Err(AuthorizeError::Fatal(
                "The request has no valid client_id.".into(),
            ));
        }
    };

    let (redirect_uri, redirect_uri_explicit) = match params.get("redirect_uri") {
        Some(uri) if client.redirect_uris.iter().any(|registered| registered == uri) => {
            (uri.to_string(), true)
        }
        Some(_) => {
            return Err(AuthorizeError::Fatal(
                "The redirect_uri does not exactly match one the client registered.".into(),
            ));
        }
        None if client.redirect_uris.len() == 1 => (client.redirect_uris[0].clone(), false),
        None => {
            return Err(AuthorizeError::Fatal(
                "The request has no redirect_uri.".into(),
            ));
        }
    };

    let state = params.get("state");
    let fail = |error: &str, description: &str| {
        AuthorizeError::Redirect(error_redirect(
            urls,
            &redirect_uri,
            state.filter(|state| state.len() <= validate::MAX_STATE_LEN),
            error,
            description,
        ))
    };

    if let Some(repeated) = params.first_repeated() {
        return Err(fail(
            "invalid_request",
            &format!("the {repeated} parameter is repeated"),
        ));
    }
    if state.is_some_and(|state| state.len() > validate::MAX_STATE_LEN) {
        return Err(fail("invalid_request", "state is too long"));
    }
    if params.get("response_type") != Some("code") {
        return Err(fail(
            "unsupported_response_type",
            "only response_type=code is supported",
        ));
    }

    let code_challenge = match params.get("code_challenge") {
        Some(challenge) if validate::pkce_value(challenge) => challenge.to_string(),
        Some(_) => return Err(fail("invalid_request", "code_challenge is malformed")),
        None => return Err(fail("invalid_request", "PKCE is required: send code_challenge")),
    };
    if params.get("code_challenge_method") != Some("S256") {
        return Err(fail(
            "invalid_request",
            "code_challenge_method must be S256",
        ));
    }

    if let Some(resource) = params.get("resource")
        && !validate::resource_matches(resource, &urls.resource)
    {
        return Err(fail(
            "invalid_target",
            &format!("this server only issues tokens for {}", urls.resource),
        ));
    }

    let requested = params
        .get("scope")
        .and_then(Scope::broadest)
        .unwrap_or(Scope::DEFAULT);

    Ok(AuthorizeRequest {
        client,
        redirect_uri,
        redirect_uri_explicit,
        state: state.map(str::to_string),
        code_challenge,
        requested,
    })
}

/// The redirect for a user who declined.
pub fn deny(urls: &Urls, request: &AuthorizeRequest) -> String {
    error_redirect(
        urls,
        &request.redirect_uri,
        request.state.as_deref(),
        "access_denied",
        "the user declined the request",
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum ApproveError {
    TooManyGrants,
    Store(String),
}

/// Issues a single-use authorization code for an approved request and returns the redirect.
pub async fn approve(
    store: &dyn Store,
    urls: &Urls,
    request: &AuthorizeRequest,
    user_uuid: Uuid,
    scope: Scope,
    now: DateTime<Utc>,
) -> Result<String, ApproveError> {
    let store_error = |err: anyhow::Error| {
        tracing::error!("Cal MCP OAuth: {err:#}");
        ApproveError::Store("The panel could not save the authorization.".into())
    };

    if store.count_grants(user_uuid).await.map_err(store_error)? >= MAX_GRANTS_PER_USER {
        return Err(ApproveError::TooManyGrants);
    }

    let code = generate(CODE_PREFIX);
    store
        .insert_code(&Code {
            digest: digest(&code),
            client_uuid: request.client.uuid,
            user_uuid,
            redirect_uri: request.redirect_uri.clone(),
            redirect_uri_explicit: request.redirect_uri_explicit,
            code_challenge: request.code_challenge.clone(),
            scope,
            expires: now + Duration::seconds(CODE_TTL_SECONDS),
        })
        .await
        .map_err(store_error)?;

    let mut pairs = vec![("code", code.as_str())];
    if let Some(state) = &request.state {
        pairs.push(("state", state.as_str()));
    }
    pairs.push(("iss", urls.issuer.as_str()));

    Ok(redirect_with(&request.redirect_uri, &pairs))
}

/// Client authentication at the token and revocation endpoints (RFC 6749 section 2.3).
pub async fn authenticate(
    store: &dyn Store,
    basic: Option<(String, String)>,
    params: &Params,
) -> Result<Client, OAuthError> {
    let body_secret = params.get("client_secret");
    let (client_id, secret, method) = match basic {
        Some((id, secret)) => {
            if body_secret.is_some() {
                return Err(OAuthError::invalid_request(
                    "use one client authentication method, not two",
                ));
            }
            if params.get("client_id").is_some_and(|body_id| body_id != id) {
                return Err(OAuthError::invalid_request(
                    "client_id does not match the Authorization header",
                ));
            }
            (id, Some(secret), AuthMethod::ClientSecretBasic)
        }
        None => match params.get("client_id") {
            Some(id) => (
                id.to_string(),
                body_secret.map(str::to_string),
                if body_secret.is_some() {
                    AuthMethod::ClientSecretPost
                } else {
                    AuthMethod::None
                },
            ),
            None => return Err(OAuthError::invalid_client("client_id is missing")),
        },
    };

    let client = match Uuid::parse_str(&client_id) {
        Ok(uuid) => store.client(uuid).await?,
        Err(_) => None,
    }
    .ok_or_else(|| OAuthError::invalid_client("unknown client"))?;

    if client.auth_method != method {
        return Err(OAuthError::invalid_client(format!(
            "this client authenticates with {}",
            client.auth_method.as_str()
        )));
    }

    if method != AuthMethod::None {
        let matches = match (secret, &client.secret_digest) {
            (Some(secret), Some(expected)) => {
                validate::constant_time_eq(digest(&secret).as_bytes(), expected.as_bytes())
            }
            _ => false,
        };
        if !matches {
            return Err(OAuthError::invalid_client("wrong client secret"));
        }
    }

    Ok(client)
}

fn new_tokens(now: DateTime<Utc>) -> ((String, Token), (String, Token)) {
    let access = generate(ACCESS_PREFIX);
    let refresh = generate(REFRESH_PREFIX);

    (
        (
            access.clone(),
            Token {
                digest: digest(&access),
                expires: now + Duration::seconds(ACCESS_TTL_SECONDS),
            },
        ),
        (
            refresh.clone(),
            Token {
                digest: digest(&refresh),
                expires: now + Duration::seconds(REFRESH_TTL_SECONDS),
            },
        ),
    )
}

fn token_response(access: &str, refresh: &str, scope: Scope) -> Value {
    json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ACCESS_TTL_SECONDS,
        "refresh_token": refresh,
        "scope": scope.as_str(),
    })
}

/// The token endpoint (RFC 6749 section 3.2) for the authorization_code and refresh_token
/// grants.
pub async fn token(
    store: &dyn Store,
    urls: &Urls,
    basic: Option<(String, String)>,
    params: &Params,
    now: DateTime<Utc>,
) -> Result<Value, OAuthError> {
    if let Some(repeated) = params.first_repeated() {
        return Err(OAuthError::invalid_request(format!(
            "the {repeated} parameter is repeated"
        )));
    }

    let grant_type = params
        .get("grant_type")
        .ok_or_else(|| OAuthError::invalid_request("grant_type is missing"))?;
    if !GRANT_TYPES.contains(&grant_type) {
        return Err(OAuthError::unsupported_grant_type(format!(
            "grant type {grant_type} is not supported"
        )));
    }

    let client = authenticate(store, basic, params).await?;

    if let Some(resource) = params.get("resource")
        && !validate::resource_matches(resource, &urls.resource)
    {
        return Err(OAuthError::invalid_target(format!(
            "this server only issues tokens for {}",
            urls.resource
        )));
    }

    if grant_type == "authorization_code" {
        exchange_code(store, urls, &client, params, now).await
    } else {
        refresh(store, &client, params, now).await
    }
}

async fn exchange_code(
    store: &dyn Store,
    urls: &Urls,
    client: &Client,
    params: &Params,
    now: DateTime<Utc>,
) -> Result<Value, OAuthError> {
    let code = params
        .get("code")
        .ok_or_else(|| OAuthError::invalid_request("code is missing"))?;
    let code_digest = digest(code);

    let code = match store.take_code(&code_digest, now).await? {
        CodeOutcome::Fresh(code) => code,
        CodeOutcome::Replayed(grant) => {
            if let Some(grant) = grant {
                tracing::warn!("Cal MCP OAuth: authorization code replayed, revoking grant {grant}");
                store.revoke(grant).await?;
            }
            return Err(OAuthError::invalid_grant(
                "the authorization code was already used",
            ));
        }
        CodeOutcome::Invalid => {
            return Err(OAuthError::invalid_grant(
                "the authorization code is invalid or expired",
            ));
        }
    };

    if code.client_uuid != client.uuid {
        return Err(OAuthError::invalid_grant(
            "the authorization code was issued to another client",
        ));
    }

    match params.get("redirect_uri") {
        Some(uri) if uri != code.redirect_uri => {
            return Err(OAuthError::invalid_grant(
                "redirect_uri does not match the authorization request",
            ));
        }
        None if code.redirect_uri_explicit => {
            return Err(OAuthError::invalid_request(
                "redirect_uri is required because the authorization request sent it",
            ));
        }
        _ => {}
    }

    let verifier = params
        .get("code_verifier")
        .ok_or_else(|| OAuthError::invalid_request("code_verifier is missing"))?;
    if !validate::pkce_matches(verifier, &code.code_challenge) {
        return Err(OAuthError::invalid_grant(
            "code_verifier does not match the code challenge",
        ));
    }

    let ((access, access_token), (refresh, refresh_token)) = new_tokens(now);
    let grant = store
        .create_grant(&NewGrant {
            uuid: Uuid::new_v4(),
            client_uuid: client.uuid,
            client_name: client.name.clone(),
            user_uuid: code.user_uuid,
            scope: code.scope,
            resource: urls.resource.clone(),
            code_digest,
            access: access_token,
            refresh: refresh_token,
        })
        .await?;

    Ok(token_response(&access, &refresh, grant.scope))
}

async fn refresh(
    store: &dyn Store,
    client: &Client,
    params: &Params,
    now: DateTime<Utc>,
) -> Result<Value, OAuthError> {
    let token = params
        .get("refresh_token")
        .ok_or_else(|| OAuthError::invalid_request("refresh_token is missing"))?;
    let requested = params.get("scope").and_then(Scope::broadest);

    let ((access, access_token), (refresh, refresh_token)) = new_tokens(now);
    match store
        .rotate_refresh(
            &digest(token),
            client.uuid,
            requested,
            now,
            &access_token,
            &refresh_token,
        )
        .await?
    {
        RefreshOutcome::Rotated(grant) => Ok(token_response(&access, &refresh, grant.scope)),
        RefreshOutcome::Replayed(grant) => {
            tracing::warn!("Cal MCP OAuth: refresh token replayed, revoking grant {grant}");
            store.revoke(grant).await?;
            Err(OAuthError::invalid_grant("the refresh token was already used"))
        }
        RefreshOutcome::ScopeExceeded => Err(OAuthError::invalid_scope(
            "the requested scope is broader than the grant",
        )),
        RefreshOutcome::Invalid => Err(OAuthError::invalid_grant(
            "the refresh token is invalid or expired",
        )),
    }
}

/// Token revocation (RFC 7009). Revoking either token of a grant ends the whole grant. Unknown
/// tokens and tokens of other clients are ignored, as the RFC asks.
pub async fn revoke(
    store: &dyn Store,
    basic: Option<(String, String)>,
    params: &Params,
) -> Result<(), OAuthError> {
    let client = authenticate(store, basic, params).await?;
    let token = params
        .get("token")
        .ok_or_else(|| OAuthError::invalid_request("token is missing"))?;

    if let Some(grant) = store.token_grant(&digest(token)).await?
        && grant.client_uuid == client.uuid
    {
        store.revoke(grant.uuid).await?;
    }

    Ok(())
}

/// The grant an access token stands for, if the token is live and was issued for this server.
pub async fn access(
    store: &dyn Store,
    urls: &Urls,
    token: &str,
    now: DateTime<Utc>,
) -> Result<Option<Grant>, anyhow::Error> {
    if !token.starts_with(ACCESS_PREFIX) {
        return Ok(None);
    }

    Ok(store
        .access(&digest(token), now)
        .await?
        .filter(|grant| validate::resource_matches(&grant.resource, &urls.resource)))
}
