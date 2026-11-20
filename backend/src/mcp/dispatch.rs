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
        // The prompt list depends on the connection's access, so it is private.
        "prompts/list" => single_page(params)
            .map(|()| {
                cacheable(
                    era,
                    prompts::definitions(ctx.access),
                    LIST_TTL_MS,
                    "private",
                )
            })
            .map_err(reject),
        "prompts/get" => prompts::get(ctx, params, ctx.access)
            .await
            .map_err(reject),
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
    use axum::http::Method;

    const READ_ONLY: Access = Access {
        read_only: true,
        client_only: false,
    };

    fn request(method: &str, params: Value) -> Prepared {
        Prepared {
            id: Some(json!(1)),
            method: method.into(),
            params: match params {
                Value::Object(params) => params,
                _ => Map::new(),
            },
            era: Era::Legacy,
        }
    }

    async fn send(
        panel: MockPanel,
        access: Access,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<Rejection>> {
        let (ctx, _) = panel.ctx(access);
        dispatch(
            &ctx,
            &request(method, params),
            &ExtensionSettingsData::default(),
        )
        .await
    }

    async fn run(access: Access, method: &str) -> Value {
        send(MockPanel::default(), access, method, json!({}))
            .await
            .unwrap()
    }

    fn names(list: &Value, key: &str) -> Vec<String> {
        list[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn read_only_hides_and_refuses_prompts_that_change_things() {
        let all = names(&run(Access::NONE, "prompts/list").await, "prompts");
        let limited = names(&run(READ_ONLY, "prompts/list").await, "prompts");
        assert!(all.contains(&"backup_before_update".to_string()));
        assert!(!limited.contains(&"backup_before_update".to_string()));
        assert!(!limited.contains(&"setup_minecraft_server".to_string()));
        assert!(limited.contains(&"diagnose_crash".to_string()));

        let refused = send(
            MockPanel::default(),
            READ_ONLY,
            "prompts/get",
            json!({ "name": "backup_before_update", "arguments": { "server": "0a1b2c3d" } }),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.code, INVALID_PARAMS);
        assert!(refused.message.contains("read-only"), "{}", refused.message);
    }

    #[tokio::test]
    async fn resources_are_read_through_the_access_checked_proxy() {
        let panel = MockPanel::default().reply(
            "/api/client/servers/0a1b2c3d/logs?lines=2",
            200,
            "text/plain",
            b"one\ntwo",
        );
        let (ctx, panel) = panel.ctx(Access {
            read_only: true,
            client_only: true,
        });
        let result = dispatch(
            &ctx,
            &request(
                "resources/read",
                json!({ "uri": "calagopus://servers/0a1b2c3d/console?lines=2" }),
            ),
            &ExtensionSettingsData::default(),
        )
        .await
        .unwrap();
        assert_eq!(result["contents"][0]["text"], json!("one\ntwo"));
        // The read went through Upstream, behind the proxy's access check.
        assert_eq!(panel.calls().len(), 1);
    }

    #[tokio::test]
    async fn json_tools_return_structured_content_next_to_the_text() {
        let servers = json!({
            "servers": { "total": 1, "page": 1, "per_page": 25, "data": [{ "name": "Survival" }] }
        });
        let panel = MockPanel::default().on_json(Method::GET, "/api/client/servers", servers.clone());
        let result = send(
            panel,
            Access::NONE,
            "tools/call",
            json!({ "name": "list_servers", "arguments": {} }),
        )
        .await
        .unwrap();

        assert_eq!(result["isError"], json!(false));
        assert_eq!(result["structuredContent"], servers);
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, servers);

        let listed = run(Access::NONE, "tools/list").await;
        let list_servers = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == json!("list_servers"))
            .unwrap();
        assert_eq!(list_servers["outputSchema"]["type"], json!("object"));
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
