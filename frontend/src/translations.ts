import { defineTranslations } from 'shared';

const translations = defineTranslations({
  items: {},
  translations: {
    title: 'MCP',
    description:
      'Connect AI assistants such as Claude, ChatGPT, Cursor or VS Code to this panel. They can use everything the panel API offers, limited to the permissions of the API key or connection you approve.',
    copy: 'Copy',
    copied: 'Copied',
    endpoint: {
      title: 'Endpoint',
      hint: 'Claude.ai and ChatGPT sign in with OAuth. Other clients send an API key as a Bearer token in the Authorization header.',
      apiKeys: 'Create an API key',
    },
    setup: {
      title: 'Set up your client',
      agent: 'Paste this into your AI agent, such as Claude Code, Cursor or Codex, and it adds the server itself.',
      web: 'Add this URL as a custom connector (Claude: Settings > Connectors; ChatGPT: Settings > Connectors). You approve it in the panel, no API key needed.',
      claudeCode: 'Run this in a terminal.',
      cursor: 'Add this to ~/.cursor/mcp.json.',
      vscode: 'Add this to .vscode/mcp.json in your project.',
      claudeDesktop: 'Add this to claude_desktop_config.json. It runs mcp-remote, which needs Node.js.',
      replace: 'Replace YOUR_API_KEY with your key.',
    },
    access: {
      title: 'Limit what each key can do',
      description:
        'These limits apply only when a key connects through MCP, and the server enforces them. Read-only allows only reading; client API only blocks /api/admin.',
      panel: 'Your administrator applies some of these limits to every connection, so they cannot be turned off here.',
      noKeys: 'You have no API keys yet.',
      readOnly: 'Read-only',
      clientOnly: 'Client API only',
      saved: 'Key limits saved.',
    },
    safety: {
      title: 'Keep the key scoped',
      description:
        'Give the key only the permissions the assistant needs. Everything it does is recorded under that key in your activity.',
    },
    scopes: {
      read: {
        label: 'Read only',
        description: 'View servers, files and console output. Nothing can be changed and the admin area is off limits.',
      },
      client: {
        label: 'Your servers',
        description: 'Everything you can do with your servers, but not the admin area.',
      },
      full: {
        label: 'Full access',
        description: 'Everything your account can do, including the admin area.',
      },
    },
    consent: {
      title: 'Connect an MCP client',
      heading: 'Connect {client}',
      description:
        '{client} wants to use this panel through MCP with your account. It can only do what your account is allowed to do, limited further by the access you pick here.',
      redirect: 'After you decide, your browser goes back to {host}:',
      loopback:
        'This client runs on your own computer (localhost). Only approve it if you started the connection yourself just now.',
      scope: 'Access',
      apiKey:
        'This adds an API key starting with "MCP" to your account. It cannot manage your API keys, SSH keys, security keys, sessions, email, password or two-factor setup. Disconnect the client at any time under Account > MCP.',
      approve: 'Approve',
      deny: 'Deny',
      error: 'This request cannot be completed',
      framed: 'This page does not work inside another page. Open it in its own browser tab.',
      invalid: 'The app sent a request that cannot be approved. Go back to {host} to see the error there.',
      return: 'Return to {host}',
    },
    connected: {
      title: 'Connected clients',
      description: 'Clients you approved through OAuth, such as Claude.ai or ChatGPT.',
      empty: 'No clients are connected.',
      connected: 'Connected',
      lastUsed: 'last used',
      never: 'never',
      revoke: 'Disconnect',
      confirmTitle: 'Disconnect client',
      confirm: '{client} loses access right away. Connecting it again needs a new approval.',
      revoked: '{client} was disconnected.',
    },
  },
});

export const useExtTranslations = translations.useTranslations.bind(translations);
export const getExtTranslations = translations.getTranslations.bind(translations);
export default translations;
