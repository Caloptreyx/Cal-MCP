use super::{
    error::{INVALID_PARAMS, METHOD_NOT_FOUND, Rejection},
    lifecycle,
    protocol::{Era, Prepared},
};
use crate::tools::{self, Ctx};
use axum::http::StatusCode;
use serde_json::{Value, json};

pub async fn dispatch(ctx: &Ctx, prepared: &Prepared) -> Result<Value, Box<Rejection>> {
    match prepared.method.as_str() {
        "initialize" => Ok(lifecycle::initialize(&prepared.params)),
        "server/discover" => Ok(lifecycle::discover()),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::definitions() })),
        "tools/call" => tools::call(ctx, &prepared.params).await.map_err(|message| {
            Rejection::new(StatusCode::OK, prepared.id.clone(), INVALID_PARAMS, message)
        }),
        method => Err(Rejection::new(
            match prepared.era {
                Era::Modern => StatusCode::NOT_FOUND,
                Era::Legacy => StatusCode::OK,
            },
            prepared.id.clone(),
            METHOD_NOT_FOUND,
            format!("Method not found: {method}"),
        )),
    }
}
