use super::lifecycle::supported_versions;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const HEADER_MISMATCH: i64 = -32020;
pub const UNSUPPORTED_VERSION: i64 = -32022;

#[derive(Debug)]
pub struct Rejection {
    pub status: StatusCode,
    pub id: Option<Value>,
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl Rejection {
    pub fn new(
        status: StatusCode,
        id: Option<Value>,
        code: i64,
        message: impl Into<String>,
    ) -> Box<Self> {
        Box::new(Self {
            status,
            id,
            code,
            message: message.into(),
            data: None,
        })
    }

    pub fn unsupported(id: Option<Value>, requested: &str) -> Box<Self> {
        let mut rejection = Self::new(
            StatusCode::BAD_REQUEST,
            id,
            UNSUPPORTED_VERSION,
            "Unsupported protocol version",
        );
        rejection.data = Some(json!({
            "supported": supported_versions(),
            "requested": requested,
        }));
        rejection
    }

    pub fn mismatch(id: &Option<Value>, message: impl Into<String>) -> Box<Self> {
        Self::new(
            StatusCode::BAD_REQUEST,
            id.clone(),
            HEADER_MISMATCH,
            message,
        )
    }
}

impl IntoResponse for Box<Rejection> {
    fn into_response(self) -> Response {
        let mut error = json!({ "code": self.code, "message": self.message });
        if let Some(data) = self.data {
            error["data"] = data;
        }

        let mut body = json!({ "jsonrpc": "2.0" });
        if let Some(id) = self.id {
            body["id"] = id;
        }
        body["error"] = error;

        (self.status, Json(body)).into_response()
    }
}
