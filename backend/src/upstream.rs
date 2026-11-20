use crate::openapi::{self, Index};
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{HeaderName, HeaderValue, Request, Response},
};
use shared::{
    State,
    models::user::{AuthMethod, User},
};
use std::{net::SocketAddr, sync::Arc};

/// Where every panel request of an MCP call goes. Production code sends them
/// through the panel's own router as the connected user; tests use
/// `panel::mock::MockPanel`. Requests reach this only through
/// `proxy::call`, which enforces the connection's access first.
#[async_trait::async_trait]
pub trait Upstream: Send + Sync {
    async fn send(&self, request: Request<Body>) -> Result<Response<Body>, String>;

    /// The searchable index of the panel's OpenAPI description.
    async fn openapi(&self) -> Result<Arc<Index>, String>;
}

pub struct LiveUpstream {
    pub state: State,
    pub user: User,
    pub auth: AuthMethod,
    pub addr: SocketAddr,
    /// Headers copied from the MCP request so the panel's activity log and
    /// rate limits see the real client.
    pub forward: Vec<(HeaderName, HeaderValue)>,
}

#[async_trait::async_trait]
impl Upstream for LiveUpstream {
    async fn send(&self, mut request: Request<Body>) -> Result<Response<Body>, String> {
        for (name, value) in &self.forward {
            request.headers_mut().insert(name.clone(), value.clone());
        }
        request.extensions_mut().insert(ConnectInfo(self.addr));

        self.state
            .send_authenticated_router_oneshot(request, self.user.clone(), self.auth.clone())
            .await
            .map_err(|err| err.to_string())
    }

    async fn openapi(&self) -> Result<Arc<Index>, String> {
        openapi::load(&self.state).await
    }
}
