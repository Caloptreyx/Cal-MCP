use super::{definitions, list};
use crate::{
    access::Access,
    panel::mock::{MockPanel, json_reply, text_reply},
};
use axum::http::Method;
use serde_json::{Map, Value, json};

const READ_ONLY: Access = Access {
    read_only: true,
    client_only: false,
};
const CLIENT_ONLY: Access = Access {
    read_only: false,
    client_only: true,
};

/// Calls a tool and returns its whole `tools/call` result.
async fn call_tool(ctx: &super::Ctx, name: &str, arguments: Value) -> Result<Value, String> {
    let mut params = Map::new();
    params.insert("name".into(), json!(name));
    params.insert("arguments".into(), arguments);
    super::call(ctx, &params).await
}

/// The text of a tool result that must have succeeded.
fn ok_text(result: &Value) -> String {
    assert_eq!(result["isError"], json!(false), "tool failed: {result}");
    result["content"][0]["text"].as_str().unwrap().to_string()
}

/// The text of a tool result that must have failed.
fn err_text(result: &Value) -> String {
    assert_eq!(result["isError"], json!(true), "tool succeeded: {result}");
    result["content"][0]["text"].as_str().unwrap().to_string()
}

fn names(tools: &[Value]) -> Vec<&str> {
    tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect()
}

#[test]
fn unrestricted_and_client_only_list_every_tool() {
    assert_eq!(list(Access::NONE).len(), definitions().len());
    assert_eq!(list(CLIENT_ONLY).len(), definitions().len());
}

#[test]
fn read_only_lists_only_read_only_tools() {
    let listed = list(READ_ONLY);
    assert!(!listed.is_empty());
    for tool in &listed {
        assert_eq!(
            tool["annotations"]["readOnlyHint"],
            json!(true),
            "{} is listed",
            tool["name"]
        );
    }
    let listed = names(&listed);
    for hidden in ["write_api", "power_server", "send_command", "write_file"] {
        assert!(!listed.contains(&hidden), "{hidden} is listed");
    }
    for shown in ["read_api", "list_servers", "read_file"] {
        assert!(listed.contains(&shown), "{shown} is hidden");
    }
}

#[tokio::test]
async fn read_only_refuses_write_tools_before_reaching_the_panel() {
    let (ctx, panel) = MockPanel::default().ctx(READ_ONLY);

    for (name, args) in [
        (
            "write_api",
            json!({ "method": "POST", "path": "/api/client/servers/abc/power", "body": { "action": "kill" } }),
        ),
        ("power_server", json!({ "server": "abc", "action": "kill" })),
        ("send_command", json!({ "server": "abc", "command": "stop" })),
        (
            "write_file",
            json!({ "server": "abc", "file": "/a.txt", "content": "x" }),
        ),
    ] {
        let err = call_tool(&ctx, name, args).await.unwrap_err();
        assert!(err.contains("read-only"), "{name}: {err}");
    }
    assert!(panel.calls().is_empty());
}

#[tokio::test]
async fn client_only_blocks_admin_paths_in_the_proxy() {
    let (ctx, panel) = MockPanel::default()
        .on_json(Method::GET, "/api/admin/users", json!({ "users": [] }))
        .ctx(CLIENT_ONLY);

    let result = call_tool(&ctx, "read_api", json!({ "path": "/api/admin/users" }))
        .await
        .unwrap();
    assert!(err_text(&result).contains("client-only"));
    assert!(panel.calls().is_empty());
}

#[tokio::test]
async fn read_only_still_allows_get_requests() {
    let (ctx, panel) = MockPanel::default()
        .on_json(Method::GET, "/api/admin/users", json!({ "users": [] }))
        .ctx(READ_ONLY);

    let result = call_tool(&ctx, "read_api", json!({ "path": "/api/admin/users" }))
        .await
        .unwrap();
    assert!(ok_text(&result).contains("users"));
    assert_eq!(panel.calls().len(), 1);
}

#[tokio::test]
async fn client_only_limits_api_search_to_client_endpoints() {
    let spec = json!({
        "paths": {
            "/api/client/servers/{server}/backups": { "get": { "operationId": "list_backups" } },
            "/api/admin/servers/{server}/backups": { "get": { "operationId": "admin_list_backups" } },
        }
    });
    let (ctx, _) = MockPanel::default().spec(&spec).ctx(CLIENT_ONLY);

    let found = ok_text(
        &call_tool(&ctx, "search_api", json!({ "query": "backups" }))
            .await
            .unwrap(),
    );
    assert!(found.contains("/api/client/servers/{server}/backups"), "{found}");
    assert!(!found.contains("/api/admin/"), "{found}");

    let refused = call_tool(
        &ctx,
        "search_api",
        json!({ "query": "backups", "scope": "admin" }),
    )
    .await
    .unwrap();
    assert!(err_text(&refused).contains("client-only"));

    let (ctx, _) = MockPanel::default().spec(&spec).ctx(Access::NONE);
    let found = ok_text(
        &call_tool(&ctx, "search_api", json!({ "query": "backups" }))
            .await
            .unwrap(),
    );
    assert!(found.contains("/api/admin/servers/{server}/backups"), "{found}");
}

#[tokio::test]
async fn unknown_tools_are_protocol_errors() {
    let (ctx, _) = MockPanel::default().ctx(Access::NONE);
    let err = call_tool(&ctx, "no_such_tool", json!({})).await.unwrap_err();
    assert!(err.contains("Unknown tool"), "{err}");
}

#[tokio::test]
async fn write_api_sends_the_method_query_and_json_body() {
    let (ctx, panel) = MockPanel::default()
        .on(Method::PATCH, "/api/client/servers/abc/settings", |_| {
            json_reply(200, json!({}))
        })
        .ctx(Access::NONE);

    let result = call_tool(
        &ctx,
        "write_api",
        json!({
            "method": "patch",
            "path": "/api/client/servers/abc/settings",
            "query": { "dry_run": true },
            "body": { "name": "Lobby" },
        }),
    )
    .await
    .unwrap();
    ok_text(&result);

    let call = &panel.calls()[0];
    assert_eq!(call.method, Method::PATCH);
    assert_eq!(call.query("dry_run"), Some("true"));
    assert_eq!(call.json(), json!({ "name": "Lobby" }));
}

#[tokio::test]
async fn read_file_returns_the_file_text() {
    let (ctx, panel) = MockPanel::default()
        .on(Method::GET, "/api/client/servers/abc/files/contents", |_| {
            text_reply("motd=Hello\n")
        })
        .ctx(Access::NONE);

    let result = call_tool(
        &ctx,
        "read_file",
        json!({ "server": "abc", "file": "/server.properties" }),
    )
    .await
    .unwrap();
    assert_eq!(ok_text(&result), "motd=Hello\n");
    assert_eq!(panel.calls()[0].query("file"), Some("/server.properties"));
}

#[tokio::test]
async fn write_file_sends_the_contents_as_text() {
    let (ctx, panel) = MockPanel::default()
        .on(Method::POST, "/api/client/servers/abc/files/write", |_| {
            json_reply(200, json!({}))
        })
        .ctx(Access::NONE);

    let result = call_tool(
        &ctx,
        "write_file",
        json!({ "server": "abc", "file": "/a.txt", "content": "hello" }),
    )
    .await
    .unwrap();
    assert!(ok_text(&result).contains("5 bytes"));

    let writes: Vec<_> = panel
        .calls()
        .into_iter()
        .filter(|call| call.method == Method::POST)
        .collect();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].query("file"), Some("/a.txt"));
    assert_eq!(writes[0].text(), "hello");
}
