import { faShieldHalved } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useEffect, useState } from 'react';
import Alert from '@/elements/Alert.tsx';
import AccountContentContainer from '@/elements/containers/AccountContentContainer.tsx';
import getBranding from '../api/getBranding.ts';
import ClientSetupCard from '../elements/ClientSetupCard.tsx';
import EndpointCard from '../elements/EndpointCard.tsx';
import { type Branding, DEFAULT_BRANDING, endpointUrl } from '../lib/clients.ts';
import { useExtTranslations } from '../translations.ts';

export default function McpPage() {
  const { t } = useExtTranslations();
  const [branding, setBranding] = useState<Branding>(DEFAULT_BRANDING);
  const url = endpointUrl();

  useEffect(() => {
    getBranding()
      .then((settings) => setBranding(settings))
      .catch(() => setBranding(DEFAULT_BRANDING));
  }, []);

  return (
    <AccountContentContainer title={t('title', {})}>
      <Alert mb='md' icon={<FontAwesomeIcon icon={faShieldHalved} />} title={t('safety.title', {})}>
        {t('safety.description', {})}
      </Alert>
      <div className='grid grid-cols-1 xl:grid-cols-2 items-start gap-4'>
        <EndpointCard url={url} />
        <ClientSetupCard url={url} branding={branding} />
      </div>
    </AccountContentContainer>
  );
}
