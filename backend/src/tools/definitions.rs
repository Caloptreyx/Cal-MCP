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
const CHANGE: Hints = Hints {
    read_only: false,
    destructive: false,
    idempotent: false,
};
const DANGER: Hints = Hints {
    read_only: false,
    destructive: true,
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
        tool(
            "read_api",
            "Read from the panel API",
            "Send a GET request to any /api/client or /api/admin endpoint and return the response. \
Runs with the permissions of the connected API key.",
            json!({ "path": path, "query": query }),
            &["path"],
            READ,
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

    vec![
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
        tool(
            "get_server",
            "Get server details",
            "Show a server's configuration together with its live state and resource usage (CPU, memory, disk, network, uptime).",
            json!({ "server": server }),
            &["server"],
            READ,
        ),
        tool(
            "power_server",
            "Start, stop, restart or kill a server",
            "Send a power action to a server. kill stops the process immediately without a clean shutdown.",
            json!({
                "server": server,
                "action": { "type": "string", "enum": ["start", "stop", "restart", "kill"] },
            }),
            &["server", "action"],
            DANGER,
        ),
        tool(
            "send_command",
            "Send a console command",
            "Run a command in a running server's console, for example \"say hello\" or \"list\". Use read_console afterwards to see the output.",
            json!({
                "server": server,
                "command": { "type": "string", "minLength": 1, "maxLength": 1024 },
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
                "lines": { "type": "integer", "minimum": 1, "maximum": 2000, "default": 100, "description": "How many of the last lines to return" },
            }),
            &["server"],
            READ,
        ),
    ]
}

fn file_tools() -> Vec<Value> {
    let server = json!({ "type": "string", "description": SERVER_HINT });

    vec![
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
        tool(
            "read_file",
            "Read a server file",
            "Return the contents of a text file on a server, such as server.properties or a plugin config.",
            json!({
                "server": server,
                "file": { "type": "string", "description": "File path, for example /server.properties" },
            }),
            &["server", "file"],
            READ,
        ),
        tool(
            "write_file",
            "Write a server file",
            "Create a text file on a server or replace its whole contents.",
            json!({
                "server": server,
                "file": { "type": "string", "description": "File path, for example /config/settings.yml" },
                "content": { "type": "string", "description": "The complete new contents of the file" },
            }),
            &["server", "file", "content"],
            CHANGE,
        ),
    ]
}

static DEFINITIONS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    api_tools()
        .into_iter()
        .chain(server_tools())
        .chain(file_tools())
        .collect()
});

pub fn definitions() -> &'static [Value] {
    &DEFINITIONS
}
