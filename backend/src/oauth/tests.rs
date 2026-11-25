//! The OAuth flow end to end against an in-memory store: registration, consent, code exchange
//! with PKCE, refresh rotation, replay detection and revocation.

use super::{
    Scope, Urls,
    flow::{self, ApproveError, AuthorizeError, AuthorizeRequest, OAuthError},
    gate::challenge,
    params::{self, Params},
    routes::{authorization_server_document, protected_resource_document},
    store::{
        Client, Code, CodeOutcome, Grant, GrantSummary, NewGrant, RefreshOutcome, Store, Token,
    },
    validate,
};
use crate::{access::Access, panel::mock::MockPanel};
use axum::http::{Method, StatusCode};
use base64::Engine;
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Mutex};
use uuid::Uuid;

// RFC 7636 appendix B.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const REDIRECT: &str = "https://claude.ai/api/mcp/auth_callback";

struct StoredToken {
    grant: Uuid,
    refresh: bool,
    expires: DateTime<Utc>,
    used: bool,
}

#[derive(Default)]
struct Inner {
    clients: HashMap<Uuid, Client>,
    codes: HashMap<String, (Code, bool, Option<Uuid>)>,
    grants: HashMap<Uuid, Grant>,
    tokens: HashMap<String, StoredToken>,
    revoked: Vec<Uuid>,
}

#[derive(Default)]
struct MemoryStore(Mutex<Inner>);

impl MemoryStore {
    fn grant_count(&self) -> usize {
        self.0.lock().unwrap().grants.len()
    }

    fn revoked(&self) -> Vec<Uuid> {
        self.0.lock().unwrap().revoked.clone()
    }
}

#[async_trait::async_trait]
impl Store for MemoryStore {
    async fn insert_client(&self, client: &Client) -> Result<(), anyhow::Error> {
        self.0
            .lock()
            .unwrap()
            .clients
            .insert(client.uuid, client.clone());
        Ok(())
    }

    async fn client(&self, uuid: Uuid) -> Result<Option<Client>, anyhow::Error> {
        Ok(self.0.lock().unwrap().clients.get(&uuid).cloned())
    }

    async fn count_grants(&self, user_uuid: Uuid) -> Result<i64, anyhow::Error> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .grants
            .values()
            .filter(|grant| grant.user_uuid == user_uuid)
            .count() as i64)
    }

    async fn insert_code(&self, code: &Code) -> Result<(), anyhow::Error> {
        self.0
            .lock()
            .unwrap()
            .codes
            .insert(code.digest.clone(), (code.clone(), false, None));
        Ok(())
    }

    async fn take_code(
        &self,
        digest: &str,
        now: DateTime<Utc>,
    ) -> Result<CodeOutcome, anyhow::Error> {
        let mut inner = self.0.lock().unwrap();
        Ok(match inner.codes.get_mut(digest) {
            Some((_, true, grant)) => CodeOutcome::Replayed(*grant),
            Some((code, used, _)) if code.expires > now => {
                *used = true;
                CodeOutcome::Fresh(code.clone())
            }
            _ => CodeOutcome::Invalid,
        })
    }

    async fn create_grant(&self, new: &NewGrant) -> Result<Grant, anyhow::Error> {
        let mut inner = self.0.lock().unwrap();
        let grant = Grant {
            uuid: new.uuid,
            client_uuid: new.client_uuid,
            user_uuid: new.user_uuid,
            api_key_uuid: Uuid::new_v4(),
            scope: new.scope,
            resource: new.resource.clone(),
        };
        inner.grants.insert(grant.uuid, grant.clone());
        if let Some(code) = inner.codes.get_mut(&new.code_digest) {
            code.2 = Some(grant.uuid);
        }
        for (token, refresh) in [(&new.access, false), (&new.refresh, true)] {
            inner.tokens.insert(
                token.digest.clone(),
                StoredToken {
                    grant: grant.uuid,
                    refresh,
                    expires: token.expires,
                    used: false,
                },
            );
        }
        Ok(grant)
    }

    async fn rotate_refresh(
        &self,
        digest: &str,
        client_uuid: Uuid,
        requested: Option<Scope>,
        now: DateTime<Utc>,
        access: &Token,
        refresh: &Token,
    ) -> Result<RefreshOutcome, anyhow::Error> {
        let mut inner = self.0.lock().unwrap();
        let Some(token) = inner.tokens.get(digest).filter(|token| token.refresh) else {
            return Ok(RefreshOutcome::Invalid);
        };
        let Some(grant) = inner.grants.get(&token.grant).cloned() else {
            return Ok(RefreshOutcome::Invalid);
        };

        if grant.client_uuid != client_uuid {
            return Ok(RefreshOutcome::Invalid);
        }
        if token.used {
            return Ok(RefreshOutcome::Replayed(grant.uuid));
        }
        if token.expires <= now {
            return Ok(RefreshOutcome::Invalid);
        }
        if requested.is_some_and(|requested| requested > grant.scope) {
            return Ok(RefreshOutcome::ScopeExceeded);
        }

        inner.tokens.get_mut(digest).unwrap().used = true;
        for (token, refresh) in [(access, false), (refresh, true)] {
            inner.tokens.insert(
                token.digest.clone(),
                StoredToken {
                    grant: grant.uuid,
                    refresh,
                    expires: token.expires,
                    used: false,
                },
            );
        }
        Ok(RefreshOutcome::Rotated(grant))
    }

    async fn access(
        &self,
        digest: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<Grant>, anyhow::Error> {
        let inner = self.0.lock().unwrap();
        Ok(inner
            .tokens
            .get(digest)
            .filter(|token| !token.refresh && token.expires > now)
            .and_then(|token| inner.grants.get(&token.grant).cloned()))
    }

    async fn token_grant(&self, digest: &str) -> Result<Option<Grant>, anyhow::Error> {
        let inner = self.0.lock().unwrap();
        Ok(inner
            .tokens
            .get(digest)
            .and_then(|token| inner.grants.get(&token.grant).cloned()))
    }

    async fn revoke(&self, grant_uuid: Uuid) -> Result<(), anyhow::Error> {
        let mut inner = self.0.lock().unwrap();
        if inner.grants.remove(&grant_uuid).is_some() {
            inner.revoked.push(grant_uuid);
        }
        inner.tokens.retain(|_, token| token.grant != grant_uuid);
        for code in inner.codes.values_mut() {
            if code.2 == Some(grant_uuid) {
                code.2 = None;
            }
        }
        Ok(())
    }

    async fn list_grants(&self, _user_uuid: Uuid) -> Result<Vec<GrantSummary>, anyhow::Error> {
        unimplemented!("not used by the flow")
    }

    async fn user_grant(
        &self,
        _user_uuid: Uuid,
        _grant_uuid: Uuid,
    ) -> Result<Option<Grant>, anyhow::Error> {
        unimplemented!("not used by the flow")
    }
}

fn urls() -> Urls {
    Urls::new("https://panel.example.com").unwrap()
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 20, 12, 0, 0).unwrap()
}

fn form(pairs: &[(&str, &str)]) -> Params {
    let encoded: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish();
    Params::parse(&encoded)
}

fn query_of(url: &str) -> HashMap<String, String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

async fn register(store: &MemoryStore, auth_method: &str) -> Value {
    flow::register(
        store,
        &json!({
            "client_name": "Claude",
            "redirect_uris": [REDIRECT],
            "grant_types": ["authorization_code", "refresh_token"],
            "token_endpoint_auth_method": auth_method,
        }),
        now(),
    )
    .await
    .unwrap()
}

fn authorize_query(client_id: &str, extra: &[(&str, &str)]) -> String {
    let mut pairs = vec![
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", REDIRECT),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
        ("state", "xyz"),
        ("resource", "https://panel.example.com/api/client/extensions/dev.caloptreyx.calmcp/mcp"),
    ];
    pairs.extend_from_slice(extra);
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

async fn authorize(store: &MemoryStore, client_id: &str, scope: Scope, user: Uuid) -> String {
    let request = flow::authorize_request(store, &urls(), &authorize_query(client_id, &[]))
        .await
        .unwrap();
    let redirect = flow::approve(store, &urls(), &request, user, scope, now())
        .await
        .unwrap();

    let query = query_of(&redirect);
    assert!(redirect.starts_with(REDIRECT));
    assert_eq!(query["state"], "xyz");
    assert_eq!(query["iss"], urls().issuer);
    query["code"].clone()
}

async fn exchange(store: &MemoryStore, client_id: &str, code: &str) -> Result<Value, OAuthError> {
    flow::token(
        store,
        &urls(),
        None,
        &form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
        ]),
        now(),
    )
    .await
}

async fn refresh(
    store: &MemoryStore,
    client_id: &str,
    token: &str,
    at: DateTime<Utc>,
) -> Result<Value, OAuthError> {
    flow::token(
        store,
        &urls(),
        None,
        &form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", token),
        ]),
        at,
    )
    .await
}

async fn access_scope(store: &MemoryStore, token: &str, at: DateTime<Utc>) -> Option<Scope> {
    flow::access(store, &urls(), token, at)
        .await
        .unwrap()
        .map(|grant| grant.scope)
}

fn str_of<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().unwrap()
}

#[tokio::test]
async fn public_client_full_flow_with_refresh_rotation() {
    let store = MemoryStore::default();
    let user = Uuid::new_v4();

    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");
    assert!(client.get("client_secret").is_none());
    assert_eq!(client["token_endpoint_auth_method"], "none");

    let code = authorize(&store, client_id, Scope::Read, user).await;
    let tokens = exchange(&store, client_id, &code).await.unwrap();
    assert_eq!(tokens["token_type"], "Bearer");
    assert_eq!(tokens["scope"], "read");
    assert_eq!(tokens["expires_in"], super::ACCESS_TTL_SECONDS);

    let access = str_of(&tokens, "access_token");
    assert!(access.starts_with(super::ACCESS_PREFIX));
    assert_eq!(access_scope(&store, access, now()).await, Some(Scope::Read));

    // Rotation hands out a new pair; both access tokens stay valid until they expire.
    let later = now() + Duration::minutes(50);
    let rotated = refresh(&store, client_id, str_of(&tokens, "refresh_token"), later)
        .await
        .unwrap();
    assert_ne!(rotated["refresh_token"], tokens["refresh_token"]);
    assert_eq!(rotated["scope"], "read");
    assert_eq!(
        access_scope(&store, str_of(&rotated, "access_token"), later).await,
        Some(Scope::Read)
    );

    // The old refresh token coming back means it leaked: the whole grant goes.
    let replay = refresh(&store, client_id, str_of(&tokens, "refresh_token"), later).await;
    assert_eq!(replay.unwrap_err().error, "invalid_grant");
    assert_eq!(store.grant_count(), 0);
    assert_eq!(access_scope(&store, str_of(&rotated, "access_token"), later).await, None);
    assert!(
        refresh(&store, client_id, str_of(&rotated, "refresh_token"), later)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn access_tokens_expire_and_are_bound_to_the_resource() {
    let store = MemoryStore::default();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");
    let code = authorize(&store, client_id, Scope::Client, Uuid::new_v4()).await;
    let tokens = exchange(&store, client_id, &code).await.unwrap();
    let access = str_of(&tokens, "access_token");

    let expiry = now() + Duration::seconds(super::ACCESS_TTL_SECONDS);
    assert_eq!(access_scope(&store, access, expiry - Duration::seconds(1)).await, Some(Scope::Client));
    assert_eq!(access_scope(&store, access, expiry).await, None);

    // After the panel moves to another URL, old tokens no longer name this server.
    let moved = Urls::new("https://new.example.com").unwrap();
    assert!(flow::access(&store, &moved, access, now()).await.unwrap().is_none());

    // Refresh tokens are not access tokens.
    assert_eq!(access_scope(&store, str_of(&tokens, "refresh_token"), now()).await, None);
}

#[tokio::test]
async fn replayed_code_revokes_the_grant_it_produced() {
    let store = MemoryStore::default();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");
    let code = authorize(&store, client_id, Scope::Full, Uuid::new_v4()).await;

    let tokens = exchange(&store, client_id, &code).await.unwrap();
    assert_eq!(tokens["scope"], "full");

    let replay = exchange(&store, client_id, &code).await.unwrap_err();
    assert_eq!(replay.error, "invalid_grant");
    assert_eq!(store.revoked().len(), 1);
    assert_eq!(access_scope(&store, str_of(&tokens, "access_token"), now()).await, None);
}

#[tokio::test]
async fn code_exchange_checks_pkce_client_redirect_and_expiry() {
    let store = MemoryStore::default();
    let user = Uuid::new_v4();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");
    let other = register(&store, "none").await;
    let other_id = str_of(&other, "client_id");

    // Wrong verifier: refused, and the code is spent.
    let code = authorize(&store, client_id, Scope::Client, user).await;
    let wrong = flow::token(
        &store,
        &urls(),
        None,
        &form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code.as_str()),
            ("redirect_uri", REDIRECT),
            ("code_verifier", "x".repeat(43).as_str()),
        ]),
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(wrong.error, "invalid_grant");
    assert_eq!(exchange(&store, client_id, &code).await.unwrap_err().error, "invalid_grant");

    // Missing verifier.
    let code = authorize(&store, client_id, Scope::Client, user).await;
    let missing = flow::token(
        &store,
        &urls(),
        None,
        &form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code.as_str()),
            ("redirect_uri", REDIRECT),
        ]),
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(missing.error, "invalid_request");

    // Another client cannot redeem it.
    let code = authorize(&store, client_id, Scope::Client, user).await;
    assert_eq!(exchange(&store, other_id, &code).await.unwrap_err().error, "invalid_grant");

    // The redirect URI has to be repeated exactly.
    let code = authorize(&store, client_id, Scope::Client, user).await;
    let redirect = flow::token(
        &store,
        &urls(),
        None,
        &form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code.as_str()),
            ("redirect_uri", "https://claude.ai/other"),
            ("code_verifier", VERIFIER),
        ]),
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(redirect.error, "invalid_grant");

    // Codes live two minutes.
    let code = authorize(&store, client_id, Scope::Client, user).await;
    let late = flow::token(
        &store,
        &urls(),
        None,
        &form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code.as_str()),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
        ]),
        now() + Duration::seconds(super::CODE_TTL_SECONDS),
    )
    .await
    .unwrap_err();
    assert_eq!(late.error, "invalid_grant");

    // Unknown codes.
    assert_eq!(
        exchange(&store, client_id, "calmcp_ac_nope").await.unwrap_err().error,
        "invalid_grant"
    );
    assert_eq!(store.grant_count(), 0);
}

#[tokio::test]
async fn confidential_clients_must_authenticate_the_way_they_registered() {
    let store = MemoryStore::default();
    let user = Uuid::new_v4();

    let basic = register(&store, "client_secret_basic").await;
    let basic_id = str_of(&basic, "client_id").to_string();
    let secret = str_of(&basic, "client_secret").to_string();
    assert!(secret.starts_with(super::SECRET_PREFIX));
    assert_eq!(basic["client_secret_expires_at"], 0);

    let token = |basic_auth: Option<(String, String)>, extra: Vec<(&'static str, String)>, code: String| {
        let store = &store;
        async move {
            let mut pairs = vec![
                ("grant_type", "authorization_code".to_string()),
                ("code", code),
                ("redirect_uri", REDIRECT.to_string()),
                ("code_verifier", VERIFIER.to_string()),
            ];
            pairs.extend(extra);
            let pairs: Vec<(&str, &str)> = pairs.iter().map(|(n, v)| (*n, v.as_str())).collect();
            flow::token(store, &urls(), basic_auth, &form(&pairs), now()).await
        }
    };

    // No secret at all.
    let code = authorize(&store, &basic_id, Scope::Client, user).await;
    let error = token(None, vec![("client_id", basic_id.clone())], code).await.unwrap_err();
    assert_eq!((error.status, error.error), (StatusCode::UNAUTHORIZED, "invalid_client"));

    // Right secret, wrong method.
    let code = authorize(&store, &basic_id, Scope::Client, user).await;
    let error = token(
        None,
        vec![("client_id", basic_id.clone()), ("client_secret", secret.clone())],
        code,
    )
    .await
    .unwrap_err();
    assert_eq!(error.error, "invalid_client");

    // Wrong secret.
    let code = authorize(&store, &basic_id, Scope::Client, user).await;
    let error = token(Some((basic_id.clone(), "wrong".into())), vec![], code)
        .await
        .unwrap_err();
    assert_eq!(error.error, "invalid_client");

    // Right secret through Basic.
    let code = authorize(&store, &basic_id, Scope::Client, user).await;
    let tokens = token(Some((basic_id.clone(), secret.clone())), vec![], code)
        .await
        .unwrap();
    assert!(tokens["access_token"].is_string());

    // client_secret_post.
    let post = register(&store, "client_secret_post").await;
    let post_id = str_of(&post, "client_id").to_string();
    let post_secret = str_of(&post, "client_secret").to_string();
    let code = authorize(&store, &post_id, Scope::Client, user).await;
    let tokens = token(
        None,
        vec![("client_id", post_id.clone()), ("client_secret", post_secret)],
        code,
    )
    .await
    .unwrap();
    assert!(tokens["refresh_token"].is_string());

    // Registration without a method defaults to client_secret_basic (RFC 7591).
    let default = flow::register(&store, &json!({ "redirect_uris": [REDIRECT] }), now())
        .await
        .unwrap();
    assert_eq!(default["token_endpoint_auth_method"], "client_secret_basic");
    assert!(default["client_secret"].is_string());
}

#[tokio::test]
async fn refresh_cannot_widen_the_scope_or_cross_clients() {
    let store = MemoryStore::default();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");
    let other = register(&store, "none").await;
    let code = authorize(&store, client_id, Scope::Read, Uuid::new_v4()).await;
    let tokens = exchange(&store, client_id, &code).await.unwrap();
    let refresh_token = str_of(&tokens, "refresh_token");

    let wider = flow::token(
        &store,
        &urls(),
        None,
        &form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("scope", "full"),
        ]),
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(wider.error, "invalid_scope");

    let foreign = refresh(&store, str_of(&other, "client_id"), refresh_token, now())
        .await
        .unwrap_err();
    assert_eq!(foreign.error, "invalid_grant");

    // Neither attempt spent the token.
    assert!(refresh(&store, client_id, refresh_token, now()).await.is_ok());

    // A second use of the same refresh token is a replay.
    let tokens = refresh(&store, client_id, refresh_token, now()).await;
    assert!(tokens.is_err(), "already rotated");
}

#[tokio::test]
async fn refresh_tokens_expire() {
    let store = MemoryStore::default();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");
    let code = authorize(&store, client_id, Scope::Client, Uuid::new_v4()).await;
    let tokens = exchange(&store, client_id, &code).await.unwrap();

    let expired = now() + Duration::seconds(super::REFRESH_TTL_SECONDS);
    let error = refresh(&store, client_id, str_of(&tokens, "refresh_token"), expired)
        .await
        .unwrap_err();
    assert_eq!(error.error, "invalid_grant");
}

#[tokio::test]
async fn revocation_only_works_for_the_owning_client() {
    let store = MemoryStore::default();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");
    let other = register(&store, "none").await;
    let code = authorize(&store, client_id, Scope::Client, Uuid::new_v4()).await;
    let tokens = exchange(&store, client_id, &code).await.unwrap();
    let access = str_of(&tokens, "access_token");

    flow::revoke(
        &store,
        None,
        &form(&[("client_id", str_of(&other, "client_id")), ("token", access)]),
    )
    .await
    .unwrap();
    assert_eq!(access_scope(&store, access, now()).await, Some(Scope::Client));

    // Unknown tokens are accepted silently (RFC 7009 section 2.2).
    flow::revoke(
        &store,
        None,
        &form(&[("client_id", client_id), ("token", "calmcp_rt_unknown")]),
    )
    .await
    .unwrap();

    flow::revoke(
        &store,
        None,
        &form(&[
            ("client_id", client_id),
            ("token", str_of(&tokens, "refresh_token")),
            ("token_type_hint", "refresh_token"),
        ]),
    )
    .await
    .unwrap();
    assert_eq!(access_scope(&store, access, now()).await, None);
    assert_eq!(store.grant_count(), 0);
}

#[tokio::test]
async fn token_requests_are_validated() {
    let store = MemoryStore::default();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");

    let repeated = Params::parse(&format!(
        "grant_type=authorization_code&client_id={client_id}&code=a&code=b"
    ));
    let error = flow::token(&store, &urls(), None, &repeated, now()).await.unwrap_err();
    assert_eq!(error.error, "invalid_request");

    let error = flow::token(
        &store,
        &urls(),
        None,
        &form(&[("grant_type", "client_credentials"), ("client_id", client_id)]),
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.error, "unsupported_grant_type");

    let error = flow::token(
        &store,
        &urls(),
        None,
        &form(&[("grant_type", "authorization_code"), ("client_id", Uuid::new_v4().to_string().as_str())]),
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.error, "invalid_client");

    let error = flow::token(
        &store,
        &urls(),
        None,
        &form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("resource", "https://elsewhere.example.com/mcp"),
        ]),
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.error, "invalid_target");
}

async fn authorize_error(store: &MemoryStore, query: &str) -> AuthorizeError {
    flow::authorize_request(store, &urls(), query).await.unwrap_err()
}

fn redirected_error(error: AuthorizeError) -> HashMap<String, String> {
    match error {
        AuthorizeError::Redirect(url) => {
            assert!(url.starts_with(REDIRECT));
            let query = query_of(&url);
            assert_eq!(query["iss"], urls().issuer);
            query
        }
        AuthorizeError::Fatal(message) => panic!("expected a redirect, got {message}"),
    }
}

#[tokio::test]
async fn authorization_requests_never_redirect_to_unregistered_uris() {
    let store = MemoryStore::default();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");

    // Unknown client.
    let unknown = authorize_query(&Uuid::new_v4().to_string(), &[]);
    assert!(matches!(authorize_error(&store, &unknown).await, AuthorizeError::Fatal(_)));

    // A redirect URI that differs in any way, even a trailing slash.
    for redirect in [
        "https://claude.ai/api/mcp/auth_callback/",
        "https://claude.ai/api/mcp/auth_callback?x=1",
        "https://evil.example.com/callback",
    ] {
        let query = format!(
            "response_type=code&client_id={client_id}&redirect_uri={}&code_challenge={CHALLENGE}&code_challenge_method=S256",
            url::form_urlencoded::byte_serialize(redirect.as_bytes()).collect::<String>()
        );
        assert!(matches!(authorize_error(&store, &query).await, AuthorizeError::Fatal(_)));
    }

    // A repeated redirect_uri is not trusted either.
    let repeated = format!("{}&redirect_uri=https%3A%2F%2Fevil.example.com", authorize_query(client_id, &[]));
    assert!(matches!(authorize_error(&store, &repeated).await, AuthorizeError::Fatal(_)));

    // With exactly one registered URI it may be left out.
    let query = format!(
        "response_type=code&client_id={client_id}&code_challenge={CHALLENGE}&code_challenge_method=S256"
    );
    let request = flow::authorize_request(&store, &urls(), &query).await.unwrap();
    assert_eq!(request.redirect_uri, REDIRECT);
    assert!(!request.redirect_uri_explicit);
}

#[tokio::test]
async fn authorization_errors_go_back_to_the_client_with_state_and_iss() {
    let store = MemoryStore::default();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");
    let base = |skip: &str, extra: &[(&str, &str)]| {
        let mut pairs: Vec<(&str, &str)> = vec![
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", REDIRECT),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
            ("state", "s1"),
        ];
        pairs.retain(|(name, _)| *name != skip);
        pairs.extend_from_slice(extra);
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs)
            .finish()
    };

    let error = redirected_error(authorize_error(&store, &base("code_challenge", &[])).await);
    assert_eq!(error["error"], "invalid_request");
    assert_eq!(error["state"], "s1");

    let error = redirected_error(authorize_error(&store, &base("code_challenge_method", &[])).await);
    assert_eq!(error["error"], "invalid_request");

    let error = redirected_error(
        authorize_error(&store, &base("code_challenge_method", &[("code_challenge_method", "plain")])).await,
    );
    assert_eq!(error["error"], "invalid_request");

    let error = redirected_error(
        authorize_error(&store, &base("response_type", &[("response_type", "token")])).await,
    );
    assert_eq!(error["error"], "unsupported_response_type");

    let error = redirected_error(
        authorize_error(&store, &base("", &[("resource", "https://other.example.com/mcp")])).await,
    );
    assert_eq!(error["error"], "invalid_target");

    let error = redirected_error(authorize_error(&store, &base("", &[("state", "again")])).await);
    assert_eq!(error["error"], "invalid_request");

    // Declining sends access_denied.
    let request = flow::authorize_request(&store, &urls(), &base("", &[])).await.unwrap();
    let denied = query_of(&flow::deny(&urls(), &request));
    assert_eq!(denied["error"], "access_denied");
    assert_eq!(denied["state"], "s1");
}

#[tokio::test]
async fn authorization_requests_pick_the_broadest_known_scope() {
    let store = MemoryStore::default();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");

    let request = |scope: Option<&'static str>| {
        let store = &store;
        async move {
            let extra: Vec<(&str, &str)> = scope.map(|scope| ("scope", scope)).into_iter().collect();
            let request: AuthorizeRequest =
                flow::authorize_request(store, &urls(), &authorize_query(client_id, &extra))
                    .await
                    .unwrap();
            request.requested
        }
    };

    assert_eq!(request(None).await, Scope::Client);
    assert_eq!(request(Some("read")).await, Scope::Read);
    assert_eq!(request(Some("read full offline_access")).await, Scope::Full);
    assert_eq!(request(Some("openid profile")).await, Scope::Client);
}

#[tokio::test]
async fn users_have_a_cap_on_connected_clients() {
    let store = MemoryStore::default();
    let user = Uuid::new_v4();
    let client = register(&store, "none").await;
    let client_id = str_of(&client, "client_id");

    for _ in 0..super::MAX_GRANTS_PER_USER {
        let code = authorize(&store, client_id, Scope::Client, user).await;
        exchange(&store, client_id, &code).await.unwrap();
    }

    let request = flow::authorize_request(&store, &urls(), &authorize_query(client_id, &[]))
        .await
        .unwrap();
    assert_eq!(
        flow::approve(&store, &urls(), &request, user, Scope::Client, now()).await,
        Err(ApproveError::TooManyGrants)
    );
    assert!(
        flow::approve(&store, &urls(), &request, Uuid::new_v4(), Scope::Client, now())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn registration_rejects_unsafe_metadata() {
    let store = MemoryStore::default();
    let register = |metadata: Value| {
        let store = &store;
        async move { flow::register(store, &metadata, now()).await }
    };

    for uri in [
        "http://claude.ai/callback",
        "javascript:alert(1)",
        "data:text/html,hi",
        "ftp://claude.ai/callback",
        "wss://claude.ai/callback",
        "https://claude.ai/callback#frag",
        "https://user:pass@claude.ai/callback",
        "/relative/callback",
    ] {
        let error = register(json!({ "redirect_uris": [uri], "token_endpoint_auth_method": "none" }))
            .await
            .unwrap_err();
        assert_eq!(error.error, "invalid_redirect_uri", "{uri}");
    }

    for uri in [
        "http://localhost:6274/oauth/callback",
        "http://127.0.0.1:33418/callback",
        "http://[::1]:8080/cb",
        "cursor://anysphere.cursor-retrieval/oauth/callback",
        "vscode://vscode.github-authentication/did-authenticate",
        "com.example.app:/oauth",
        "https://chatgpt.com/connector_platform_oauth_redirect",
    ] {
        register(json!({ "redirect_uris": [uri], "token_endpoint_auth_method": "none" }))
            .await
            .unwrap_or_else(|err| panic!("{uri}: {err:?}"));
    }

    let error = register(json!({ "redirect_uris": [] })).await.unwrap_err();
    assert_eq!(error.error, "invalid_redirect_uri");
    let error = register(json!({ "client_name": "x" })).await.unwrap_err();
    assert_eq!(error.error, "invalid_redirect_uri");

    let error = register(json!({
        "redirect_uris": [REDIRECT],
        "grant_types": ["authorization_code", "client_credentials"],
    }))
    .await
    .unwrap_err();
    assert_eq!(error.error, "invalid_client_metadata");

    let error = register(json!({ "redirect_uris": [REDIRECT], "response_types": ["token"] }))
        .await
        .unwrap_err();
    assert_eq!(error.error, "invalid_client_metadata");

    let error = register(json!({ "redirect_uris": [REDIRECT], "token_endpoint_auth_method": "private_key_jwt" }))
        .await
        .unwrap_err();
    assert_eq!(error.error, "invalid_client_metadata");

    let error = register(json!({ "redirect_uris": [REDIRECT], "client_name": "bad\u{7}name" }))
        .await
        .unwrap_err();
    assert_eq!(error.error, "invalid_client_metadata");

    let too_many: Vec<String> = (0..11).map(|i| format!("https://claude.ai/cb/{i}")).collect();
    let error = register(json!({ "redirect_uris": too_many })).await.unwrap_err();
    assert_eq!(error.error, "invalid_redirect_uri");
}

#[test]
fn urls_follow_the_well_known_rules() {
    let urls = Urls::new("https://Panel.Example.com/").unwrap();
    assert_eq!(
        urls.resource,
        "https://panel.example.com/api/client/extensions/dev.caloptreyx.calmcp/mcp"
    );
    assert_eq!(
        urls.resource_metadata,
        "https://panel.example.com/.well-known/oauth-protected-resource/api/client/extensions/dev.caloptreyx.calmcp/mcp"
    );
    assert_eq!(
        urls.issuer,
        "https://panel.example.com/api/auth/extensions/dev.caloptreyx.calmcp/oauth"
    );
    assert_eq!(urls.token_endpoint, format!("{}/token", urls.issuer));
    assert_eq!(
        urls.authorization_endpoint,
        "https://panel.example.com/account/mcp/authorize"
    );

    let prefixed = Urls::new("https://example.com:8443/panel").unwrap();
    assert_eq!(
        prefixed.resource_metadata,
        "https://example.com:8443/.well-known/oauth-protected-resource/panel/api/client/extensions/dev.caloptreyx.calmcp/mcp"
    );
    assert!(prefixed.issuer.starts_with("https://example.com:8443/panel/api/auth/"));

    assert!(Urls::new("not a url").is_none());
    assert!(Urls::new("ftp://example.com").is_none());
}

#[test]
fn metadata_documents_advertise_what_the_flow_needs() {
    let urls = urls();

    let resource = protected_resource_document(&urls, "Cal MCP");
    assert_eq!(resource["resource"], urls.resource);
    assert_eq!(resource["authorization_servers"], json!([urls.issuer]));
    assert_eq!(resource["scopes_supported"], json!(["read", "client", "full"]));

    let server = authorization_server_document(&urls);
    assert_eq!(server["issuer"], urls.issuer);
    assert_eq!(server["code_challenge_methods_supported"], json!(["S256"]));
    assert_eq!(server["authorization_response_iss_parameter_supported"], true);
    assert_eq!(server["registration_endpoint"], urls.registration_endpoint);
    assert_eq!(server["revocation_endpoint"], urls.revocation_endpoint);
    assert_eq!(
        server["grant_types_supported"],
        json!(["authorization_code", "refresh_token"])
    );
    assert!(server.get("client_id_metadata_document_supported").is_none());
}

#[test]
fn challenge_points_at_the_resource_metadata() {
    let urls = urls();
    assert_eq!(
        challenge(&urls, None),
        format!("Bearer resource_metadata=\"{}\", scope=\"client\"", urls.resource_metadata)
    );
    assert!(
        challenge(&urls, Some(("invalid_token", "expired")))
            .ends_with(", error=\"invalid_token\", error_description=\"expired\"")
    );
}

#[test]
fn pkce_uses_s256() {
    assert!(validate::pkce_matches(VERIFIER, CHALLENGE));
    assert!(!validate::pkce_matches(VERIFIER, VERIFIER));
    assert!(!validate::pkce_matches("short", CHALLENGE));
    assert!(!validate::pkce_value(&"a".repeat(42)));
    assert!(validate::pkce_value(&"a".repeat(128)));
    assert!(!validate::pkce_value(&"a".repeat(129)));
    assert!(!validate::pkce_value(&format!("{}+", "a".repeat(43))));
}

#[test]
fn resource_comparison_tolerates_case_and_trailing_slash_only() {
    let resource = "https://panel.example.com/api/client/extensions/dev.caloptreyx.calmcp/mcp";
    assert!(validate::resource_matches(resource, resource));
    assert!(validate::resource_matches(
        "HTTPS://PANEL.example.com/api/client/extensions/dev.caloptreyx.calmcp/mcp/",
        resource
    ));
    assert!(!validate::resource_matches("https://panel.example.com", resource));
    assert!(!validate::resource_matches(
        "https://panel.example.com/api/client/extensions/dev.caloptreyx.calmcp/MCP",
        resource
    ));
    assert!(!validate::resource_matches(&format!("{resource}#x"), resource));
}

#[test]
fn credentials_are_parsed_strictly() {
    let encoded = base64::engine::general_purpose::STANDARD.encode("my%20id:s%3Acret");
    assert_eq!(
        params::basic_credentials(&format!("Basic {encoded}")),
        Some(("my id".into(), "s:cret".into()))
    );
    assert_eq!(params::basic_credentials("Bearer abc"), None);
    assert_eq!(params::basic_credentials("Basic !!!"), None);

    assert_eq!(params::bearer("Bearer abc"), Some("abc"));
    assert_eq!(params::bearer("bearer  abc "), Some("abc"));
    assert_eq!(params::bearer("Bearer "), None);
    assert_eq!(params::bearer("Basic abc"), None);

    let params = Params::parse("a=1&b=&a=2");
    assert_eq!(params.get("a"), Some("1"));
    assert_eq!(params.get("b"), None);
    assert!(params.is_repeated("a"));
    assert_eq!(params.first_repeated(), Some("a"));
}

#[test]
fn scopes_are_ordered_and_parsed() {
    assert!(Scope::Read < Scope::Client && Scope::Client < Scope::Full);
    assert_eq!(Scope::broadest("offline_access client"), Some(Scope::Client));
    assert_eq!(Scope::broadest("unknown"), None);
    assert_eq!(
        Scope::Read.access(),
        Access {
            read_only: true,
            client_only: true
        }
    );
    assert_eq!(
        Scope::Client.access(),
        Access {
            read_only: false,
            client_only: true
        }
    );
    assert_eq!(Scope::Full.access(), Access::NONE);
}

#[test]
fn a_broad_grant_never_loosens_the_key_restrictions() {
    let read_only_key = Access {
        read_only: true,
        client_only: false,
    };
    assert_eq!(read_only_key.union(Scope::Full.access()), read_only_key);
    assert_eq!(
        read_only_key.union(Scope::Client.access()),
        Scope::Read.access()
    );
}

async fn call_tool(ctx: &crate::tools::Ctx, name: &str, arguments: Value) -> Result<Value, String> {
    let mut params = serde_json::Map::new();
    params.insert("name".into(), json!(name));
    params.insert("arguments".into(), arguments);
    crate::tools::call(ctx, &params).await
}

#[tokio::test]
async fn a_read_grant_hides_and_refuses_writes_and_admin() {
    let access = Access::NONE.union(Scope::Read.access());
    let listed = crate::tools::list(access);
    assert!(
        listed
            .iter()
            .all(|tool| tool["annotations"]["readOnlyHint"] == json!(true))
    );

    let (ctx, panel) = MockPanel::default()
        .on_json(Method::GET, "/api/admin/users", json!({ "users": [] }))
        .ctx(access);

    let write = call_tool(
        &ctx,
        "write_file",
        json!({ "server": "abc", "file": "/a.txt", "content": "x" }),
    )
    .await;
    assert!(write.unwrap_err().contains("read-only"));

    let admin = call_tool(&ctx, "read_api", json!({ "path": "/api/admin/users" }))
        .await
        .unwrap();
    assert_eq!(admin["isError"], json!(true));
    assert!(panel.calls().is_empty());
}

#[tokio::test]
async fn a_client_grant_writes_to_the_client_area_only() {
    let (ctx, panel) = MockPanel::default()
        .on_json(Method::POST, "/api/client/servers/abc/power", json!({}))
        .on_json(Method::GET, "/api/admin/users", json!({ "users": [] }))
        .ctx(Access::NONE.union(Scope::Client.access()));

    let write = call_tool(
        &ctx,
        "write_api",
        json!({ "method": "POST", "path": "/api/client/servers/abc/power", "body": { "action": "start" } }),
    )
    .await
    .unwrap();
    assert_eq!(write["isError"], json!(false), "{write}");

    let admin = call_tool(&ctx, "read_api", json!({ "path": "/api/admin/users" }))
        .await
        .unwrap();
    assert_eq!(admin["isError"], json!(true));
    assert_eq!(panel.calls().len(), 1);
}

#[test]
fn consent_page_shows_where_the_code_goes() {
    assert_eq!(validate::redirect_host(REDIRECT), "claude.ai");
    assert_eq!(validate::redirect_host("http://localhost:6274/cb"), "localhost:6274");
    assert_eq!(
        validate::redirect_host("cursor://anysphere.cursor-retrieval/oauth/callback"),
        "anysphere.cursor-retrieval"
    );
    assert_eq!(validate::redirect_host("com.example.app:/oauth"), "com.example.app:");
    assert!(validate::is_loopback_redirect("http://127.0.0.1:1234/cb"));
    assert!(!validate::is_loopback_redirect(REDIRECT));
}

/// The user permissions a grant's api key holds. The panel answers 403 when the permission an
/// endpoint needs is not in the key's list (`CredentialScope::covers` is plain membership).
fn grant_key_user_permissions() -> Vec<String> {
    // A sample of what the panel defines; the real list is only filled in at runtime.
    let defined: Vec<String> = [
        "account.infos",
        "account.email",
        "account.password",
        "account.two-factor",
        "account.avatar",
        "servers.read",
        "api-keys.create",
        "api-keys.read",
        "api-keys.recreate",
        "ssh-keys.create",
        "ssh-keys.read",
        "security-keys.create",
        "sessions.read",
        "sessions.delete",
        "command-snippets.create",
        "activity.read",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    super::grant_user_permissions(&defined)
}

#[test]
fn grant_keys_cannot_create_api_keys_or_ssh_keys() {
    let granted = grant_key_user_permissions();
    let allowed = |permission: &str| granted.iter().any(|p| p == permission);

    // POST /api/client/account/api-keys, .../recreate and /ssh-keys check these; with them
    // missing, a client or full scope connection gets 403 and cannot mint a credential that
    // outlives disconnecting it.
    for permission in super::CREDENTIAL_PERMISSIONS {
        assert!(!allowed(permission), "{permission}");
    }
    for permission in ["servers.read", "account.infos", "api-keys.read", "ssh-keys.read", "sessions.read"] {
        assert!(allowed(permission), "{permission}");
    }
}

/// The options are deserialized, so a field the panel adds without a default would only fail
/// when a client is approved; this catches it at test time instead.
#[test]
fn grant_key_options_deserialize_on_this_panel() {
    use compact_str::CompactString;

    let user_uuid = uuid::Uuid::new_v4();
    let options = super::pg::api_key_options(
        user_uuid,
        "MCP Claude abc123".into(),
        vec![CompactString::from("servers.read")],
        Vec::new(),
        vec![CompactString::from("files.read")],
    )
    .unwrap();

    assert_eq!(options.user_uuid, user_uuid);
    assert_eq!(options.name, "MCP Claude abc123");
    assert!(options.allowed_ips.is_empty());
    assert!(options.enabled);
    assert_eq!(options.user_permissions, ["servers.read"]);
    assert!(options.admin_permissions.is_empty());
    assert_eq!(options.server_permissions, ["files.read"]);
    assert!(options.expires.is_none());
}

#[test]
fn oauth_connections_honour_the_keys_allowed_ips() {
    use super::gate::key_allows_ip;
    use std::net::IpAddr;

    let ip = |value: &str| value.parse::<IpAddr>().unwrap();
    let networks: Vec<sqlx::types::ipnetwork::IpNetwork> =
        vec!["10.0.0.0/8".parse().unwrap(), "2001:db8::/32".parse().unwrap()];

    assert!(key_allows_ip(&[], ip("203.0.113.9")));
    assert!(key_allows_ip(&networks, ip("10.1.2.3")));
    assert!(key_allows_ip(&networks, ip("2001:db8::1")));
    assert!(!key_allows_ip(&networks, ip("203.0.113.9")));
}

#[test]
fn consent_decisions_must_come_from_the_panel_origin() {
    use super::routes::cross_origin;
    use axum::http::{HeaderMap, HeaderValue};

    let urls = urls();
    let origin = url::Url::parse(&urls.issuer).unwrap().origin().ascii_serialization();
    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    assert!(!cross_origin(&headers(&[]), &urls));
    assert!(!cross_origin(
        &headers(&[("origin", origin.as_str()), ("sec-fetch-site", "same-origin")]),
        &urls
    ));
    assert!(cross_origin(&headers(&[("origin", "https://evil.example")]), &urls));
    // A sibling subdomain is the same site for the session cookie, but not the same origin.
    assert!(cross_origin(&headers(&[("sec-fetch-site", "same-site")]), &urls));
    assert!(cross_origin(
        &headers(&[("origin", origin.as_str()), ("sec-fetch-site", "cross-site")]),
        &urls
    ));
}

#[tokio::test]
async fn a_read_grant_cannot_fetch_console_or_upload_credentials() {
    let (ctx, panel) = MockPanel::default()
        .on_json(Method::GET, "/api/client/servers/0a1b2c3d/websocket", json!({ "token": "jwt" }))
        .on_json(Method::GET, "/api/client/servers/0a1b2c3d/files/upload", json!({ "url": "u" }))
        .ctx(Access::NONE.union(Scope::Read.access()));

    for path in [
        "/api/client/servers/0a1b2c3d/websocket",
        "/api/client/servers/0a1b2c3d/files/upload",
    ] {
        let result = call_tool(&ctx, "read_api", json!({ "path": path })).await.unwrap();
        assert_eq!(result["isError"], json!(true), "{path}: {result}");
    }
    assert!(panel.calls().is_empty());
}
