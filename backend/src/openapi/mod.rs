mod index;
mod schema;
mod search;

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Method, Request},
};
use serde_json::{Map, Value};
use shared::State;
use std::{
    collections::HashSet,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::sync::OnceCell;

pub use search::Scope;

const MAX_SPEC: usize = 64 * 1024 * 1024;

static INDEX: OnceCell<Arc<Index>> = OnceCell::const_new();

#[derive(Debug)]
pub struct Param {
    pub name: String,
    pub location: String,
    pub required: bool,
    pub schema: Value,
    pub description: String,
}

#[derive(Debug)]
pub struct Operation {
    pub method: String,
    pub path: String,
    pub id: String,
    pub params: Vec<Param>,
    pub body_type: Option<String>,
    pub body: Option<Value>,
    pub body_fields: Vec<String>,
    pub response: Option<Value>,
    path_words: HashSet<String>,
    field_words: HashSet<String>,
}

pub struct Index {
    pub operations: Vec<Operation>,
    schemas: Map<String, Value>,
}

pub async fn load(state: &State) -> Result<Arc<Index>, String> {
    INDEX
        .get_or_try_init(|| async {
            let mut request = Request::builder()
                .method(Method::GET)
                .uri("/openapi.json")
                .body(Body::empty())
                .map_err(|err| err.to_string())?;
            request
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))));

            let response = state
                .send_router_oneshot(request)
                .await
                .map_err(|err| format!("Could not read the panel API description: {err}"))?;
            if !response.status().is_success() {
                return Err(format!(
                    "Could not read the panel API description: HTTP {}",
                    response.status()
                ));
            }

            let bytes = axum::body::to_bytes(response.into_body(), MAX_SPEC)
                .await
                .map_err(|err| err.to_string())?;
            let spec: Value = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;

            Ok(Arc::new(Index::new(&spec)))
        })
        .await
        .cloned()
}
