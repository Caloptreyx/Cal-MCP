# Cal MCP [![M8ven Score](https://m8ven.ai/badge/mcp/caloptreyx/cal-mcp)](https://m8ven.ai/mcp/caloptreyx/cal-mcp)

MCP server for [Calagopus Panel](https://calagopus.com). It lets AI assistants such as Claude, ChatGPT, Cursor and VS Code manage your panel through its API, limited to the permissions of the API key or OAuth connection they use.

## Requirements

Calagopus Panel 1.2.0 or newer.

## Install

1. In the panel, open **Admin → Extensions** and install `dev_caloptreyx_calmcp.c7s.zip`.
2. Restart the panel.

## Connect

Endpoint:

```
https://your-panel.com/api/client/extensions/dev.caloptreyx.calmcp/mcp
```

**Account → MCP** shows the endpoint and ready-made setup for each client, plus a prompt your AI agent can use to set itself up.

### Web connectors (Claude.ai, ChatGPT): OAuth

1. Add the endpoint as a custom connector (Claude: **Settings → Connectors**; ChatGPT: **Settings → Connectors**).
2. The client opens the panel. Sign in if asked, pick the access and approve.

| Access | Allows |
| --- | --- |
| Read only | GET requests and the read tools in the client area; no `/api/admin` |
| Your servers | Everything in the client area; no `/api/admin` |
| Full access | Everything your account can do, admin area included (offered to admins only) |

Approved clients are listed under **Account → MCP → Connected clients**, where you can disconnect them. On panels older than 1.2.10 the panel does not return to the approval page after signing in, so sign in first and then connect.

No OAuth connection can manage your sign-in credentials, whatever access you pick: it cannot create, change, recreate or delete API keys, SSH keys or security keys, change your email, password, password login or two-factor setup, unlink OAuth logins or end sessions. This way a client cannot leave a credential behind that outlives disconnecting it. Reading them still works. To let an assistant manage credentials, connect it with an API key instead.

Any MCP client that supports OAuth works the same way. The server follows the [MCP authorization spec](https://modelcontextprotocol.io/specification/latest/basic/authorization): protected resource metadata (RFC 9728) announced by `WWW-Authenticate` on 401, authorization server metadata (RFC 8414) at `/.well-known/oauth-authorization-server/api/auth/extensions/dev.caloptreyx.calmcp/oauth`, dynamic client registration (RFC 7591), the authorization code flow with PKCE S256 only, refresh tokens and token revocation (RFC 7009). The scopes are `read`, `client` and `full`.

### Other clients: API key

1. Create an API key under **Account → API Keys**. Give it only the permissions the assistant needs.
2. Send it as `Authorization: Bearer <api key>`. For example, in Claude Code:

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
| `power_server` | Starts, stops, restarts or kills a server; with `wait`, waits until it is running or offline and reports a crash during start-up |
| `send_command` | Sends a console command |
| `run_command` | Sends a console command and returns only the output it produced, waiting for a text or regular expression (`wait_for`) or until the console is quiet, for at most 60 seconds |
| `read_console` | Reads recent console output |
| `list_files` | Lists files in a server directory |
| `read_file` | Reads a text file, whole or by line or byte range, and reports its size, line count and SHA-256 |
| `edit_file` | Replaces exact text in a file, once or everywhere, without touching the rest |
| `write_file` | Creates or replaces a text file |
| `list_backups`, `create_backup`, `restore_backup`, `delete_backup` | Manage backups |
| `list_schedules`, `create_schedule`, `run_schedule` | List schedules, create one with a cron time and steps, or run one now |
| `list_startup_variables`, `update_startup_variables` | Read and change startup variables |

The four API tools cover every client and admin endpoint, including those added by other extensions. Every tool declares whether it only reads, whether it can destroy data and whether repeating it is safe, so clients can ask before risky calls.

`list_servers`, `get_server`, `list_files` and `read_api` also return their JSON as `structuredContent` with an `outputSchema`; the text stays for clients that do not read structured output.

### Large files

`read_file` returns at most 100 KB at a time and says when it showed only part of a file. `write_file` replaces the whole file, so a part passed back to it would cut the file short. To prevent that:

- `edit_file` changes files of any size in place.
- `write_file` refuses to replace an existing file over 100 KB unless it gets the file's `expected_sha256` from `read_file`.
- `write_file` and `edit_file` refuse to write when `expected_sha256` is given and the file has changed since it was read.

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
| Read-only | Only GET requests and read tools. Tools that change anything, and the `setup_minecraft_server` and `backup_before_update` prompts, are hidden from their lists and refused if called. GET endpoints that hand out credentials able to change things (the console websocket token, file upload URLs, node and database agent tokens and configs) and the two-factor setup endpoint are blocked as well. |
| Client API only | No requests to `/api/admin`, even when the key has admin permissions. |

- Admins can turn either limit on for every connection under **Admin → Extensions → Cal MCP**.
- Users can turn them on for each of their own API keys under **Account → MCP**. These choices can only be changed while signed in, so a key cannot lift its own limits.
- Limits add up: a key gets the panel-wide limits plus its own, and an OAuth connection also gets the limits of the access picked when approving it (Read only = read-only and client API only, Your servers = client API only). The server enforces them on every panel request the tools make, not only by hiding tools.

## Security

- Every call runs as a panel API key, so the panel's own permission checks, rate limits and activity log apply. Browser sessions and impersonation (the `Calagopus-User` header) are refused.
- An approved OAuth connection is backed by an API key the extension creates for you, named `MCP <client> <id>`, with your permissions narrowed to the chosen access and without credential management (see above). Its secret is never stored or shown. Deleting that key under **Account → API Keys** disconnects the client too, and allowed IPs you set on it apply to the connection.
- OAuth tokens, codes and client secrets are stored as SHA-256 digests. Access tokens last an hour and refresh tokens 30 days. Each refresh replaces the refresh token, and a reused refresh token or authorization code disconnects the client. Authorization codes live two minutes and work once.
- Redirect URIs must match a registered one exactly and use https, http on localhost, or an app scheme such as `cursor://`. Approving a client needs a signed-in browser session on the panel's own origin; API keys, OAuth tokens and impersonating admins cannot approve or manage connections, and the approval page refuses to work inside a frame. Each user can connect up to 25 clients.

## Support

Need help or want to request a feature? Join the [Caloptreyx Discord](https://discord.gg/4qjMWU7S8x).

## License

[MIT](LICENSE)
