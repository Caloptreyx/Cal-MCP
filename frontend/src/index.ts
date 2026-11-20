import { faRobot } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { createElement } from 'react';
import { Extension, type ExtensionContext } from 'shared';
import ConfigurationPage from './pages/ConfigurationPage.tsx';
import ConsentPage from './pages/ConsentPage.tsx';
import McpPage from './pages/McpPage.tsx';
import { getExtTranslations } from './translations.ts';

class DevCaloptreyxCalmcpExtension extends Extension {
  public cardIcon = createElement(FontAwesomeIcon, { icon: faRobot });
  public cardConfigurationPage: React.FC | null = ConfigurationPage;
  public cardComponent: React.FC | null = null;

  public initialize(ctx: ExtensionContext): void {
    ctx.extensionRegistry.routes.addAccountRoute({
      name: () => getExtTranslations().t('title', {}),
      icon: faRobot,
      path: '/mcp',
      element: McpPage,
    });
    // The OAuth authorization endpoint; unnamed so it stays out of the sidebar.
    ctx.extensionRegistry.routes.addAccountRoute({
      name: undefined,
      path: '/mcp/authorize',
      element: ConsentPage,
    });
  }
}

export default new DevCaloptreyxCalmcpExtension();
