import { defineTranslations } from 'shared';

const translations = defineTranslations({
  items: {},
  translations: {
    title: 'MCP',
    description:
      'Connect AI assistants such as Claude, Cursor or VS Code to this panel. They can use everything the panel API offers, limited to the permissions of the API key you connect with.',
    copy: 'Copy',
    copied: 'Copied',
    endpoint: {
      title: 'Endpoint',
      hint: 'Send an API key as a Bearer token in the Authorization header.',
      apiKeys: 'Create an API key',
    },
    setup: {
      title: 'Set up your client',
      agent: 'Paste this into your AI agent, such as Claude Code, Cursor or Codex, and it adds the server itself.',
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
  },
});

export const useExtTranslations = translations.useTranslations.bind(translations);
export const getExtTranslations = translations.getTranslations.bind(translations);
export default translations;
