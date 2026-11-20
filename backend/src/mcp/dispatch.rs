use super::{
    error::{INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND, RESOURCE_NOT_FOUND, Rejection},
    lifecycle,
    protocol::{Era, Prepared},
};
use crate::{
    completion,
    panel::Failure,
    prompts, resources,
    settings::ExtensionSettingsData,
    tools::{self, Ctx},
};
use axum::http::StatusCode;
use serde_json::{Map, Value, json};

/// How long clients may cache lists that only change with an extension update.
const LIST_TTL_MS: u64 = 5 * 60 * 1000;
/// The servers a key can access change more often.
const RESOURCE_LIST_TTL_MS: u64 = 60 * 1000;

pub async fn dispatch(
    ctx: &Ctx,
    prepared: &Prepared,
    settings: &ExtensionSettingsData,
) -> Result<Value, Box<Rejection>> {
    let era = &prepared.era;
    let params = &prepared.params;
    let reject = |failure| reject(prepared, failure);

    match prepared.method.as_str() {
        "initialize" => Ok(lifecycle::initialize(params, settings, ctx.access)),
        "server/discover" => Ok(cacheable(
            era,
            lifecycle::discover(settings, ctx.access),
            LIST_TTL_MS,
            "private",
        )),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(cacheable(
            era,
            json!({ "tools": tools::list(ctx.access) }),
            LIST_TTL_MS,
            "private",
        )),
        "tools/call" => tools::call(ctx, params).await.map_err(|message| {
            Rejection::new(StatusCode::OK, prepared.id.clone(), INVALID_PARAMS, message)
        }),
        "prompts/list" => single_page(params)
            .map(|()| cacheable(era, prompts::definitions(), LIST_TTL_MS, "public"))
            .map_err(reject),
        "prompts/get" => prompts::get(ctx, params).await.map_err(reject),
        "resources/list" => resources::list(ctx, params)
            .await
            .map(|result| cacheable(era, result, RESOURCE_LIST_TTL_MS, "private"))
            .map_err(reject),
        "resources/templates/list" => single_page(params)
            .map(|()| {
                cacheable(
                    era,
                    json!({ "resourceTemplates": resources::templates() }),
                    LIST_TTL_MS,
                    "public",
                )
            })
            .map_err(reject),
        "resources/read" => resources::read(ctx, params)
            .await
            .map(|result| cacheable(era, result, 0, "private"))
            .map_err(reject),
        "completion/complete" => completion::complete(ctx, params).await.map_err(reject),
        method => Err(Rejection::new(
            match era {
                Era::Modern => StatusCode::NOT_FOUND,
                Era::Legacy => StatusCode::OK,
            },
            prepared.id.clone(),
            METHOD_NOT_FOUND,
            format!("Method not found: {method}"),
        )),
    }
}

/// Lists that fit in one page never hand out a cursor, so any cursor is invalid.
fn single_page(params: &Map<String, Value>) -> Result<(), Failure> {
    match params.get("cursor") {
        None | Some(Value::Null) => Ok(()),
        Some(_) => Err(Failure::Invalid("Invalid cursor".into())),
    }
}

/// Adds the caching hints the modern protocol requires on list and read results.
fn cacheable(era: &Era, mut result: Value, ttl_ms: u64, scope: &str) -> Value {
    if *era == Era::Modern {
        result["ttlMs"] = json!(ttl_ms);
        result["cacheScope"] = json!(scope);
    }
    result
}

/// Maps a failure to the error code of the request's protocol version: a
/// missing resource is -32002 up to 2025-11-25 and -32602 since 2026-07-28.
fn reject(prepared: &Prepared, failure: Failure) -> Box<Rejection> {
    let code = match (&failure, &prepared.era) {
        (Failure::NotFound { .. }, Era::Legacy) => RESOURCE_NOT_FOUND,
        (Failure::Internal(_), _) => INTERNAL_ERROR,
        _ => INVALID_PARAMS,
    };
    let mut rejection =
        Rejection::new(StatusCode::OK, prepared.id.clone(), code, failure.message());
    rejection.data = failure.data();
    rejection
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(era: Era) -> Prepared {
        Prepared {
            id: Some(json!(7)),
            method: "resources/read".into(),
            params: Map::new(),
            era,
        }
    }

    #[test]
    fn missing_resources_use_the_code_of_each_era() {
        let missing = || Failure::NotFound {
            uri: "calagopus://servers/0a1b2c3d/files/x".into(),
            message: "Resource not found".into(),
        };

        let modern = reject(&prepared(Era::Modern), missing());
        assert_eq!(modern.code, INVALID_PARAMS);
        assert_eq!(modern.status, StatusCode::OK);
        assert_eq!(modern.id, Some(json!(7)));
        assert_eq!(
            modern.data,
            Some(json!({ "uri": "calagopus://servers/0a1b2c3d/files/x" }))
        );

        let legacy = reject(&prepared(Era::Legacy), missing());
        assert_eq!(legacy.code, RESOURCE_NOT_FOUND);

        assert_eq!(
            reject(&prepared(Era::Legacy), Failure::Internal("down".into())).code,
            INTERNAL_ERROR
        );
        let invalid = reject(&prepared(Era::Modern), Failure::Invalid("bad".into()));
        assert_eq!((invalid.code, invalid.data), (INVALID_PARAMS, None));
    }

    #[test]
    fn only_modern_results_carry_caching_hints() {
        let modern = cacheable(&Era::Modern, json!({ "prompts": [] }), 1000, "public");
        assert_eq!(
            modern,
            json!({ "prompts": [], "ttlMs": 1000, "cacheScope": "public" })
        );
        let legacy = cacheable(&Era::Legacy, json!({ "prompts": [] }), 1000, "public");
        assert_eq!(legacy, json!({ "prompts": [] }));
    }

    #[test]
    fn single_page_lists_refuse_cursors() {
        assert!(single_page(&Map::new()).is_ok());
        let mut params = Map::new();
        params.insert("cursor".into(), json!(""));
        assert!(single_page(&params).is_err());
    }
}

#[cfg(test)]
mod access_tests {
    use super::*;
    use crate::{access::Access, panel::mock::MockPanel};

    fn request(method: &str) -> Prepared {
        Prepared {
            id: Some(json!(1)),
            method: method.into(),
            params: Map::new(),
            era: Era::Legacy,
        }
    }

    async fn run(access: Access, method: &str) -> Value {
        let (ctx, _) = MockPanel::default().ctx(access);
        dispatch(&ctx, &request(method), &ExtensionSettingsData::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn tools_list_follows_the_connection_access() {
        let read_only = Access {
            read_only: true,
            client_only: false,
        };
        let all = run(Access::NONE, "tools/list").await;
        let limited = run(read_only, "tools/list").await;
        let has = |result: &Value, name: &str| {
            result["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == json!(name))
        };

        assert!(has(&all, "write_api"));
        assert!(!has(&limited, "write_api"));
        assert!(has(&limited, "read_api"));
    }

    #[tokio::test]
    async fn instructions_mention_restrictions() {
        let open = run(Access::NONE, "initialize").await;
        assert!(!open["instructions"].as_str().unwrap().contains("read-only"));

        let limited = run(
            Access {
                read_only: true,
                client_only: true,
            },
            "initialize",
        )
        .await;
        let text = limited["instructions"].as_str().unwrap();
        assert!(text.contains("read-only and client-only"), "{text}");
    }
}
