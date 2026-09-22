export const PACKAGE = 'dev.caloptreyx.calmcp';

export type Client = 'agent' | 'claudeCode' | 'cursor' | 'vscode' | 'claudeDesktop';

export const CLIENTS: { value: Client; label: string }[] = [
  { value: 'agent', label: 'AI agent' },
  { value: 'claudeCode', label: 'Claude Code' },
  { value: 'cursor', label: 'Cursor' },
  { value: 'vscode', label: 'VS Code' },
  { value: 'claudeDesktop', label: 'Claude Desktop' },
];

const KEY = 'YOUR_API_KEY';
const AUTHORIZATION = `Bearer ${KEY}`;

export function endpointUrl(): string {
  return `${window.location.origin}/api/client/extensions/${PACKAGE}/mcp`;
}

function agentPrompt(url: string): string {
  return [
    'Add an MCP server to your own configuration so you can manage my Calagopus game panel.',
    '',
    'Name: calagopus',
    'Transport: Streamable HTTP',
    `URL: ${url}`,
    `Header: Authorization: ${AUTHORIZATION}`,
    '',
    `Use your client's own way of adding MCP servers. In Claude Code that is: claude mcp add --transport http calagopus ${url} --header "Authorization: ${AUTHORIZATION}"`,
    'Keep the key out of shared or committed files.',
    'Once it is added, connect to it, list its tools and call list_servers to confirm it works.',
  ].join('\n');
}

export function snippet(client: Client, url: string): string {
  switch (client) {
    case 'agent':
      return agentPrompt(url);
    case 'claudeCode':
      return `claude mcp add --transport http calagopus ${url} --header "Authorization: ${AUTHORIZATION}"`;
    case 'cursor':
      return JSON.stringify({ mcpServers: { calagopus: { url, headers: { Authorization: AUTHORIZATION } } } }, null, 2);
    case 'vscode':
      return JSON.stringify(
        { servers: { calagopus: { type: 'http', url, headers: { Authorization: AUTHORIZATION } } } },
        null,
        2,
      );
    case 'claudeDesktop':
      return JSON.stringify(
        {
          mcpServers: {
            calagopus: {
              command: 'npx',
              args: ['-y', 'mcp-remote', url, '--header', `Authorization: ${AUTHORIZATION}`],
            },
          },
        },
        null,
        2,
      );
  }
}
