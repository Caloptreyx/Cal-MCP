# Cal MCP [![M8ven Score](https://m8ven.ai/badge/mcp/caloptreyx/cal-mcp)](https://m8ven.ai/mcp/caloptreyx/cal-mcp)

MCP server for [Calagopus Panel](https://calagopus.com). It lets AI assistants such as Claude, Cursor and VS Code manage your panel through its API, limited to the permissions of the API key they connect with.

## Requirements

Calagopus Panel 1.2.0 or newer.

## Install

1. In the panel, open **Admin → Extensions** and install `dev_caloptreyx_calmcp.c7s.zip`.
2. Restart the panel.

## Connect

1. Create an API key under **Account → API Keys**. Give it only the permissions the assistant needs.
2. Open **Account → MCP**. It shows the endpoint and ready-made setup for Claude Code, Cursor, VS Code and Claude Desktop, plus a prompt your AI agent can use to set itself up.

Endpoint:

```
https://your-panel.com/api/client/extensions/dev.caloptreyx.calmcp/mcp
```

Send the key as `Authorization: Bearer <api key>`. For example, in Claude Code:

```
claude mcp add --transport http calagopus https://your-panel.com/api/client/extensions/dev.caloptreyx.calmcp/mcp --header "Authorization: Bearer <api key>"
```

## Tools

| Tool | What it does |
| --- | --- |
| `search_api` | Finds panel API endpoints by keywords |
| `describe_api` | Shows an endpoint's parameters, request body and response |
| `read_api` | Sends a GET request to any `/api/client` or `/api/admin` endpoint |
| `write_api` | Sends a POST, PUT, PATCH or DELETE request to any `/api/client` or `/api/admin` endpoint |
| `list_servers` | Lists the servers the account can access |
| `get_server` | Shows a server's details and live resource usage |
| `power_server` | Starts, stops, restarts or kills a server |
| `send_command` | Sends a console command |
| `read_console` | Reads recent console output |
| `list_files` | Lists files in a server directory |
| `read_file` | Reads a text file |
| `write_file` | Creates or replaces a text file |

The four API tools cover every client and admin endpoint, including those added by other extensions.

## Prompts

Prompts are ready-made jobs you pick in your client, for example as slash commands. Server arguments accept a name, UUID or short ID, and clients that support completion suggest them as you type.

| Prompt | Arguments | What it does |
| --- | --- | --- |
| `diagnose_crash` | `server` | Works out why a server crashed, starting from its recent console output |
| `setup_minecraft_server` | `software`, `version`, `name`, `memory`, `server` (all optional) | Creates or reconfigures a Minecraft server step by step, and asks you before accepting the EULA |
| `backup_before_update` | `server`, `update` | Makes a backup and checks that it finished before anything is updated |
| `find_resource_usage` | `server`, `resource` (both optional) | Finds which servers, or what inside one server, use the most CPU, memory, disk or network |

## Resources

Clients that support resources can attach a server's console or files to a conversation.

| Resource | Contents |
| --- | --- |
| `calagopus://servers/{server}/console{?lines}` | The last 200 console lines (up to 1000 with `?lines=`), without colour codes |
| `calagopus://servers/{server}/files/{+path}` | A file (text, or base64 for binary files, up to 1 MB), or a JSON directory listing when the path ends with `/`; listings link every entry and page with `?page=` |

`{server}` is a server UUID or short ID. Both templates complete `{server}`, and the files template completes `{path}`. The resource list holds the console and root directory of every server the key can access, 50 servers per page. Reads use the same panel endpoints as the tools, so the key's and subuser's permissions apply: a subuser without console access neither sees nor reads the console.
## Access limits

Two limits keep an assistant from doing more than you want:

| Limit | What it allows |
| --- | --- |
| Read-only | Only GET requests and read tools. Tools that change anything are hidden from the tool list and refused if called. |
| Client API only | No requests to `/api/admin`, even when the key has admin permissions. |

- Admins can turn either limit on for every connection under **Admin → Extensions → Cal MCP**.
- Users can turn them on for each of their own API keys under **Account → MCP**. These choices can only be changed while signed in, so a key cannot lift its own limits.
- Limits add up: a key gets the panel-wide limits plus its own. The server enforces them on every panel request the tools make, not only by hiding tools.

## Security

- Every call runs as the API key, so the panel's own permission checks, rate limits and activity log apply.
- Only API keys are accepted. Browser sessions are refused.

## Support

Need help or want to request a feature? Join the [Caloptreyx Discord](https://discord.gg/4qjMWU7S8x).

## License

[MIT](LICENSE)
