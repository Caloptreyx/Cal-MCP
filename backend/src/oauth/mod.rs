//! OAuth 2.1 authorization for the MCP endpoint, following the MCP authorization spec: protected
//! resource metadata (RFC 9728), authorization server metadata (RFC 8414), dynamic client
//! registration (RFC 7591), authorization code with PKCE S256, refresh token rotation and token
//! revocation (RFC 7009).
//!
//! Every grant is backed by a panel API key created for the approving user, so OAuth tokens act
//! through the panel's own permission checks. The key's plaintext is never stored or handed out.

mod flow;
mod gate;
mod params;
mod pg;
mod routes;
mod store;
mod validate;

#[cfg(test)]
mod tests;

use crate::access::Access;
use rand::distr::SampleString;
use serde::{Deserialize, Serialize};
use shared::State;
use utoipa_axum::router::OpenApiRouter;

pub use gate::OAuthGrant;

/// Where MCP clients connect. The route lives on the global router so it can answer OAuth
/// challenges; panel API keys are forwarded to [`INNER_PATH`] through the panel's own auth.
pub const MCP_PATH: &str = "/api/client/extensions/dev.caloptreyx.calmcp/mcp";
/// The client API route that runs MCP requests once the panel has authenticated them.
pub const INNER_PATH: &str = "/api/client/extensions/dev.caloptreyx.calmcp/mcp/inner";
/// Path of the authorization server's issuer identifier; token, registration and revocation
/// endpoints live below it.
pub const ISSUER_PATH: &str = "/api/auth/extensions/dev.caloptreyx.calmcp/oauth";
/// The consent page in the panel frontend.
pub const AUTHORIZE_PAGE: &str = "/account/mcp/authorize";

pub const ACCESS_PREFIX: &str = "calmcp_at_";
pub const REFRESH_PREFIX: &str = "calmcp_rt_";
pub const CODE_PREFIX: &str = "calmcp_ac_";
pub const SECRET_PREFIX: &str = "calmcp_cs_";

pub const ACCESS_TTL_SECONDS: i64 = 60 * 60;
pub const REFRESH_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const CODE_TTL_SECONDS: i64 = 120;
/// Connected clients per user; each one holds a panel API key.
pub const MAX_GRANTS_PER_USER: i64 = 25;
/// Token rows kept per grant when it refreshes, so a client refreshing in a loop cannot grow
/// the table without bound.
pub const KEPT_ACCESS_TOKENS: i64 = 5;
pub const KEPT_SPENT_REFRESH_TOKENS: i64 = 20;

/// User permissions an OAuth grant's API key never gets: everything that creates or changes a
/// way to sign in or call the panel. With them, a connected client could mint an API key or SSH
/// key that outlives disconnecting it, or lock the user out. Reading them stays allowed. Anyone
/// who wants an MCP client to manage credentials can connect it with a normal API key instead.
pub const CREDENTIAL_PERMISSIONS: [&str; 17] = [
    "account.email",
    "account.password",
    "account.password-login",
    "account.two-factor",
    "api-keys.create",
    "api-keys.update",
    "api-keys.delete",
    "api-keys.recreate",
    "ssh-keys.create",
    "ssh-keys.update",
    "ssh-keys.delete",
    "security-keys.create",
    "security-keys.update",
    "security-keys.delete",
    "oauth-links.create",
    "oauth-links.delete",
    "sessions.delete",
];

/// The user permissions of a grant's API key, out of every one the panel defines.
pub fn grant_user_permissions<'a>(all: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut permissions: Vec<String> = all
        .into_iter()
        .filter(|permission| !CREDENTIAL_PERMISSIONS.contains(&permission.as_str()))
        .cloned()
        .collect();
    permissions.sort_unstable();
    permissions
}

/// What a grant allows, from narrowest to broadest. A broader scope includes the narrower ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Reading the client area only: GET requests and read tools, no /api/admin.
    Read,
    /// Everything in the client area, no /api/admin.
    Client,
    /// Everything the user's account can do, including admin endpoints it has access to.
    Full,
}

impl Scope {
    pub const ALL: [Scope; 3] = [Scope::Read, Scope::Client, Scope::Full];
    /// Offered when the client asks for nothing we know.
    pub const DEFAULT: Scope = Scope::Client;

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Client => "client",
            Scope::Full => "full",
        }
    }

    pub fn parse(value: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|scope| scope.as_str() == value)
    }

    /// The broadest known scope in a space-separated scope parameter. Unknown values such as
    /// `offline_access` or `openid` are ignored rather than rejected, so generic clients work.
    pub fn broadest(requested: &str) -> Option<Scope> {
        requested.split(' ').filter_map(Scope::parse).max()
    }

    /// The restrictions this scope adds to the connection; they are combined with the
    /// panel-wide and per-key ones through [`Access::union`].
    pub fn access(self) -> Access {
        match self {
            Scope::Read => Access {
                read_only: true,
                client_only: true,
            },
            Scope::Client => Access {
                read_only: false,
                client_only: true,
            },
            Scope::Full => Access::NONE,
        }
    }
}

/// The absolute URLs this extension publishes, derived from the panel's app URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Urls {
    /// The MCP endpoint, which is also the OAuth resource identifier (RFC 8707).
    pub resource: String,
    pub resource_metadata: String,
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: String,
    pub revocation_endpoint: String,
}

impl Urls {
    pub fn new(app_url: &str) -> Option<Urls> {
        let url = url::Url::parse(app_url.trim()).ok()?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return None;
        }

        let origin = url.origin().ascii_serialization();
        let prefix = url.path().trim_end_matches('/');
        let base = format!("{origin}{prefix}");
        let issuer = format!("{base}{ISSUER_PATH}");

        Some(Urls {
            resource: format!("{base}{MCP_PATH}"),
            resource_metadata: format!(
                "{origin}/.well-known/oauth-protected-resource{prefix}{MCP_PATH}"
            ),
            authorization_endpoint: format!("{base}{AUTHORIZE_PAGE}"),
            token_endpoint: format!("{issuer}/token"),
            registration_endpoint: format!("{issuer}/register"),
            revocation_endpoint: format!("{issuer}/revoke"),
            issuer,
        })
    }

    pub async fn load(state: &State) -> Result<Urls, anyhow::Error> {
        let settings = state.settings.get().await?;

        Urls::new(&settings.app.url)
            .ok_or_else(|| anyhow::anyhow!("the panel app URL is not a valid URL"))
    }
}

/// A random token with a recognisable prefix: 43 alphanumeric characters, about 256 bits.
pub fn generate(prefix: &str) -> String {
    format!(
        "{prefix}{}",
        rand::distr::Alphanumeric.sample_string(&mut rand::rng(), 43)
    )
}

/// How codes, tokens and secrets are stored: the panel's own digest for random tokens.
pub fn digest(token: &str) -> String {
    shared::crypt::token_digest(token)
}

pub fn global_router(state: &State) -> OpenApiRouter<State> {
    routes::global_router(state)
}

pub fn auth_router(state: &State) -> OpenApiRouter<State> {
    routes::auth_router(state)
}

pub fn client_router(state: &State) -> OpenApiRouter<State> {
    routes::client_router(state)
}

/// Removes expired codes and tokens, grants nobody can refresh any more (with their API keys)
/// and registrations that never led to a grant.
pub async fn prune(state: &State) -> Result<(), anyhow::Error> {
    pg::prune(state).await
}
