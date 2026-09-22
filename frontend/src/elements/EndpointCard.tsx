import { faKey, faPlug } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useNavigate } from 'react-router';
import Button from '@/elements/Button.tsx';
import Code from '@/elements/Code.tsx';
import Group from '@/elements/Group.tsx';
import Stack from '@/elements/Stack.tsx';
import Text from '@/elements/Text.tsx';
import TitleCard from '@/elements/TitleCard.tsx';
import { useExtTranslations } from '../translations.ts';
import CopyAction from './CopyAction.tsx';

export default function EndpointCard({ url }: { url: string }) {
  const { t } = useExtTranslations();
  const navigate = useNavigate();

  return (
    <TitleCard
      title={t('endpoint.title', {})}
      icon={<FontAwesomeIcon icon={faPlug} />}
      rightSection={<CopyAction value={url} />}
    >
      <Stack>
        <Text size='sm'>{t('description', {})}</Text>
        <Code block className='break-all whitespace-pre-wrap'>
          {url}
        </Code>
        <Group justify='space-between'>
          <Text size='sm' c='dimmed'>
            {t('endpoint.hint', {})}
          </Text>
          <Button
            variant='light'
            leftSection={<FontAwesomeIcon icon={faKey} />}
            onClick={() => navigate('/account/api-keys')}
          >
            {t('endpoint.apiKeys', {})}
          </Button>
        </Group>
      </Stack>
    </TitleCard>
  );
}
