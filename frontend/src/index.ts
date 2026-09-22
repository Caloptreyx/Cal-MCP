import { faRobot } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { createElement } from 'react';
import { Extension, type ExtensionContext } from 'shared';
import McpPage from './pages/McpPage.tsx';
import { getExtTranslations } from './translations.ts';

class DevCaloptreyxCalmcpExtension extends Extension {
  public cardIcon = createElement(FontAwesomeIcon, { icon: faRobot });
  public cardConfigurationPage: React.FC | null = null;
  public cardComponent: React.FC | null = null;

  public initialize(ctx: ExtensionContext): void {
    ctx.extensionRegistry.routes.addAccountRoute({
      name: () => getExtTranslations().t('title', {}),
      icon: faRobot,
      path: '/mcp',
      element: McpPage,
    });
  }
}

export default new DevCaloptreyxCalmcpExtension();
