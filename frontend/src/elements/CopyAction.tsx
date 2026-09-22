import { faCheck, faCopy } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { CopyButton } from '@mantine/core';
import ActionIcon from '@/elements/ActionIcon.tsx';
import Tooltip from '@/elements/Tooltip.tsx';
import { useExtTranslations } from '../translations.ts';

export default function CopyAction({ value }: { value: string }) {
  const { t } = useExtTranslations();

  return (
    <CopyButton value={value}>
      {({ copied, copy }) => (
        <Tooltip label={copied ? t('copied', {}) : t('copy', {})}>
          <ActionIcon variant='subtle' color={copied ? 'green' : 'gray'} onClick={copy} aria-label={t('copy', {})}>
            <FontAwesomeIcon icon={copied ? faCheck : faCopy} />
          </ActionIcon>
        </Tooltip>
      )}
    </CopyButton>
  );
}
