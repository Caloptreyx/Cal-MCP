use serde_json::{Value, json};
use std::sync::LazyLock;

const SERVER_HINT: &str = "Server UUID or short ID, as returned by list_servers";

struct Hints {
    read_only: bool,
    destructive: bool,
    idempotent: bool,
}

const READ: Hints = Hints {
    read_only: true,
    destructive: false,
    idempotent: true,
};
const DANGER: Hints = Hints {
    read_only: false,
    destructive: true,
    idempotent: false,
};
// Replaces data, so the old values are lost, but doing it twice with the same
// arguments leaves the same result.
const OVERWRITE: Hints = Hints {
    read_only: false,
    destructive: true,
    idempotent: true,
};
// Adds something new every time and removes nothing.
const CREATE: Hints = Hints {
    read_only: false,
    destructive: false,
    idempotent: false,
};

fn tool(
    name: &str,
    title: &str,
    description: &str,
    properties: Value,
    required: &[&str],
    hints: Hints,
) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
        "annotations": {
            "title": title,
            "readOnlyHint": hints.read_only,
            "destructiveHint": hints.destructive,
            "idempotentHint": hints.idempotent,
            "openWorldHint": false,
        },
    })
}

/// Declares the shape of the tool's `structuredContent`.
fn with_output(mut tool: Value, schema: Value) -> Value {
    tool["outputSchema"] = schema;
    tool
}

/// A panel page of `items`, as the panel's pagination serializes it.
fn page_of(items: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "total": { "type": "integer" },
            "page": { "type": "integer" },
            "per_page": { "type": "integer" },
            "data": { "type": "array", "items": { "type": "object" }, "description": items },
        },
        "required": ["data"],
    })
}

fn api_tools() -> Vec<Value> {
    let path = json!({
        "type": "string",
        "description": "Endpoint path starting with /api/client/ or /api/admin/, every {placeholder} replaced with a real value",
    });
    let query = json!({
        "type": "object",
        "description": "Query parameters, for example {\"page\": 1, \"per_page\": 25}",
        "additionalProperties": { "type": ["string", "number", "boolean", "array", "null"] },
    });

    vec![
        tool(
            "search_api",
            "Search the panel API",
            "Find panel API endpoints by keywords such as \"server backups\", \"create user\" or \"node allocations\". \
Returns method, path and inputs of the best matches (* marks required query parameters). \
Follow up with describe_api for the exact body and response, then call the endpoint with read_api or write_api.",
            json!({
                "query": { "type": "string", "minLength": 1, "description": "What you want to do or find" },
                "scope": { "type": "string", "enum": ["all", "client", "admin"], "default": "all", "description": "client: your servers and account; admin: panel administration" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 15 },
            }),
            &["query"],
            READ,
        ),
        tool(
            "describe_api",
            "Describe a panel API endpoint",
            "Show every parameter, the request body schema and the response schema of one endpoint. \
Accepts the path template from search_api or a concrete path.",
            json!({
                "method": { "type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"] },
                "path": { "type": "string", "description": "For example /api/client/servers/{server}/backups" },
            }),
            &["method", "path"],
            READ,
        ),
        with_output(
            tool(
                "read_api",
                "Read from the panel API",
                "Send a GET request to any /api/client or /api/admin endpoint and return the response. \
Runs with the permissions of the connected API key.",
                json!({ "path": path, "query": query }),
                &["path"],
                READ,
            ),
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "integer", "description": "HTTP status of the response" },
                    "data": { "description": "The response body: parsed JSON, or the text when it is not JSON. null when truncated." },
                    "truncated": { "type": "boolean", "description": "The body was too large and is only in the text content, cut short" },
                },
                "required": ["status", "data", "truncated"],
            }),
        ),
        tool(
            "write_api",
            "Change data through the panel API",
            "Send a POST, PUT, PATCH or DELETE request to any /api/client or /api/admin endpoint. \
This changes real data on the panel, so check the endpoint with describe_api first. \
`body` is sent as JSON; a plain string is sent as raw text, which is what file writes expect.",
            json!({
                "method": { "type": "string", "enum": ["POST", "PUT", "PATCH", "DELETE"] },
                "path": path,
                "query": query,
                "body": { "description": "Request body: any JSON value, or a string for raw text" },
            }),
            &["method", "path"],
            DANGER,
        ),
    ]
}

fn server_tools() -> Vec<Value> {
    let server = json!({ "type": "string", "description": SERVER_HINT });
    let command = json!({ "type": "string", "minLength": 1, "maxLength": 1024 });

    vec![
        with_output(
            tool(
                "list_servers",
                "List servers",
                "List the servers this account can access, with their UUID, short ID, name, node and limits.",
                json!({
                    "search": { "type": "string", "description": "Filter by name or ID" },
                    "page": { "type": "integer", "minimum": 1, "default": 1 },
                    "per_page": { "type": "integer", "minimum": 1, "maximum": 100, "default": 25 },
                    "other": { "type": "boolean", "default": false, "description": "Admins only: list servers owned by other users" },
                }),
                &[],
                READ,
            ),
            json!({
                "type": "object",
                "properties": { "servers": page_of("Servers") },
                "required": ["servers"],
            }),
        ),
        with_output(
            tool(
                "get_server",
                "Get server details",
                "Show a server's configuration together with its live state and resource usage (CPU, memory, disk, network, uptime).",
                json!({ "server": server }),
                &["server"],
                READ,
            ),
            json!({
                "type": "object",
                "properties": {
                    "server": { "description": "The server's configuration, limits and feature limits" },
                    "resources": { "description": "Live state (offline, starting, running, stopping) and resource usage, or an error" },
                },
                "required": ["server", "resources"],
            }),
        ),
        tool(
            "power_server",
            "Start, stop, restart or kill a server",
            "Send a power action to a server. kill stops the process immediately without a clean shutdown. \
With wait, the tool waits until the server is running (start, restart) or offline (stop, kill), and reports a crash during start-up.",
            json!({
                "server": server,
                "action": { "type": "string", "enum": ["start", "stop", "restart", "kill"] },
                "wait": { "type": "boolean", "default": false, "description": "Wait until the server reaches the new state" },
                "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 120, "default": 60, "description": "How long to wait" },
            }),
            &["server", "action"],
            DANGER,
        ),
        tool(
            "send_command",
            "Send a console command",
            "Run a command in a running server's console, for example \"say hello\" or \"list\", without waiting for its output. \
To see what the command prints, use run_command instead.",
            json!({ "server": server, "command": command }),
            &["server", "command"],
            DANGER,
        ),
        tool(
            "run_command",
            "Run a console command and wait for its output",
            "Run a command in a running server's console and return only the console output it produced. \
With wait_for, waits until that text (or regular expression, with regex) appears; without it, returns once the console is quiet. \
Waits at most timeout_seconds.",
            json!({
                "server": server,
                "command": command,
                "wait_for": { "type": "string", "description": "Text to wait for in the new output, for example \"Done\" or \"players online\"" },
                "regex": { "type": "boolean", "default": false, "description": "Treat wait_for as a regular expression" },
                "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 60, "default": 10 },
            }),
            &["server", "command"],
            DANGER,
        ),
        tool(
            "read_console",
            "Read the console log",
            "Return the most recent console output of a server.",
            json!({
                "server": server,
                "lines": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 100, "description": "How many of the last lines to return" },
            }),
            &["server"],
            READ,
        ),
    ]
}

fn file_tools() -> Vec<Value> {
    let server = json!({ "type": "string", "description": SERVER_HINT });
    let expected = json!({
        "type": "string",
        "description": "The sha256 read_file reported. The change is refused if the file has changed since.",
    });

    vec![
        with_output(
            tool(
                "list_files",
                "List server files",
                "List the files and folders in a directory of a server.",
                json!({
                    "server": server,
                    "directory": { "type": "string", "default": "/", "description": "Directory to list, for example /plugins" },
                    "page": { "type": "integer", "minimum": 1, "default": 1 },
                    "per_page": { "type": "integer", "minimum": 1, "maximum": 100, "default": 100 },
                }),
                &["server"],
                READ,
            ),
            json!({
                "type": "object",
                "properties": { "entries": page_of("Files and directories with name, size, mode and modification time") },
                "required": ["entries"],
            }),
        ),
        tool(
            "read_file",
            "Read a server file",
            "Return the contents of a text file on a server, such as server.properties or a plugin config, followed by a note with its size, line count and sha256. \
Large files come back in parts of at most 100 KB: read on with start_line/end_line (one-based, inclusive) or offset/length (bytes). \
Never pass a part to write_file; change large files with edit_file.",
            json!({
                "server": server,
                "file": { "type": "string", "description": "File path, for example /server.properties" },
                "start_line": { "type": "integer", "minimum": 1, "description": "First line to return" },
                "end_line": { "type": "integer", "minimum": 1, "description": "Last line to return" },
                "offset": { "type": "integer", "minimum": 0, "description": "First byte to return, instead of lines" },
                "length": { "type": "integer", "minimum": 1, "description": "How many bytes to return" },
            }),
            &["server", "file"],
            READ,
        ),
        tool(
            "write_file",
            "Write a server file",
            "Create a text file on a server or replace its whole contents. \
To change part of an existing file, use edit_file. Files over 100 KB are only replaced when expected_sha256 is given.",
            json!({
                "server": server,
                "file": { "type": "string", "description": "File path, for example /config/settings.yml" },
                "content": { "type": "string", "description": "The complete new contents of the file" },
                "expected_sha256": expected,
            }),
            &["server", "file", "content"],
            OVERWRITE,
        ),
        tool(
            "edit_file",
            "Edit a server file",
            "Replace exact text in a text file on a server, leaving the rest of the file untouched, so it is safe on files of any size. \
old_string must match the file exactly, including indentation and line breaks, and must occur once unless replace_all is set.",
            json!({
                "server": server,
                "file": { "type": "string", "description": "File path, for example /config/settings.yml" },
                "old_string": { "type": "string", "minLength": 1, "description": "The exact text to replace" },
                "new_string": { "type": "string", "description": "The text to put in its place" },
                "replace_all": { "type": "boolean", "default": false, "description": "Replace every occurrence instead of exactly one" },
                "expected_sha256": expected,
            }),
            &["server", "file", "old_string", "new_string"],
            DANGER,
        ),
    ]
}

fn management_tools() -> Vec<Value> {
    let server = json!({ "type": "string", "description": SERVER_HINT });
    let page = json!({ "type": "integer", "minimum": 1, "default": 1 });
    let per_page = json!({ "type": "integer", "minimum": 1, "maximum": 100, "default": 25 });
    let backup = json!({ "type": "string", "description": "Backup UUID, as returned by list_backups" });
    let schedule = json!({ "type": "string", "description": "Schedule UUID, as returned by list_schedules" });

    vec![
        tool(
            "list_backups",
            "List backups",
            "List a server's backups with their UUID, name, size, whether they completed and whether they are locked.",
            json!({
                "server": server,
                "search": { "type": "string", "description": "Filter by name" },
                "page": page,
                "per_page": per_page,
            }),
            &["server"],
            READ,
        ),
        tool(
            "create_backup",
            "Create a backup",
            "Start a backup of a server's files. It runs in the background; list_backups shows when it has completed.",
            json!({
                "server": server,
                "name": { "type": "string", "maxLength": 255, "description": "Defaults to the current date and time" },
                "ignored_files": { "type": "array", "items": { "type": "string" }, "description": "Paths or patterns to leave out, like .gitignore lines" },
            }),
            &["server"],
            CREATE,
        ),
        tool(
            "restore_backup",
            "Restore a backup",
            "Replace a server's files with a backup. Files changed since the backup are overwritten; with truncate_directory, every file not in the backup is deleted first.",
            json!({
                "server": server,
                "backup": backup,
                "truncate_directory": { "type": "boolean", "default": false, "description": "Delete all current files before restoring" },
                "restore_startup": { "type": "boolean", "default": false, "description": "Also restore the startup command and variables saved with the backup" },
            }),
            &["server", "backup"],
            DANGER,
        ),
        tool(
            "delete_backup",
            "Delete a backup",
            "Delete a backup permanently.",
            json!({ "server": server, "backup": backup }),
            &["server", "backup"],
            OVERWRITE,
        ),
        tool(
            "list_schedules",
            "List schedules",
            "List a server's schedules with their UUID, triggers, condition and when they last ran.",
            json!({
                "server": server,
                "search": { "type": "string", "description": "Filter by name" },
                "page": page,
                "per_page": per_page,
            }),
            &["server"],
            READ,
        ),
        tool(
            "create_schedule",
            "Create a schedule",
            "Create a schedule that runs steps on a server. cron adds a time trigger; triggers takes any trigger objects. \
Each step is an action object, for example {\"type\": \"send_command\", \"command\": \"say Restarting\", \"ignore_failure\": false}, \
{\"type\": \"sleep\", \"duration\": 5000}, {\"type\": \"send_power\", \"action\": \"restart\", \"ignore_failure\": false} or \
{\"type\": \"create_backup\", \"foreground\": true, \"ignored_files\": [], \"ignore_failure\": false}. \
describe_api for POST /api/client/servers/{server}/schedules/{schedule}/steps lists every action.",
            json!({
                "server": server,
                "name": { "type": "string", "minLength": 1, "maxLength": 255 },
                "cron": { "type": "string", "description": "Six fields: second minute hour day month weekday, for example \"0 0 4 * * *\" for 04:00 every day" },
                "triggers": { "type": "array", "items": { "type": "object" }, "description": "Trigger objects such as {\"type\": \"server_state\", \"state\": \"offline\"}" },
                "condition": { "type": "object", "description": "Only run when this holds, for example {\"type\": \"server_state\", \"state\": \"running\"}; defaults to always" },
                "steps": { "type": "array", "items": { "type": "object" }, "maxItems": 100, "description": "Actions to run in order" },
                "enabled": { "type": "boolean", "default": true },
            }),
            &["server", "name"],
            CREATE,
        ),
        tool(
            "run_schedule",
            "Run a schedule now",
            "Run a schedule's steps now, in the background.",
            json!({
                "server": server,
                "schedule": schedule,
                "skip_condition": { "type": "boolean", "default": false, "description": "Run even if the schedule's condition does not hold" },
            }),
            &["server", "schedule"],
            DANGER,
        ),
        tool(
            "list_startup_variables",
            "List startup variables",
            "List a server's startup variables with their name, environment variable, current value, rules and whether you may edit them.",
            json!({ "server": server }),
            &["server"],
            READ,
        ),
        tool(
            "update_startup_variables",
            "Update startup variables",
            "Set new values for a server's startup variables. Only the variables you name change; the server needs a restart to use them.",
            json!({
                "server": server,
                "variables": {
                    "type": "object",
                    "minProperties": 1,
                    "additionalProperties": { "type": ["string", "number", "boolean"] },
                    "description": "Environment variable names and their new values, for example {\"MINECRAFT_VERSION\": \"1.21.4\"}",
                },
            }),
            &["server", "variables"],
            OVERWRITE,
        ),
    ]
}

static DEFINITIONS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    api_tools()
        .into_iter()
        .chain(server_tools())
        .chain(file_tools())
        .chain(management_tools())
        .collect()
});

pub fn definitions() -> &'static [Value] {
    &DEFINITIONS
}

#[cfg(test)]
mod tests {
    use super::*;

    // name, readOnlyHint, destructiveHint, idempotentHint
    const EXPECTED: [(&str, bool, bool, bool); 23] = [
        ("search_api", true, false, true),
        ("describe_api", true, false, true),
        ("read_api", true, false, true),
        ("write_api", false, true, false),
        ("list_servers", true, false, true),
        ("get_server", true, false, true),
        ("power_server", false, true, false),
        ("send_command", false, true, false),
        ("run_command", false, true, false),
        ("read_console", true, false, true),
        ("list_files", true, false, true),
        ("read_file", true, false, true),
        ("write_file", false, true, true),
        ("edit_file", false, true, false),
        ("list_backups", true, false, true),
        ("create_backup", false, false, false),
        ("restore_backup", false, true, false),
        ("delete_backup", false, true, true),
        ("list_schedules", true, false, true),
        ("create_schedule", false, false, false),
        ("run_schedule", false, true, false),
        ("list_startup_variables", true, false, true),
        ("update_startup_variables", false, true, true),
    ];

    const STRUCTURED: [&str; 4] = ["read_api", "list_servers", "get_server", "list_files"];

    fn named(name: &str) -> &'static Value {
        definitions()
            .iter()
            .find(|tool| tool["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("tool {name} is not declared"))
    }

    #[test]
    fn declares_every_tool_once() {
        assert_eq!(definitions().len(), EXPECTED.len());
        for (name, ..) in EXPECTED {
            named(name);
        }
    }

    #[test]
    fn every_tool_declares_all_four_boolean_hints() {
        for tool in definitions() {
            let name = tool["name"].as_str().unwrap_or_default();
            let annotations = tool["annotations"]
                .as_object()
                .unwrap_or_else(|| panic!("{name} has no annotations"));
            for hint in [
                "readOnlyHint",
                "destructiveHint",
                "idempotentHint",
                "openWorldHint",
            ] {
                assert!(
                    annotations.get(hint).and_then(Value::as_bool).is_some(),
                    "{name} is missing a boolean {hint}"
                );
            }
        }
    }

    #[test]
    fn hints_match_handler_behaviour() {
        for (name, read_only, destructive, idempotent) in EXPECTED {
            let annotations = &named(name)["annotations"];
            assert_eq!(
                annotations["readOnlyHint"],
                json!(read_only),
                "{name} readOnlyHint"
            );
            assert_eq!(
                annotations["destructiveHint"],
                json!(destructive),
                "{name} destructiveHint"
            );
            assert_eq!(
                annotations["idempotentHint"],
                json!(idempotent),
                "{name} idempotentHint"
            );
            assert_eq!(
                annotations["openWorldHint"],
                json!(false),
                "{name} openWorldHint"
            );
        }
    }

    #[test]
    fn schemas_are_well_formed() {
        for tool in definitions() {
            let name = tool["name"].as_str().unwrap_or_default();
            assert!(!name.is_empty(), "a tool has no name");
            assert!(
                tool["description"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                "{name} has no description"
            );

            let schema = tool["inputSchema"]
                .as_object()
                .unwrap_or_else(|| panic!("{name} has no inputSchema"));
            assert_eq!(schema["type"], json!("object"), "{name} inputSchema type");
            assert_eq!(
                schema["additionalProperties"],
                json!(false),
                "{name} inputSchema additionalProperties"
            );

            let properties = schema["properties"].as_object().unwrap();
            for required in schema["required"].as_array().unwrap() {
                let required = required.as_str().unwrap();
                assert!(
                    properties.contains_key(required),
                    "{name} requires undeclared property {required}"
                );
            }
        }
    }

    #[test]
    fn json_tools_declare_an_object_output_schema() {
        for tool in definitions() {
            let name = tool["name"].as_str().unwrap();
            let schema = tool.get("outputSchema");
            if STRUCTURED.contains(&name) {
                let schema = schema.unwrap_or_else(|| panic!("{name} has no outputSchema"));
                // Protocol versions before 2026-07-28 only allow object results.
                assert_eq!(schema["type"], json!("object"), "{name} outputSchema type");
                let properties = schema["properties"].as_object().unwrap();
                for required in schema["required"].as_array().unwrap() {
                    assert!(properties.contains_key(required.as_str().unwrap()));
                }
            } else {
                assert!(schema.is_none(), "{name} declares an outputSchema");
            }
        }
    }
}
