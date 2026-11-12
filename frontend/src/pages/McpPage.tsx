import { faShieldHalved } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useEffect, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import AccountContentContainer from '@/elements/containers/AccountContentContainer.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import ErrorState from '@/elements/feedback/ErrorState.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import getBranding from '../api/getBranding.ts';
import ClientSetupCard from '../elements/ClientSetupCard.tsx';
import EndpointCard from '../elements/EndpointCard.tsx';
import { type Branding, endpointUrl } from '../lib/clients.ts';
import { useExtTranslations } from '../translations.ts';

export default function McpPage() {
  const { t } = useExtTranslations();
  const [branding, setBranding] = useState<Branding | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const url = endpointUrl();

  const loadBranding = () => {
    setLoading(true);
    setLoadError(null);

    getBranding()
      .then((settings) => setBranding(settings))
      .catch((err) => setLoadError(httpErrorToHuman(err)))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    loadBranding();
  }, []);

  return (
    <AccountContentContainer title={t('title', {})}>
      <Alert mb='md' icon={<FontAwesomeIcon icon={faShieldHalved} />} title={t('safety.title', {})}>
        {t('safety.description', {})}
      </Alert>
      {loading ? (
        <Spinner.Centered />
      ) : loadError || !branding ? (
        <ErrorState error={loadError ?? 'Could not load branding.'} onRetry={loadBranding} />
      ) : (
        <div className='grid grid-cols-1 xl:grid-cols-2 items-start gap-4'>
          <EndpointCard url={url} />
          <ClientSetupCard url={url} branding={branding} />
        </div>
      )}
    </AccountContentContainer>
  );
}
