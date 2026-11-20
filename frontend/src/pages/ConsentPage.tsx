import { faCheck, faPlug, faTriangleExclamation, faXmark } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Radio } from '@mantine/core';
import { useEffect, useState } from 'react';
import { useLocation } from 'react-router';
import { httpErrorToHuman } from '@/api/axios.ts';
import Button from '@/elements/Button.tsx';
import Code from '@/elements/Code.tsx';
import AccountContentContainer from '@/elements/containers/AccountContentContainer.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import Group from '@/elements/Group.tsx';
import Stack from '@/elements/Stack.tsx';
import Text from '@/elements/Text.tsx';
import TitleCard from '@/elements/TitleCard.tsx';
import { decideConsent, getConsent } from '../api/oauth.ts';
import { type Consent, SCOPES, type Scope } from '../lib/oauth.ts';
import { useExtTranslations } from '../translations.ts';

type Request = NonNullable<Consent['consent']>;

/**
 * The OAuth authorization endpoint. MCP clients send the browser here; the signed-in user picks a
 * scope and approves or declines, and the browser goes back to the client's redirect URI.
 */
export default function ConsentPage() {
  const { t } = useExtTranslations();
  const { search } = useLocation();
  const query = search.startsWith('?') ? search.slice(1) : search;

  const [request, setRequest] = useState<Request | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [scope, setScope] = useState<Scope>('client');
  const [submitting, setSubmitting] = useState<'approve' | 'deny' | null>(null);

  useEffect(() => {
    setRequest(null);
    setError(null);

    getConsent(query)
      .then(({ redirect, consent }) => {
        if (redirect) {
          window.location.assign(redirect);
          return;
        }
        if (!consent) return;

        setRequest(consent);
        setScope(consent.requestedScope === 'full' && !consent.admin ? 'client' : consent.requestedScope);
      })
      .catch((err) => setError(httpErrorToHuman(err)));
  }, [query]);

  const decide = (approve: boolean) => {
    setSubmitting(approve ? 'approve' : 'deny');
    setError(null);

    decideConsent(query, approve, scope)
      .then((redirect) => window.location.assign(redirect))
      .catch((err) => {
        setError(httpErrorToHuman(err));
        setSubmitting(null);
      });
  };

  const scopes = SCOPES.filter((option) => option !== 'full' || request?.admin);

  return (
    <AccountContentContainer title={t('consent.title', {})}>
      {error && (
        <Alert
          mb='md'
          color='red'
          icon={<FontAwesomeIcon icon={faTriangleExclamation} />}
          title={t('consent.error', {})}
        >
          {error}
        </Alert>
      )}
      {!request ? (
        !error && <Spinner.Centered />
      ) : (
        <div className='max-w-2xl'>
          <TitleCard
            title={t('consent.heading', { client: request.clientName })}
            icon={<FontAwesomeIcon icon={faPlug} />}
          >
            <Stack>
              <Text size='sm'>{t('consent.description', { client: request.clientName })}</Text>
              <div>
                <Text size='sm' c='dimmed'>
                  {t('consent.redirect', { host: request.redirectHost })}
                </Text>
                <Code block className='whitespace-pre-wrap [overflow-wrap:anywhere]'>
                  {request.redirectUri}
                </Code>
              </div>
              {request.loopback && (
                <Alert color='yellow' icon={<FontAwesomeIcon icon={faTriangleExclamation} />}>
                  {t('consent.loopback', {})}
                </Alert>
              )}
              <Radio.Group label={t('consent.scope', {})} value={scope} onChange={(value) => setScope(value as Scope)}>
                <Stack mt='xs' gap='sm'>
                  {scopes.map((option) => (
                    <Radio
                      key={option}
                      value={option}
                      label={t(`scopes.${option}.label`, {})}
                      description={t(`scopes.${option}.description`, {})}
                    />
                  ))}
                </Stack>
              </Radio.Group>
              <Text size='sm' c='dimmed'>
                {t('consent.apiKey', {})}
              </Text>
              <Group justify='flex-end'>
                <Button
                  variant='default'
                  leftSection={<FontAwesomeIcon icon={faXmark} />}
                  disabled={submitting !== null}
                  loading={submitting === 'deny'}
                  onClick={() => decide(false)}
                >
                  {t('consent.deny', {})}
                </Button>
                <Button
                  leftSection={<FontAwesomeIcon icon={faCheck} />}
                  disabled={submitting !== null}
                  loading={submitting === 'approve'}
                  onClick={() => decide(true)}
                >
                  {t('consent.approve', {})}
                </Button>
              </Group>
            </Stack>
          </TitleCard>
        </div>
      )}
    </AccountContentContainer>
  );
}
