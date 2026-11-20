import { faLink, faTrash } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useEffect, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import ActionIcon from '@/elements/ActionIcon.tsx';
import Badge from '@/elements/Badge.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import Group from '@/elements/Group.tsx';
import ConfirmationModal from '@/elements/modals/ConfirmationModal.tsx';
import Stack from '@/elements/Stack.tsx';
import Text from '@/elements/Text.tsx';
import FormattedTimestamp from '@/elements/time/FormattedTimestamp.tsx';
import TitleCard from '@/elements/TitleCard.tsx';
import Tooltip from '@/elements/Tooltip.tsx';
import { useToast } from '@/providers/ToastProvider.tsx';
import { getGrants, revokeGrant } from '../api/oauth.ts';
import { type Grant, redirectHost } from '../lib/oauth.ts';
import { useExtTranslations } from '../translations.ts';

/** OAuth clients the user approved, with a way to disconnect each. */
export default function ConnectedClientsCard() {
  const { t } = useExtTranslations();
  const { addToast } = useToast();
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<Grant | null>(null);

  const load = () => {
    setError(null);
    getGrants()
      .then(setGrants)
      .catch((err) => setError(httpErrorToHuman(err)));
  };

  useEffect(load, []);

  const doRevoke = async () => {
    if (!revoking) return;

    await revokeGrant(revoking.uuid)
      .then(() => {
        addToast(t('connected.revoked', { client: revoking.clientName }), 'success');
        setRevoking(null);
        load();
      })
      .catch((err) => addToast(httpErrorToHuman(err), 'error'));
  };

  return (
    <TitleCard title={t('connected.title', {})} icon={<FontAwesomeIcon icon={faLink} />}>
      <ConfirmationModal
        opened={revoking !== null}
        onClose={() => setRevoking(null)}
        title={t('connected.confirmTitle', {})}
        confirm={t('connected.revoke', {})}
        onConfirmed={doRevoke}
      >
        {t('connected.confirm', { client: revoking?.clientName ?? '' })}
      </ConfirmationModal>

      <Stack>
        <Text size='sm'>{t('connected.description', {})}</Text>
        {error ? (
          <Text size='sm' c='red'>
            {error}
          </Text>
        ) : grants === null ? (
          <Spinner.Centered />
        ) : grants.length === 0 ? (
          <Text size='sm' c='dimmed'>
            {t('connected.empty', {})}
          </Text>
        ) : (
          grants.map((grant) => (
            <Group key={grant.uuid} justify='space-between' wrap='nowrap'>
              <Stack gap={2}>
                <Group gap='xs'>
                  <Text fw={500}>{grant.clientName}</Text>
                  <Badge size='sm' variant='light'>
                    {t(`scopes.${grant.scope}.label`, {})}
                  </Badge>
                </Group>
                <Text size='xs' c='dimmed'>
                  {grant.redirectUris.map(redirectHost).join(', ')}
                </Text>
                <Text size='xs' c='dimmed'>
                  {t('connected.connected', {})} <FormattedTimestamp timestamp={grant.created} />
                  {' · '}
                  {t('connected.lastUsed', {})}{' '}
                  {grant.lastUsed ? <FormattedTimestamp timestamp={grant.lastUsed} /> : t('connected.never', {})}
                </Text>
              </Stack>
              <Tooltip label={t('connected.revoke', {})}>
                <ActionIcon
                  variant='subtle'
                  color='red'
                  onClick={() => setRevoking(grant)}
                  aria-label={t('connected.revoke', {})}
                >
                  <FontAwesomeIcon icon={faTrash} />
                </ActionIcon>
              </Tooltip>
            </Group>
          ))
        )}
      </Stack>
    </TitleCard>
  );
}
