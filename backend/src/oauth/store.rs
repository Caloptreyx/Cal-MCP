use super::Scope;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// How a client authenticates at the token and revocation endpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMethod {
    /// Public client, identified by `client_id` alone and protected by PKCE.
    None,
    ClientSecretBasic,
    ClientSecretPost,
}

impl AuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthMethod::None => "none",
            AuthMethod::ClientSecretBasic => "client_secret_basic",
            AuthMethod::ClientSecretPost => "client_secret_post",
        }
    }

    pub fn parse(value: &str) -> Option<AuthMethod> {
        [
            AuthMethod::None,
            AuthMethod::ClientSecretBasic,
            AuthMethod::ClientSecretPost,
        ]
        .into_iter()
        .find(|method| method.as_str() == value)
    }
}

#[derive(Clone, Debug)]
pub struct Client {
    pub uuid: Uuid,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub auth_method: AuthMethod,
    pub secret_digest: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Code {
    pub digest: String,
    pub client_uuid: Uuid,
    pub user_uuid: Uuid,
    pub redirect_uri: String,
    /// Whether the authorization request named the redirect URI; the token request must then
    /// repeat it (RFC 6749 section 4.1.3).
    pub redirect_uri_explicit: bool,
    pub code_challenge: String,
    pub scope: Scope,
    pub expires: DateTime<Utc>,
}

pub enum CodeOutcome {
    /// The code was valid and is now spent.
    Fresh(Code),
    /// The code was already exchanged; the grant it produced, if any, has to be revoked.
    Replayed(Option<Uuid>),
    /// Unknown or expired.
    Invalid,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub digest: String,
    pub expires: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct NewGrant {
    pub uuid: Uuid,
    pub client_uuid: Uuid,
    pub client_name: String,
    pub user_uuid: Uuid,
    pub scope: Scope,
    pub resource: String,
    /// The code this grant was exchanged for, remembered for replay detection.
    pub code_digest: String,
    pub access: Token,
    pub refresh: Token,
}

#[derive(Clone, Debug)]
pub struct Grant {
    pub uuid: Uuid,
    pub client_uuid: Uuid,
    pub user_uuid: Uuid,
    pub api_key_uuid: Uuid,
    pub scope: Scope,
    pub resource: String,
}

pub enum RefreshOutcome {
    Rotated(Grant),
    /// A refresh token that was already rotated came back: it leaked, so the grant goes.
    Replayed(Uuid),
    /// The client asked for more than the grant holds.
    ScopeExceeded,
    /// Unknown, expired, or issued to another client.
    Invalid,
}

/// A grant as shown under Account -> MCP.
#[derive(Clone, Debug, serde::Serialize)]
pub struct GrantSummary {
    pub uuid: Uuid,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub scope: Scope,
    pub created: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
}

/// Persistence for the OAuth flow. The panel implementation lives in `pg.rs`; the tests use an
/// in-memory one. Every method that consumes something does so atomically.
#[async_trait::async_trait]
pub trait Store: Send + Sync {
    async fn insert_client(&self, client: &Client) -> Result<(), anyhow::Error>;
    async fn client(&self, uuid: Uuid) -> Result<Option<Client>, anyhow::Error>;

    async fn count_grants(&self, user_uuid: Uuid) -> Result<i64, anyhow::Error>;

    async fn insert_code(&self, code: &Code) -> Result<(), anyhow::Error>;
    async fn take_code(&self, digest: &str, now: DateTime<Utc>)
    -> Result<CodeOutcome, anyhow::Error>;

    /// Creates the grant, its backing panel API key and its first tokens in one transaction.
    async fn create_grant(&self, grant: &NewGrant) -> Result<Grant, anyhow::Error>;

    /// Spends a refresh token issued to `client_uuid` and stores its successors.
    async fn rotate_refresh(
        &self,
        digest: &str,
        client_uuid: Uuid,
        requested: Option<Scope>,
        now: DateTime<Utc>,
        access: &Token,
        refresh: &Token,
    ) -> Result<RefreshOutcome, anyhow::Error>;

    /// The grant behind a live access token.
    async fn access(&self, digest: &str, now: DateTime<Utc>)
    -> Result<Option<Grant>, anyhow::Error>;

    /// The grant behind any token (access or refresh, used or not), for revocation.
    async fn token_grant(&self, digest: &str) -> Result<Option<Grant>, anyhow::Error>;

    /// Ends a grant by deleting its API key, which takes the grant and its tokens with it.
    async fn revoke(&self, grant_uuid: Uuid) -> Result<(), anyhow::Error>;

    async fn list_grants(&self, user_uuid: Uuid) -> Result<Vec<GrantSummary>, anyhow::Error>;
    async fn user_grant(
        &self,
        user_uuid: Uuid,
        grant_uuid: Uuid,
    ) -> Result<Option<Grant>, anyhow::Error>;
}
