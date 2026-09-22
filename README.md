# Cal MCP

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

## Security

- Every call runs as the API key, so the panel's own permission checks, rate limits and activity log apply.
- Only API keys are accepted. Browser sessions are refused.

## Support

Join the [Discord](https://discord.gg/wRrNKhxZvy).

## License

[MIT](LICENSE)
