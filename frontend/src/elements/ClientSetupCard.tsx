import { faTerminal } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useMediaQuery } from '@mantine/hooks';
import { useState } from 'react';
import Code from '@/elements/Code.tsx';
import SegmentedControl from '@/elements/SegmentedControl.tsx';
import Stack from '@/elements/Stack.tsx';
import Text from '@/elements/Text.tsx';
import TitleCard from '@/elements/TitleCard.tsx';
import { CLIENTS, type Client, snippet } from '../lib/clients.ts';
import { useExtTranslations } from '../translations.ts';
import CopyAction from './CopyAction.tsx';

export default function ClientSetupCard({ url }: { url: string }) {
  const { t } = useExtTranslations();
  const [client, setClient] = useState<Client>('agent');
  const narrow = useMediaQuery('(max-width: 40em)');
  const text = snippet(client, url);

  return (
    <TitleCard
      title={t('setup.title', {})}
      icon={<FontAwesomeIcon icon={faTerminal} />}
      rightSection={<CopyAction value={text} />}
    >
      <Stack>
        <SegmentedControl
          fullWidth
          orientation={narrow ? 'vertical' : 'horizontal'}
          data={CLIENTS}
          value={client}
          onChange={(value) => setClient(value as Client)}
        />
        <Text size='sm'>
          {t(`setup.${client}`, {})} {t('setup.replace', {})}
        </Text>
        <Code block className='whitespace-pre-wrap [overflow-wrap:anywhere]'>
          {text}
        </Code>
      </Stack>
    </TitleCard>
  );
}
