import { faKey } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useEffect, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import ErrorState from '@/elements/feedback/ErrorState.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import Switch from '@/elements/input/Switch.tsx';
import Group from '@/elements/layout/Group.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import Text from '@/elements/typography/Text.tsx';
import { useToast } from '@/providers/ToastProvider.tsx';
import type { Access, ApiKey } from '../lib/schemas.ts';
import getApiKeys from '../api/keyAccess/getApiKeys.ts';
import getKeyAccess from '../api/keyAccess/getKeyAccess.ts';
import updateKeyAccess from '../api/keyAccess/updateKeyAccess.ts';
import { useExtTranslations } from '../translations.ts';

const NO_LIMITS: Access = { readOnly: false, clientOnly: false };

export default function KeyAccessCard() {
  const { t } = useExtTranslations();
  const { addToast } = useToast();
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [panel, setPanel] = useState<Access>(NO_LIMITS);
  const [chosen, setChosen] = useState<Record<string, Access>>({});
  const [saving, setSaving] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const load = () => {
    setLoading(true);
    setLoadError(null);

    Promise.all([getApiKeys(), getKeyAccess()])
      .then(([apiKeys, access]) => {
        setKeys(apiKeys);
        setPanel(access.panel);
        setChosen(
          Object.fromEntries(
            access.keys.map((key) => [key.apiKey, { readOnly: key.readOnly, clientOnly: key.clientOnly }]),
          ),
        );
      })
      .catch((err) => setLoadError(httpErrorToHuman(err)))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    load();
  }, []);

  const change = (apiKey: string, access: Access) => {
    setSaving(apiKey);

    updateKeyAccess(apiKey, access)
      .then(() => {
        setChosen((current) => ({ ...current, [apiKey]: access }));
        addToast(t('access.saved', {}), 'success');
      })
      .catch((err) => addToast(httpErrorToHuman(err), 'error'))
      .finally(() => setSaving(null));
  };

  return (
    <TitleCard title={t('access.title', {})} icon={<FontAwesomeIcon icon={faKey} />}>
      {loading ? (
        <Spinner.Centered />
      ) : loadError ? (
        <ErrorState error={loadError} onRetry={load} />
      ) : (
        <Stack>
          <Text size='sm'>{t('access.description', {})}</Text>
          {(panel.readOnly || panel.clientOnly) && (
            <Text size='sm' c='dimmed'>
              {t('access.panel', {})}
            </Text>
          )}
          {keys.length === 0 ? (
            <Text size='sm' c='dimmed'>
              {t('access.noKeys', {})}
            </Text>
          ) : (
            keys.map((key) => {
              const access = chosen[key.uuid] ?? NO_LIMITS;

              return (
                <Group key={key.uuid} justify='space-between'>
                  <Text size='sm'>
                    {key.name} <Text span c='dimmed'>{`(${key.keyStart}…)`}</Text>
                  </Text>
                  <Group>
                    <Switch
                      label={t('access.readOnly', {})}
                      checked={panel.readOnly || access.readOnly}
                      disabled={panel.readOnly || saving === key.uuid}
                      onChange={(event) => change(key.uuid, { ...access, readOnly: event.currentTarget.checked })}
                    />
                    <Switch
                      label={t('access.clientOnly', {})}
                      checked={panel.clientOnly || access.clientOnly}
                      disabled={panel.clientOnly || saving === key.uuid}
                      onChange={(event) => change(key.uuid, { ...access, clientOnly: event.currentTarget.checked })}
                    />
                  </Group>
                </Group>
              );
            })
          )}
        </Stack>
      )}
    </TitleCard>
  );
}
