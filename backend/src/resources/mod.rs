//! MCP resources: each server's recent console and its files.

pub mod uri;

use crate::{
    panel::{Failure, Panel, failure, get_json, object},
    tools::output::{strip_ansi, tail},
};
use base64::Engine;
use serde_json::{Map, Value, json};
use uri::Target;

const SERVERS_PER_PAGE: i64 = 50;
pub const ENTRIES_PER_PAGE: i64 = 100;
/// Larger files are refused instead of truncated, so a client never mistakes
/// part of a file for all of it.
pub const MAX_BYTES: usize = 1_000_000;

pub fn templates() -> Value {
    json!([
        {
            "uriTemplate": uri::FILES_TEMPLATE,
            "name": "server-files",
            "title": "Server file or directory",
            "description": "A file on a server, or a directory when the path ends with / (an empty path is the root). \
    Directories are a JSON listing whose entries link to their own resources; add ?page=N for the next page. \
    Files up to 1 MB are returned whole, text as text and anything else as base64.",
        },
        {
            "uriTemplate": uri::CONSOLE_TEMPLATE,
            "name": "server-console",
            "title": "Server console",
            "description": format!(
                "The most recent console output of a server, without colour codes. lines picks how many (1-{}, default {}).",
                uri::MAX_LINES,
                uri::DEFAULT_LINES
            ),
            "mimeType": "text/plain",
        },
    ])
}

fn encode_cursor(page: i64) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!("servers:{page}"))
}

fn decode_cursor(params: &Map<String, Value>) -> Result<i64, Failure> {
    let cursor = match params.get("cursor") {
        None | Some(Value::Null) => return Ok(1),
        Some(Value::String(cursor)) => cursor,
        Some(_) => return Err(Failure::Invalid("cursor must be a string".into())),
    };
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.strip_prefix("servers:")?.parse::<i64>().ok())
        .filter(|page| *page >= 1)
        .ok_or_else(|| Failure::Invalid("Invalid cursor".into()))
}

/// Whether a server's `permissions` list from the panel grants `permission`.
/// Servers the caller owns, and admins, get `*`.
fn granted(server: &Value, permission: &str) -> bool {
    match server.get("permissions").and_then(Value::as_array) {
        None => true,
        Some(permissions) => permissions
            .iter()
            .filter_map(Value::as_str)
            .any(|granted| granted == "*" || granted == permission),
    }
}

/// `resources/list`: the console and root directory of every server the
/// caller can access, one page of servers at a time.
pub async fn list(panel: &impl Panel, params: &Map<String, Value>) -> Result<Value, Failure> {
    let page = decode_cursor(params)?;
    let query = object(json!({ "page": page, "per_page": SERVERS_PER_PAGE }));
    let body = get_json(panel, "/api/client/servers", Some(&query), None).await?;

    let mut resources = Vec::new();
    for server in body
        .pointer("/servers/data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = server.get("uuid_short").and_then(Value::as_str) else {
            continue;
        };
        let name = server.get("name").and_then(Value::as_str).unwrap_or(id);

        if granted(server, "control.read-console") {
            resources.push(json!({
                "uri": uri::console(id),
                "name": format!("{name} console"),
                "title": format!("{name}: console"),
                "description": format!(
                    "The last {} lines of console output of {name} ({id}). Add ?lines=N (up to {}) for more or fewer.",
                    uri::DEFAULT_LINES,
                    uri::MAX_LINES
                ),
                "mimeType": "text/plain",
            }));
        }
        if granted(server, "files.read") {
            resources.push(json!({
                "uri": uri::directory(id, "", 1),
                "name": format!("{name} files"),
                "title": format!("{name}: files"),
                "description": format!(
                    "The root directory of {name} ({id}) as a JSON listing; every entry links to its own resource."
                ),
                "mimeType": "application/json",
            }));
        }
    }

    let total = body
        .pointer("/servers/total")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let mut result = json!({ "resources": resources });
    if page * SERVERS_PER_PAGE < total {
        result["nextCursor"] = json!(encode_cursor(page + 1));
    }
    Ok(result)
}

/// `resources/read`
pub async fn read(panel: &impl Panel, params: &Map<String, Value>) -> Result<Value, Failure> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| Failure::Invalid("resources/read needs params.uri".into()))?;
    let contents = match uri::parse(uri).map_err(Failure::Invalid)? {
        Target::Console { server, lines } => console(panel, uri, &server, lines).await?,
        Target::Directory { server, path, page } => {
            directory(panel, uri, &server, &path, page).await?
        }
        Target::File { server, path } => file(panel, uri, &server, &path).await?,
    };
    Ok(json!({ "contents": [contents] }))
}

async fn console(
    panel: &impl Panel,
    uri: &str,
    server: &str,
    lines: u32,
) -> Result<Value, Failure> {
    let query = object(json!({ "lines": lines }));
    let reply = panel
        .get(&format!("/api/client/servers/{server}/logs"), Some(&query))
        .await
        .map_err(Failure::Internal)?;
    if !(200..300).contains(&reply.status) {
        return Err(failure(&reply, Some(uri)));
    }

    // Panels without the `lines` parameter always send their default tail.
    let text = tail(
        &strip_ansi(&String::from_utf8_lossy(&reply.body)),
        lines as usize,
    );
    Ok(json!({ "uri": uri, "mimeType": "text/plain", "text": text }))
}

/// One page of a directory, as a JSON listing.
pub async fn list_directory(
    panel: &impl Panel,
    server: &str,
    path: &str,
    page: u32,
    uri: Option<&str>,
) -> Result<Value, Failure> {
    let query = object(json!({
        "directory": format!("/{path}"),
        "page": page,
        "per_page": ENTRIES_PER_PAGE,
    }));
    get_json(
        panel,
        &format!("/api/client/servers/{server}/files/list"),
        Some(&query),
        uri,
    )
    .await
}

fn child(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}/{name}")
    }
}

async fn directory(
    panel: &impl Panel,
    uri: &str,
    server: &str,
    path: &str,
    page: u32,
) -> Result<Value, Failure> {
    let body = list_directory(panel, server, path, page, Some(uri)).await?;
    Ok(listing(uri, server, path, page, &body))
}

fn listing(uri: &str, server: &str, path: &str, page: u32, body: &Value) -> Value {
    let entries: Vec<Value> = body
        .pointer("/entries/data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?;
            let is_directory = entry.get("directory").and_then(Value::as_bool) == Some(true);
            let child = child(path, name);
            let mut item = json!({
                "name": name,
                "type": if is_directory { "directory" } else { "file" },
                "uri": if is_directory { uri::directory(server, &child, 1) } else { uri::file(server, &child) },
            });
            if !is_directory {
                item["size"] = entry.get("size").cloned().unwrap_or(Value::Null);
                item["mime"] = entry.get("mime").cloned().unwrap_or(Value::Null);
            }
            if entry.get("symlink").and_then(Value::as_bool) == Some(true) {
                item["symlink"] = json!(true);
            }
            item["modified"] = entry.get("modified").cloned().unwrap_or(Value::Null);
            Some(item)
        })
        .collect();

    let total = body
        .pointer("/entries/total")
        .and_then(Value::as_i64)
        .unwrap_or(entries.len() as i64);
    let mut listing = json!({
        "server": server,
        "directory": format!("/{path}"),
        "page": page,
        "total": total,
        "entries": entries,
    });
    if i64::from(page) * ENTRIES_PER_PAGE < total {
        listing["nextPage"] = json!(uri::directory(server, path, page + 1));
    }

    json!({
        "uri": uri,
        "mimeType": "application/json",
        "text": serde_json::to_string_pretty(&listing).unwrap_or_default(),
    })
}

async fn file(panel: &impl Panel, uri: &str, server: &str, path: &str) -> Result<Value, Failure> {
    let query = object(json!({ "file": format!("/{path}") }));
    let reply = panel
        .get(
            &format!("/api/client/servers/{server}/files/contents"),
            Some(&query),
        )
        .await
        .map_err(Failure::Internal)?;

    if (200..300).contains(&reply.status) {
        return file_contents(uri, path, &reply.body);
    }

    // The URI had no trailing slash, but the path may still be a directory.
    if matches!(reply.status, 400 | 404 | 422)
        && let Ok(body) = list_directory(panel, server, path, 1, Some(uri)).await
    {
        return Ok(listing(uri, server, path, 1, &body));
    }

    Err(match reply.status {
        413 => Failure::Invalid(format!(
            "/{path} is larger than the panel lets the file manager open; download it instead"
        )),
        _ => failure(&reply, Some(uri)),
    })
}

fn file_contents(uri: &str, path: &str, body: &[u8]) -> Result<Value, Failure> {
    if body.len() > MAX_BYTES {
        return Err(Failure::Invalid(format!(
            "/{path} is {} bytes, more than the {MAX_BYTES} bytes a resource may return. Read it in parts with the read_file tool.",
            body.len()
        )));
    }

    let mime = mime_type(path);
    let text = (!body.contains(&0))
        .then(|| std::str::from_utf8(body).ok())
        .flatten();
    Ok(match text {
        Some(text) => json!({
            "uri": uri,
            "mimeType": mime.filter(|mime| is_text_mime(mime)).unwrap_or("text/plain"),
            "text": text,
        }),
        None => json!({
            "uri": uri,
            "mimeType": mime.unwrap_or("application/octet-stream"),
            "blob": base64::engine::general_purpose::STANDARD.encode(body),
        }),
    })
}

fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/yaml"
                | "application/toml"
                | "application/xml"
                | "application/javascript"
                | "application/x-sh"
                | "image/svg+xml"
        )
}

fn mime_type(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "txt" | "log" | "properties" | "cfg" | "conf" | "ini" | "env" | "lang" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" | "json5" | "mcmeta" => "application/json",
        "yml" | "yaml" => "application/yaml",
        "toml" => "application/toml",
        "xml" => "application/xml",
        "sh" => "application/x-sh",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "jar" => "application/java-archive",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::mock::MockPanel;

    const ID: &str = "0a1b2c3d";

    fn params(value: Value) -> Map<String, Value> {
        object(value)
    }

    fn server(id: &str, name: &str, permissions: &[&str]) -> Value {
        json!({ "uuid_short": id, "name": name, "permissions": permissions })
    }

    #[tokio::test]
    async fn list_follows_permissions_and_paginates() {
        let panel = MockPanel::default().json(
            "/api/client/servers?page=1&per_page=50",
            json!({ "servers": {
                "total": 120, "per_page": 50, "page": 1,
                "data": [
                    server(ID, "Survival", &["*"]),
                    server("11111111", "Shared", &["files.read"]),
                    server("22222222", "Console only", &["control.read-console"]),
                ],
            }}),
        );

        let result = list(&panel, &Map::new()).await.unwrap();
        let uris: Vec<&str> = result["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|resource| resource["uri"].as_str().unwrap())
            .collect();
        assert_eq!(
            uris,
            [
                "calagopus://servers/0a1b2c3d/console",
                "calagopus://servers/0a1b2c3d/files/",
                "calagopus://servers/11111111/files/",
                "calagopus://servers/22222222/console",
            ]
        );

        let cursor = result["nextCursor"].as_str().unwrap().to_string();
        let panel = MockPanel::default().json(
            "/api/client/servers?page=2&per_page=50",
            json!({ "servers": { "total": 51, "per_page": 50, "page": 2, "data": [server("33333333", "Last", &["*"])] } }),
        );
        let result = list(&panel, &params(json!({ "cursor": cursor })))
            .await
            .unwrap();
        assert_eq!(result["resources"].as_array().unwrap().len(), 2);
        assert!(result.get("nextCursor").is_none());
    }

    #[tokio::test]
    async fn list_rejects_bad_cursors() {
        for cursor in [json!("nonsense"), json!(3), json!(encode_cursor(0))] {
            let result = list(&MockPanel::default(), &params(json!({ "cursor": cursor }))).await;
            assert!(matches!(result, Err(Failure::Invalid(_))), "{cursor}");
        }
    }

    #[tokio::test]
    async fn reads_the_console_without_colour_codes() {
        let panel = MockPanel::default().reply(
            "/api/client/servers/0a1b2c3d/logs?lines=2",
            200,
            "text/plain",
            b"one\n\x1b[31mtwo\x1b[0m\nthree",
        );
        let result = read(
            &panel,
            &params(json!({ "uri": "calagopus://servers/0a1b2c3d/console?lines=2" })),
        )
        .await
        .unwrap();
        assert_eq!(
            result["contents"][0],
            json!({ "uri": "calagopus://servers/0a1b2c3d/console?lines=2", "mimeType": "text/plain", "text": "two\nthree" })
        );
    }

    #[tokio::test]
    async fn reads_text_and_binary_files() {
        let panel = MockPanel::default()
            .reply(
                "/api/client/servers/0a1b2c3d/files/contents?file=/config/My Plugin.yml",
                200,
                "application/octet-stream",
                b"enabled: true\n",
            )
            .reply(
                "/api/client/servers/0a1b2c3d/files/contents?file=/icon.png",
                200,
                "application/octet-stream",
                &[0x89, b'P', b'N', b'G', 0, 1],
            );

        let uri = "calagopus://servers/0a1b2c3d/files/config/My%20Plugin.yml";
        let result = read(&panel, &params(json!({ "uri": uri }))).await.unwrap();
        assert_eq!(
            result["contents"][0],
            json!({ "uri": uri, "mimeType": "application/yaml", "text": "enabled: true\n" })
        );

        let uri = "calagopus://servers/0a1b2c3d/files/icon.png";
        let result = read(&panel, &params(json!({ "uri": uri }))).await.unwrap();
        assert_eq!(result["contents"][0]["mimeType"], json!("image/png"));
        assert_eq!(result["contents"][0]["blob"], json!("iVBORwAB"));
        assert!(result["contents"][0].get("text").is_none());
    }

    #[tokio::test]
    async fn refuses_files_over_the_limit_instead_of_truncating() {
        let panel = MockPanel::default().reply(
            "/api/client/servers/0a1b2c3d/files/contents?file=/big.log",
            200,
            "application/octet-stream",
            &vec![b'a'; MAX_BYTES + 1],
        );
        let result = read(
            &panel,
            &params(json!({ "uri": "calagopus://servers/0a1b2c3d/files/big.log" })),
        )
        .await;
        assert!(matches!(result, Err(Failure::Invalid(message)) if message.contains("read_file")));
    }

    fn entries(total: i64, data: Value) -> Value {
        json!({ "entries": { "total": total, "per_page": 100, "page": 1, "data": data } })
    }

    #[tokio::test]
    async fn lists_directories_with_links_and_next_page() {
        let panel = MockPanel::default().json(
            "/api/client/servers/0a1b2c3d/files/list?directory=/plugins&page=1&per_page=100",
            entries(
                150,
                json!([
                    { "name": "Essentials", "directory": true, "file": false, "symlink": false, "size": 0, "mime": "inode/directory", "modified": "2026-01-01T00:00:00Z" },
                    { "name": "a b.jar", "directory": false, "file": true, "symlink": false, "size": 42, "mime": "application/java-archive", "modified": "2026-01-02T00:00:00Z" },
                ]),
            ),
        );
        let uri = "calagopus://servers/0a1b2c3d/files/plugins/";
        let result = read(&panel, &params(json!({ "uri": uri }))).await.unwrap();
        let contents = &result["contents"][0];
        assert_eq!(contents["uri"], json!(uri));
        assert_eq!(contents["mimeType"], json!("application/json"));

        let listing: Value = serde_json::from_str(contents["text"].as_str().unwrap()).unwrap();
        assert_eq!(listing["directory"], json!("/plugins"));
        assert_eq!(
            listing["entries"][0]["uri"],
            json!("calagopus://servers/0a1b2c3d/files/plugins/Essentials/")
        );
        assert_eq!(
            listing["entries"][1]["uri"],
            json!("calagopus://servers/0a1b2c3d/files/plugins/a%20b.jar")
        );
        assert_eq!(listing["entries"][1]["size"], json!(42));
        assert_eq!(
            listing["nextPage"],
            json!("calagopus://servers/0a1b2c3d/files/plugins/?page=2")
        );
    }

    #[tokio::test]
    async fn a_path_without_slash_falls_back_to_a_listing() {
        let panel = MockPanel::default()
            .error(
                "/api/client/servers/0a1b2c3d/files/contents?file=/world",
                400,
                "path is a directory",
            )
            .json(
                "/api/client/servers/0a1b2c3d/files/list?directory=/world&page=1&per_page=100",
                entries(0, json!([])),
            );
        let result = read(
            &panel,
            &params(json!({ "uri": "calagopus://servers/0a1b2c3d/files/world" })),
        )
        .await
        .unwrap();
        assert_eq!(result["contents"][0]["mimeType"], json!("application/json"));
    }

    #[tokio::test]
    async fn missing_and_forbidden_resources_fail_clearly() {
        let panel = MockPanel::default()
            .error(
                "/api/client/servers/0a1b2c3d/files/contents?file=/secret.txt",
                403,
                "permission denied",
            )
            .error(
                "/api/client/servers/0a1b2c3d/logs?lines=200",
                404,
                "server not found",
            );

        let result = read(
            &panel,
            &params(json!({ "uri": "calagopus://servers/0a1b2c3d/files/secret.txt" })),
        )
        .await;
        assert!(
            matches!(result, Err(Failure::Invalid(message)) if message.contains("permission denied"))
        );
        // A forbidden file is not retried as a directory.
        assert_eq!(panel.requested().len(), 1);

        let missing = "calagopus://servers/0a1b2c3d/files/missing.txt";
        assert!(matches!(
            read(&panel, &params(json!({ "uri": missing }))).await,
            Err(Failure::NotFound { uri, .. }) if uri == missing
        ));

        assert!(matches!(
            read(
                &panel,
                &params(json!({ "uri": "calagopus://servers/0a1b2c3d/console" }))
            )
            .await,
            Err(Failure::NotFound { .. })
        ));
        assert!(matches!(
            read(
                &panel,
                &params(json!({ "uri": "calagopus://servers/0a1b2c3d/files/../x" }))
            )
            .await,
            Err(Failure::Invalid(_))
        ));
    }
}
