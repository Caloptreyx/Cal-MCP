export const PACKAGE = 'dev.caloptreyx.calmcp';

export type Client = 'claudeCode' | 'cursor' | 'vscode' | 'claudeDesktop';

export const CLIENTS: { value: Client; label: string }[] = [
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

export function snippet(client: Client, url: string): string {
  switch (client) {
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
