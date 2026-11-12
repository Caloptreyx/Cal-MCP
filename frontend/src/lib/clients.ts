export const PACKAGE = 'dev.caloptreyx.calmcp';

export type Client = 'agent' | 'claudeCode' | 'cursor' | 'vscode' | 'claudeDesktop';

export interface Branding {
  integrationName: string;
  displayTitle: string;
  panelName: string;
}

export const DEFAULT_BRANDING: Branding = {
  integrationName: 'calagopus',
  displayTitle: 'Cal MCP',
  panelName: 'Calagopus',
};

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

function agentPrompt(url: string, branding: Branding): string {
  return [
    `Add an MCP server to your own configuration so you can manage my ${branding.panelName} game panel.`,
    '',
    `Name: ${branding.integrationName}`,
    'Transport: Streamable HTTP',
    `URL: ${url}`,
    `Header: Authorization: ${AUTHORIZATION}`,
    '',
    `Use your client's own way of adding MCP servers. In Claude Code that is: claude mcp add --transport http ${branding.integrationName} ${url} --header "Authorization: ${AUTHORIZATION}"`,
    'Keep the key out of shared or committed files.',
    'Once it is added, connect to it, list its tools and call list_servers to confirm it works.',
  ].join('\n');
}

export function snippet(client: Client, url: string, branding: Branding): string {
  switch (client) {
    case 'agent':
      return agentPrompt(url, branding);
    case 'claudeCode':
      return `claude mcp add --transport http ${branding.integrationName} ${url} --header "Authorization: ${AUTHORIZATION}"`;
    case 'cursor':
      return JSON.stringify(
        {
          mcpServers: {
            [branding.integrationName]: { url, headers: { Authorization: AUTHORIZATION } },
          },
        },
        null,
        2,
      );
    case 'vscode':
      return JSON.stringify(
        {
          servers: {
            [branding.integrationName]: {
              type: 'http',
              url,
              headers: { Authorization: AUTHORIZATION },
            },
          },
        },
        null,
        2,
      );
    case 'claudeDesktop':
      return JSON.stringify(
        {
          mcpServers: {
            [branding.integrationName]: {
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
