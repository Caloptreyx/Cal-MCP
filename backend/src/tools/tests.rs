use super::{definitions, list};
use crate::{
    access::Access,
    panel::mock::{Call, MockPanel, json_reply, text_reply},
};
use std::sync::atomic::{AtomicUsize, Ordering};
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

const SERVER: &str = "/api/client/servers/abc";

fn sent(panel: &MockPanel, method: Method) -> Vec<Call> {
    panel
        .calls()
        .into_iter()
        .filter(|call| call.method == method)
        .collect()
}

/// Answers with the next item of `replies` on every call, repeating the last.
fn sequence(replies: Vec<Value>) -> impl Fn(&Call) -> crate::proxy::Reply + Send + Sync {
    let count = AtomicUsize::new(0);
    move |_| {
        let index = count.fetch_add(1, Ordering::SeqCst).min(replies.len() - 1);
        json_reply(200, replies[index].clone())
    }
}

fn state(state: &str) -> Value {
    json!({ "resources": { "state": state } })
}

/// Console logs that change on every call: `logs[0]`, then `logs[1]`, ...
fn console(logs: Vec<&'static str>) -> impl Fn(&Call) -> crate::proxy::Reply + Send + Sync {
    let count = AtomicUsize::new(0);
    move |_| text_reply(logs[count.fetch_add(1, Ordering::SeqCst).min(logs.len() - 1)])
}

async fn run(panel: MockPanel, name: &str, args: Value) -> (Value, std::sync::Arc<MockPanel>) {
    let (ctx, panel) = panel.ctx(Access::NONE);
    let result = call_tool(&ctx, name, args).await.unwrap();
    (result, panel)
}

// API tools

#[tokio::test]
async fn describe_api_resolves_a_concrete_path() {
    let spec = json!({
        "paths": {
            "/api/client/servers/{server}/backups": { "get": {
                "operationId": "list_backups",
                "parameters": [{ "name": "page", "in": "query", "schema": { "type": "integer" } }],
            } },
        }
    });
    let (result, _) = run(
        MockPanel::default().spec(&spec),
        "describe_api",
        json!({ "method": "get", "path": "/api/client/servers/abc/backups" }),
    )
    .await;
    let text = ok_text(&result);
    assert!(text.contains("\"operationId\": \"list_backups\""), "{text}");
    assert!(text.contains("\"page\""), "{text}");
}

#[tokio::test]
async fn read_api_returns_structured_status_and_data() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::GET, "/api/client/account", json!({ "user": { "username": "steve" } })),
        "read_api",
        json!({ "path": "/api/client/account", "query": { "page": 2 } }),
    )
    .await;
    assert!(ok_text(&result).contains("steve"));
    assert_eq!(
        result["structuredContent"],
        json!({ "status": 200, "data": { "user": { "username": "steve" } }, "truncated": false })
    );
    assert_eq!(panel.calls()[0].query("page"), Some("2"));
}

#[tokio::test]
async fn read_api_reports_panel_errors() {
    let (result, _) = run(
        MockPanel::default().error("/api/client/account", 403, "permission denied"),
        "read_api",
        json!({ "path": "/api/client/account" }),
    )
    .await;
    assert!(err_text(&result).contains("HTTP 403"));
    assert!(result.get("structuredContent").is_none());
}

// Servers

#[tokio::test]
async fn list_servers_passes_paging_and_search() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::GET, "/api/client/servers", json!({ "servers": { "data": [] } })),
        "list_servers",
        json!({ "search": "lobby", "per_page": 10 }),
    )
    .await;
    assert_eq!(result["structuredContent"], json!({ "servers": { "data": [] } }));
    let call = &panel.calls()[0];
    assert_eq!(call.query("search"), Some("lobby"));
    assert_eq!(call.query("per_page"), Some("10"));
    assert_eq!(call.query("page"), Some("1"));
}

#[tokio::test]
async fn get_server_combines_details_and_live_state() {
    let (result, _) = run(
        MockPanel::default()
            .on_json(Method::GET, SERVER, json!({ "server": { "name": "Lobby" } }))
            .on_json(Method::GET, &format!("{SERVER}/resources"), state("running")),
        "get_server",
        json!({ "server": "abc" }),
    )
    .await;
    assert_eq!(
        result["structuredContent"],
        json!({ "server": { "name": "Lobby" }, "resources": { "state": "running" } })
    );
}

#[tokio::test]
async fn power_server_sends_the_action() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::POST, &format!("{SERVER}/power"), json!({})),
        "power_server",
        json!({ "server": "abc", "action": "start" }),
    )
    .await;
    assert_eq!(ok_text(&result), "Sent start to server abc.");
    assert_eq!(sent(&panel, Method::POST)[0].json(), json!({ "action": "start" }));
    assert!(sent(&panel, Method::GET).is_empty());
}

#[tokio::test(start_paused = true)]
async fn power_server_waits_for_a_restart_to_come_back() {
    let (result, _) = run(
        MockPanel::default()
            .on_json(Method::POST, &format!("{SERVER}/power"), json!({}))
            .on(
                Method::GET,
                &format!("{SERVER}/resources"),
                sequence(vec![
                    state("running"),
                    state("stopping"),
                    state("starting"),
                    state("running"),
                ]),
            ),
        "power_server",
        json!({ "server": "abc", "action": "restart", "wait": true }),
    )
    .await;
    assert!(ok_text(&result).contains("It is running after 4 s"), "{result}");
}

#[tokio::test(start_paused = true)]
async fn power_server_reports_a_crash_while_starting() {
    let (result, _) = run(
        MockPanel::default()
            .on_json(Method::POST, &format!("{SERVER}/power"), json!({}))
            .on(
                Method::GET,
                &format!("{SERVER}/resources"),
                sequence(vec![state("starting"), state("offline")]),
            ),
        "power_server",
        json!({ "server": "abc", "action": "start", "wait": true }),
    )
    .await;
    assert!(err_text(&result).contains("probably crashed"));
}

#[tokio::test(start_paused = true)]
async fn power_server_gives_up_after_the_timeout() {
    let (result, _) = run(
        MockPanel::default()
            .on_json(Method::POST, &format!("{SERVER}/power"), json!({}))
            .on_json(Method::GET, &format!("{SERVER}/resources"), state("running")),
        "power_server",
        json!({ "server": "abc", "action": "stop", "wait": true, "timeout_seconds": 3 }),
    )
    .await;
    assert!(err_text(&result).contains("still running after 3 s"));
}

#[tokio::test]
async fn send_command_explains_an_offline_server() {
    let (result, _) = run(
        MockPanel::default()
            .on(Method::POST, &format!("{SERVER}/command"), |_| {
                json_reply(409, json!({ "errors": ["server is not running"] }))
            })
            .on_json(Method::GET, &format!("{SERVER}/resources"), state("offline")),
        "send_command",
        json!({ "server": "abc", "command": "list" }),
    )
    .await;
    assert!(err_text(&result).contains("is offline"));
}

#[tokio::test]
async fn send_command_sends_the_command() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::POST, &format!("{SERVER}/command"), json!({})),
        "send_command",
        json!({ "server": "abc", "command": "say hi" }),
    )
    .await;
    ok_text(&result);
    assert_eq!(sent(&panel, Method::POST)[0].json(), json!({ "command": "say hi" }));
}

#[tokio::test(start_paused = true)]
async fn run_command_returns_only_new_output_once_it_matches() {
    let (result, panel) = run(
        MockPanel::default()
            .on_json(Method::POST, &format!("{SERVER}/command"), json!({}))
            .on(
                Method::GET,
                &format!("{SERVER}/logs"),
                console(vec![
                    "old line\nanother old line",
                    "old line\nanother old line\n> list",
                    "old line\nanother old line\n> list\nThere are 0 of 20 players online",
                ]),
            ),
        "run_command",
        json!({ "server": "abc", "command": "list", "wait_for": "players online" }),
    )
    .await;
    let text = ok_text(&result);
    assert!(text.contains("There are 0 of 20 players online"), "{text}");
    assert!(!text.contains("old line"), "{text}");
    assert_eq!(sent(&panel, Method::POST).len(), 1);
    assert_eq!(sent(&panel, Method::GET)[0].query("lines"), Some("1000"));
}

#[tokio::test(start_paused = true)]
async fn run_command_matches_regular_expressions() {
    let (result, _) = run(
        MockPanel::default()
            .on_json(Method::POST, &format!("{SERVER}/command"), json!({}))
            .on(
                Method::GET,
                &format!("{SERVER}/logs"),
                console(vec!["", "Done (12.5s)! For help, type \"help\""]),
            ),
        "run_command",
        json!({ "server": "abc", "command": "reload", "wait_for": "Done \\(\\d+", "regex": true }),
    )
    .await;
    assert!(ok_text(&result).contains("Done (12.5s)"));
}

#[tokio::test(start_paused = true)]
async fn run_command_without_wait_for_returns_once_the_console_is_quiet() {
    let (result, _) = run(
        MockPanel::default()
            .on_json(Method::POST, &format!("{SERVER}/command"), json!({}))
            .on(
                Method::GET,
                &format!("{SERVER}/logs"),
                console(vec!["a", "a\nb"]),
            ),
        "run_command",
        json!({ "server": "abc", "command": "version" }),
    )
    .await;
    let text = ok_text(&result);
    assert!(text.ends_with("\nb"), "{text}");
}

#[tokio::test(start_paused = true)]
async fn run_command_times_out_when_the_text_never_appears() {
    let (result, _) = run(
        MockPanel::default()
            .on_json(Method::POST, &format!("{SERVER}/command"), json!({}))
            .on(Method::GET, &format!("{SERVER}/logs"), console(vec!["a", "a\nb"])),
        "run_command",
        json!({ "server": "abc", "command": "save-all", "wait_for": "Saved", "timeout_seconds": 2 }),
    )
    .await;
    let text = err_text(&result);
    assert!(text.contains("did not appear within 2 s"), "{text}");
    assert!(text.ends_with("\nb"), "{text}");
}

#[tokio::test]
async fn run_command_refuses_a_bad_regex_before_sending() {
    let (result, panel) = run(
        MockPanel::default(),
        "run_command",
        json!({ "server": "abc", "command": "list", "wait_for": "(", "regex": true }),
    )
    .await;
    assert!(err_text(&result).contains("not a valid regular expression"));
    assert!(panel.calls().is_empty());
}

#[tokio::test]
async fn read_console_strips_colours_and_keeps_the_last_lines() {
    let (result, panel) = run(
        MockPanel::default().on(Method::GET, &format!("{SERVER}/logs"), |_| {
            text_reply("one\n\u{1b}[31mtwo\u{1b}[0m\nthree")
        }),
        "read_console",
        json!({ "server": "abc", "lines": 2 }),
    )
    .await;
    assert_eq!(ok_text(&result), "two\nthree");
    assert_eq!(panel.calls()[0].query("lines"), Some("2"));
}

// Files

const CONTENTS: &str = "/api/client/servers/abc/files/contents";
const WRITE: &str = "/api/client/servers/abc/files/write";

fn file(text: impl Into<String>) -> MockPanel {
    let text = text.into();
    MockPanel::default()
        .on(Method::GET, CONTENTS, move |_| text_reply(&text))
        .on_json(Method::POST, WRITE, json!({}))
}

#[tokio::test]
async fn list_files_returns_structured_entries() {
    let entries = json!({ "entries": { "total": 1, "page": 1, "per_page": 100, "data": [{ "name": "world" }] } });
    let (result, panel) = run(
        MockPanel::default().on_json(Method::GET, &format!("{SERVER}/files/list"), entries.clone()),
        "list_files",
        json!({ "server": "abc", "directory": "/plugins" }),
    )
    .await;
    assert_eq!(result["structuredContent"], entries);
    assert_eq!(panel.calls()[0].query("directory"), Some("/plugins"));
}

#[tokio::test]
async fn read_file_reads_line_ranges_and_describes_them() {
    let (result, _) = run(
        file("one\ntwo\nthree\nfour\n"),
        "read_file",
        json!({ "server": "abc", "file": "/a.txt", "start_line": 2, "end_line": 3 }),
    )
    .await;
    assert_eq!(ok_text(&result), "two\nthree\n");
    let note = result["content"][1]["text"].as_str().unwrap();
    assert!(note.contains("lines 2-3 of 4"), "{note}");
    assert!(note.contains("edit_file"), "{note}");
}

#[tokio::test]
async fn read_file_reports_missing_files() {
    let (result, _) = run(
        MockPanel::default().error(&format!("{CONTENTS}?file=/nope"), 404, "file not found"),
        "read_file",
        json!({ "server": "abc", "file": "/nope" }),
    )
    .await;
    assert!(err_text(&result).contains("does not exist"));
}

#[tokio::test]
async fn write_file_refuses_to_replace_a_large_file_blindly() {
    let big = "x\n".repeat(60_000);
    let (result, panel) = run(
        file(big),
        "write_file",
        json!({ "server": "abc", "file": "/big.log", "content": "x\n" }),
    )
    .await;
    assert!(err_text(&result).contains("edit_file"));
    assert!(sent(&panel, Method::POST).is_empty());
}

#[tokio::test]
async fn write_file_checks_the_expected_hash() {
    let (stale, panel) = run(
        file("new\n"),
        "write_file",
        json!({ "server": "abc", "file": "/a.txt", "content": "x", "expected_sha256": crate::tools::output::sha256_hex(b"old\n") }),
    )
    .await;
    assert!(err_text(&stale).contains("changed since it was read"));
    assert!(sent(&panel, Method::POST).is_empty());

    let (fresh, panel) = run(
        file("old\n"),
        "write_file",
        json!({ "server": "abc", "file": "/a.txt", "content": "x", "expected_sha256": crate::tools::output::sha256_hex(b"old\n") }),
    )
    .await;
    ok_text(&fresh);
    assert_eq!(sent(&panel, Method::POST)[0].text(), "x");
}

#[tokio::test]
async fn edit_file_replaces_exactly_one_match() {
    let (result, panel) = run(
        file("a=1\nb=2\n"),
        "edit_file",
        json!({ "server": "abc", "file": "/a.properties", "old_string": "b=2", "new_string": "b=3" }),
    )
    .await;
    assert!(ok_text(&result).contains("Replaced 1 occurrence"));
    assert_eq!(sent(&panel, Method::POST)[0].text(), "a=1\nb=3\n");
}

#[tokio::test]
async fn edit_file_refuses_missing_or_ambiguous_text() {
    let (missing, panel) = run(
        file("a=1\n"),
        "edit_file",
        json!({ "server": "abc", "file": "/a", "old_string": "c=1", "new_string": "c=2" }),
    )
    .await;
    assert!(err_text(&missing).contains("was not found"));
    assert!(sent(&panel, Method::POST).is_empty());

    let (ambiguous, _) = run(
        file("x\nx\n"),
        "edit_file",
        json!({ "server": "abc", "file": "/a", "old_string": "x", "new_string": "y" }),
    )
    .await;
    assert!(err_text(&ambiguous).contains("appears 2 times"));

    let (all, panel) = run(
        file("x\nx\n"),
        "edit_file",
        json!({ "server": "abc", "file": "/a", "old_string": "x", "new_string": "y", "replace_all": true }),
    )
    .await;
    assert!(ok_text(&all).contains("Replaced 2 occurrences"));
    assert_eq!(sent(&panel, Method::POST)[0].text(), "y\ny\n");
}

#[tokio::test]
async fn edit_file_matches_across_windows_line_endings() {
    let (result, panel) = run(
        file("a=1\r\nb=2\r\n"),
        "edit_file",
        json!({ "server": "abc", "file": "/a", "old_string": "a=1\nb=2", "new_string": "a=1\nb=3" }),
    )
    .await;
    ok_text(&result);
    assert_eq!(sent(&panel, Method::POST)[0].text(), "a=1\r\nb=3\r\n");
}

// Backups, schedules and startup variables

#[tokio::test]
async fn list_backups_pages_through_backups() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::GET, &format!("{SERVER}/backups"), json!({ "backups": { "data": [] } })),
        "list_backups",
        json!({ "server": "abc", "page": 2 }),
    )
    .await;
    assert!(ok_text(&result).contains("backups"));
    assert_eq!(panel.calls()[0].query("page"), Some("2"));
}

#[tokio::test]
async fn create_backup_sends_the_name_and_ignored_files() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::POST, &format!("{SERVER}/backups"), json!({ "backup": { "uuid": "b1" } })),
        "create_backup",
        json!({ "server": "abc", "name": "Before update", "ignored_files": ["logs/"] }),
    )
    .await;
    assert!(ok_text(&result).contains("list_backups"));
    assert_eq!(
        sent(&panel, Method::POST)[0].json(),
        json!({ "name": "Before update", "ignored_files": ["logs/"] })
    );
}

#[tokio::test]
async fn restore_backup_sends_the_options() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::POST, &format!("{SERVER}/backups/b1/restore"), json!({})),
        "restore_backup",
        json!({ "server": "abc", "backup": "b1", "truncate_directory": true }),
    )
    .await;
    ok_text(&result);
    assert_eq!(
        sent(&panel, Method::POST)[0].json(),
        json!({ "truncate_directory": true, "restore_startup": false })
    );
}

#[tokio::test]
async fn delete_backup_deletes_it() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::DELETE, &format!("{SERVER}/backups/b1"), json!({})),
        "delete_backup",
        json!({ "server": "abc", "backup": "b1" }),
    )
    .await;
    assert!(ok_text(&result).contains("Deleted backup b1"));
    assert_eq!(sent(&panel, Method::DELETE).len(), 1);

    let (bad, panel) = run(
        MockPanel::default(),
        "delete_backup",
        json!({ "server": "abc", "backup": "../x" }),
    )
    .await;
    err_text(&bad);
    assert!(panel.calls().is_empty());
}

#[tokio::test]
async fn list_schedules_lists_them() {
    let (result, _) = run(
        MockPanel::default().on_json(Method::GET, &format!("{SERVER}/schedules"), json!({ "schedules": { "data": [{ "name": "Nightly" }] } })),
        "list_schedules",
        json!({ "server": "abc" }),
    )
    .await;
    assert!(ok_text(&result).contains("Nightly"));
}

#[tokio::test]
async fn create_schedule_creates_it_and_its_steps_in_order() {
    let (result, panel) = run(
        MockPanel::default()
            .on_json(Method::POST, &format!("{SERVER}/schedules"), json!({ "schedule": { "uuid": "s1" } }))
            .on_json(Method::POST, &format!("{SERVER}/schedules/s1/steps"), json!({})),
        "create_schedule",
        json!({
            "server": "abc",
            "name": "Nightly restart",
            "cron": "0 0 4 * * *",
            "steps": [
                { "type": "send_command", "command": "say Restarting", "ignore_failure": false },
                { "type": "send_power", "action": "restart", "ignore_failure": false },
            ],
        }),
    )
    .await;
    assert!(ok_text(&result).contains("1 trigger and 2 steps"));

    let posts = sent(&panel, Method::POST);
    assert_eq!(
        posts[0].json(),
        json!({
            "name": "Nightly restart",
            "enabled": true,
            "triggers": [{ "type": "cron", "schedule": "0 0 4 * * *" }],
            "condition": { "type": "none" },
        })
    );
    assert_eq!(posts[1].json()["order"], json!(1));
    assert_eq!(posts[1].json()["action"]["type"], json!("send_command"));
    assert_eq!(posts[2].json()["order"], json!(2));
}

#[tokio::test]
async fn create_schedule_reports_a_failed_step() {
    let (result, _) = run(
        MockPanel::default()
            .on_json(Method::POST, &format!("{SERVER}/schedules"), json!({ "schedule": { "uuid": "s1" } }))
            .on(Method::POST, &format!("{SERVER}/schedules/s1/steps"), |_| {
                json_reply(400, json!({ "errors": ["unknown action"] }))
            }),
        "create_schedule",
        json!({ "server": "abc", "name": "Broken", "steps": [{ "type": "nonsense" }] }),
    )
    .await;
    let text = err_text(&result);
    assert!(text.contains("with 0 of 1 steps"), "{text}");
    assert!(text.contains("unknown action"), "{text}");
}

#[tokio::test]
async fn run_schedule_triggers_it() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::POST, &format!("{SERVER}/schedules/s1/trigger"), json!({})),
        "run_schedule",
        json!({ "server": "abc", "schedule": "s1", "skip_condition": true }),
    )
    .await;
    ok_text(&result);
    assert_eq!(sent(&panel, Method::POST)[0].json(), json!({ "skip_condition": true }));
}

#[tokio::test]
async fn list_startup_variables_lists_them() {
    let (result, _) = run(
        MockPanel::default().on_json(
            Method::GET,
            &format!("{SERVER}/startup/variables"),
            json!({ "variables": [{ "env_variable": "SERVER_JARFILE", "value": "server.jar" }] }),
        ),
        "list_startup_variables",
        json!({ "server": "abc" }),
    )
    .await;
    assert!(ok_text(&result).contains("SERVER_JARFILE"));
}

#[tokio::test]
async fn update_startup_variables_sends_only_the_named_ones() {
    let (result, panel) = run(
        MockPanel::default().on_json(Method::PUT, &format!("{SERVER}/startup/variables"), json!({})),
        "update_startup_variables",
        json!({ "server": "abc", "variables": { "MINECRAFT_VERSION": "1.21.4", "MAX_PLAYERS": 20 } }),
    )
    .await;
    assert!(ok_text(&result).contains("Restart the server"));
    let body = sent(&panel, Method::PUT)[0].json();
    let variables = body["variables"].as_array().unwrap();
    assert_eq!(variables.len(), 2);
    assert!(variables.contains(&json!({ "env_variable": "MAX_PLAYERS", "value": "20" })));
    assert!(variables.contains(&json!({ "env_variable": "MINECRAFT_VERSION", "value": "1.21.4" })));
}
