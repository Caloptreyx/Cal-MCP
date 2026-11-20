//! Read access to the panel API for prompts, resources and completions. Every
//! request runs through the panel's own router with the caller's credentials,
//! so the panel's permission checks decide what is visible.

use crate::{
    proxy::{self, Reply},
    tools::Ctx,
};
use axum::http::Method;
use serde_json::{Map, Value, json};

pub trait Panel: Sync {
    fn get(
        &self,
        path: &str,
        query: Option<&Map<String, Value>>,
    ) -> impl Future<Output = Result<Reply, String>> + Send;
}

impl Panel for Ctx {
    fn get(
        &self,
        path: &str,
        query: Option<&Map<String, Value>>,
    ) -> impl Future<Output = Result<Reply, String>> + Send {
        proxy::call(self, Method::GET, path, query, None)
    }
}

/// A `json!({...})` literal as a query map.
pub fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Why a prompt, resource or completion request failed. The MCP layer maps
/// each kind to the JSON-RPC error code of the negotiated protocol version.
#[derive(Debug, PartialEq)]
pub enum Failure {
    /// The resource does not exist or is hidden from the caller.
    NotFound { uri: String, message: String },
    /// The request itself is wrong: bad arguments, a bad URI, a missing permission.
    Invalid(String),
    /// The panel or the node failed.
    Internal(String),
}

impl Failure {
    pub fn message(&self) -> &str {
        match self {
            Self::NotFound { message, .. } | Self::Invalid(message) | Self::Internal(message) => {
                message
            }
        }
    }

    pub fn data(&self) -> Option<Value> {
        match self {
            Self::NotFound { uri, .. } => Some(json!({ "uri": uri })),
            _ => None,
        }
    }
}

/// The panel's error text: the `errors` list of an `ApiError` body, or the
/// raw body when it is something else.
pub fn error_text(reply: &Reply) -> String {
    let body = String::from_utf8_lossy(&reply.body);
    let errors = serde_json::from_slice::<Value>(&reply.body)
        .ok()
        .and_then(|value| value.get("errors").and_then(Value::as_array).cloned())
        .map(|errors| {
            errors
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ")
        })
        .filter(|errors| !errors.is_empty());

    match errors {
        Some(errors) => errors,
        None if body.trim().is_empty() => format!("HTTP {}", reply.status),
        None => body.chars().take(500).collect(),
    }
}

/// Turns an unsuccessful panel reply into a [`Failure`]. `uri` names the
/// resource that was asked for, when there is one.
pub fn failure(reply: &Reply, uri: Option<&str>) -> Failure {
    let text = error_text(reply);
    match (reply.status, uri) {
        (404, Some(uri)) => Failure::NotFound {
            uri: uri.to_string(),
            message: format!("Resource not found: {text}"),
        },
        (404, None) => Failure::Invalid(format!("Not found: {text}")),
        (401 | 403, _) => {
            Failure::Invalid(format!("The API key is not allowed to do this: {text}"))
        }
        (status, _) if status < 500 => Failure::Invalid(format!("HTTP {status}: {text}")),
        (status, _) => Failure::Internal(format!("The panel answered HTTP {status}: {text}")),
    }
}

/// GETs a JSON endpoint and fails on any non-2xx status.
pub async fn get_json(
    panel: &impl Panel,
    path: &str,
    query: Option<&Map<String, Value>>,
    uri: Option<&str>,
) -> Result<Value, Failure> {
    let reply = panel.get(path, query).await.map_err(Failure::Internal)?;
    if !(200..300).contains(&reply.status) {
        return Err(failure(&reply, uri));
    }
    serde_json::from_slice(&reply.body)
        .map_err(|err| Failure::Internal(format!("The panel sent invalid JSON for {path}: {err}")))
}

/// Server identifiers the panel routes accept: a UUID or an 8-digit short ID.
pub fn is_server_id(value: &str) -> bool {
    let hex =
        |part: &str, len: usize| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit());
    if value.len() == 8 {
        return hex(value, 8);
    }
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(len, part)| hex(part, *len))
}

/// A canned panel for tests: answers `path?key=value&...` (query keys in
/// insertion order) and records every request. [`MockPanel::on`] adds
/// handlers for any method, and [`MockPanel::ctx`] hands out a tool context
/// whose requests all land here.
#[cfg(test)]
pub mod mock {
    use super::*;
    use crate::{access::Access, openapi::Index, tools::Ctx, upstream::Upstream};
    use axum::{
        body::Body,
        http::{Request, Response, header},
    };
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    type Handler = Box<dyn Fn(&Call) -> Reply + Send + Sync>;

    /// One request a tool sent through [`Upstream`].
    #[derive(Clone, Debug)]
    pub struct Call {
        pub method: Method,
        pub path: String,
        pub query: Map<String, Value>,
        pub body: Vec<u8>,
    }

    impl Call {
        pub fn query(&self, key: &str) -> Option<&str> {
            self.query.get(key).and_then(Value::as_str)
        }

        pub fn json(&self) -> Value {
            serde_json::from_slice(&self.body).unwrap_or(Value::Null)
        }

        pub fn text(&self) -> String {
            String::from_utf8_lossy(&self.body).into_owned()
        }
    }

    pub fn json_reply(status: u16, body: Value) -> Reply {
        Reply {
            status,
            content_type: Some("application/json".into()),
            body: body.to_string().into_bytes(),
        }
    }

    pub fn text_reply(text: &str) -> Reply {
        Reply {
            status: 200,
            content_type: Some("application/octet-stream".into()),
            body: text.as_bytes().to_vec(),
        }
    }

    #[derive(Default)]
    pub struct MockPanel {
        replies: HashMap<String, Reply>,
        handlers: Vec<(Method, String, Handler)>,
        index: Option<Arc<Index>>,
        pub requests: Mutex<Vec<String>>,
        calls: Mutex<Vec<Call>>,
    }

    pub fn key(path: &str, query: Option<&Map<String, Value>>) -> String {
        let pairs: Vec<String> = query
            .into_iter()
            .flatten()
            .map(|(key, value)| match value {
                Value::String(text) => format!("{key}={text}"),
                value => format!("{key}={value}"),
            })
            .collect();
        if pairs.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{}", pairs.join("&"))
        }
    }

    impl MockPanel {
        pub fn reply(mut self, key: &str, status: u16, content_type: &str, body: &[u8]) -> Self {
            self.replies.insert(
                key.to_string(),
                Reply {
                    status,
                    content_type: Some(content_type.to_string()),
                    body: body.to_vec(),
                },
            );
            self
        }

        pub fn json(self, key: &str, body: Value) -> Self {
            self.reply(key, 200, "application/json", body.to_string().as_bytes())
        }

        pub fn error(self, key: &str, status: u16, message: &str) -> Self {
            self.reply(
                key,
                status,
                "application/json",
                json!({ "errors": [message] }).to_string().as_bytes(),
            )
        }

        /// Answers `method path`, whatever the query, with `handler`. Wins over
        /// canned replies.
        pub fn on(
            mut self,
            method: Method,
            path: &str,
            handler: impl Fn(&Call) -> Reply + Send + Sync + 'static,
        ) -> Self {
            self.handlers
                .push((method, path.to_string(), Box::new(handler)));
            self
        }

        /// Answers `method path` with `200` and a fixed JSON body.
        pub fn on_json(self, method: Method, path: &str, body: Value) -> Self {
            self.on(method, path, move |_| json_reply(200, body.clone()))
        }

        /// Uses `spec` as the panel's OpenAPI description.
        pub fn spec(mut self, spec: &Value) -> Self {
            self.index = Some(Arc::new(Index::new(spec)));
            self
        }

        /// A tool context with `access` whose panel requests all land here.
        pub fn ctx(self, access: Access) -> (Ctx, Arc<MockPanel>) {
            let panel = Arc::new(self);
            (
                Ctx {
                    upstream: panel.clone(),
                    access,
                },
                panel,
            )
        }

        pub fn requested(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }

        /// Every request that came through [`Upstream`].
        pub fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn canned(&self, key: &str) -> Reply {
            self.requests.lock().unwrap().push(key.to_string());
            match self.replies.get(key) {
                Some(reply) => Reply {
                    status: reply.status,
                    content_type: reply.content_type.clone(),
                    body: reply.body.clone(),
                },
                None => json_reply(
                    404,
                    json!({ "errors": [format!("mock has no reply for {key}")] }),
                ),
            }
        }
    }

    impl Panel for MockPanel {
        fn get(
            &self,
            path: &str,
            query: Option<&Map<String, Value>>,
        ) -> impl Future<Output = Result<Reply, String>> + Send {
            std::future::ready(Ok(self.canned(&key(path, query))))
        }
    }

    #[async_trait::async_trait]
    impl Upstream for MockPanel {
        async fn send(&self, request: Request<Body>) -> Result<Response<Body>, String> {
            let (parts, body) = request.into_parts();
            let body = axum::body::to_bytes(body, usize::MAX)
                .await
                .map_err(|err| err.to_string())?
                .to_vec();
            let query: Map<String, Value> =
                url::form_urlencoded::parse(parts.uri.query().unwrap_or_default().as_bytes())
                    .map(|(key, value)| (key.into_owned(), Value::String(value.into_owned())))
                    .collect();
            let call = Call {
                method: parts.method,
                path: parts.uri.path().to_string(),
                query,
                body,
            };

            let handler = self
                .handlers
                .iter()
                .find(|(method, path, _)| *method == call.method && *path == call.path);
            let reply = match handler {
                Some((_, _, handler)) => handler(&call),
                None if call.method == Method::GET => {
                    self.canned(&key(&call.path, Some(&call.query)))
                }
                None => json_reply(
                    404,
                    json!({ "errors": [format!("mock has no handler for {} {}", call.method, call.path)] }),
                ),
            };
            self.calls.lock().unwrap().push(call);

            let mut response = Response::builder().status(reply.status);
            if let Some(content_type) = &reply.content_type {
                response = response.header(header::CONTENT_TYPE, content_type);
            }
            response
                .body(Body::from(reply.body))
                .map_err(|err| err.to_string())
        }

        async fn openapi(&self) -> Result<Arc<Index>, String> {
            Ok(self
                .index
                .clone()
                .unwrap_or_else(|| Arc::new(Index::new(&json!({ "paths": {} })))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(status: u16, body: &str) -> Reply {
        Reply {
            status,
            content_type: Some("application/json".into()),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn server_ids_are_short_ids_or_uuids() {
        assert!(is_server_id("0a1b2c3d"));
        assert!(is_server_id("123e4567-e89b-12d3-a456-426614174000"));
        assert!(!is_server_id("Survival"));
        assert!(!is_server_id("0a1b2c3"));
        assert!(!is_server_id("123e4567e89b12d3a456426614174000"));
    }

    #[test]
    fn failures_follow_the_status() {
        let not_found = failure(
            &reply(404, r#"{"errors":["file not found"]}"#),
            Some("x://a"),
        );
        assert_eq!(
            not_found,
            Failure::NotFound {
                uri: "x://a".into(),
                message: "Resource not found: file not found".into()
            }
        );
        assert_eq!(not_found.data(), Some(json!({ "uri": "x://a" })));
        assert!(matches!(
            failure(&reply(403, r#"{"errors":["permission denied"]}"#), Some("x://a")),
            Failure::Invalid(message) if message.contains("permission denied")
        ));
        assert!(
            matches!(failure(&reply(502, ""), None), Failure::Internal(message) if message.contains("HTTP 502"))
        );
    }
}
