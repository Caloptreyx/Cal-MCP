import { faTags } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useForm } from '@mantine/form';
import { zod4Resolver } from 'mantine-form-zod-resolver';
import { useEffect, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import Button from '@/elements/buttons/Button.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import ErrorState from '@/elements/feedback/ErrorState.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import Text from '@/elements/typography/Text.tsx';
import { useAdminCan } from '@/plugins/usePermissions.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import getSettings from '../api/admin/getSettings.ts';
import updateSettings from '../api/admin/updateSettings.ts';
import { DEFAULT_BRANDING } from '../lib/clients.ts';
import { type Settings, settingsSchema } from '../lib/schemas.ts';

export default function ConfigurationPage() {
  const { addToast } = useToast();
  const canManage = useAdminCan('extensions.manage');
  const [loading, setLoading] = useState(true);
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const form = useForm<Settings>({
    initialValues: DEFAULT_BRANDING,
    validateInputOnBlur: true,
    validate: zod4Resolver(settingsSchema),
  });

  const loadSettings = () => {
    setLoading(true);
    setLoadError(null);

    getSettings()
      .then((settings) => {
        form.setValues(settings);
        setLoaded(true);
      })
      .catch((err) => {
        setLoaded(false);
        setLoadError(httpErrorToHuman(err));
      })
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    if (canManage) {
      loadSettings();
    }
  }, [canManage]);

  const doSave = (values: Settings) => {
    if (!loaded) {
      return;
    }

    setSaving(true);

    updateSettings(values)
      .then(() => addToast('Settings saved.', 'success'))
      .catch((err) => addToast(httpErrorToHuman(err), 'error'))
      .finally(() => setSaving(false));
  };

  if (!canManage) {
    return (
      <TitleCard title='Branding' icon={<FontAwesomeIcon icon={faTags} />}>
        <Text size='sm' c='dimmed'>
          You need the extensions.manage permission to change these settings.
        </Text>
      </TitleCard>
    );
  }

  return (
    <TitleCard title='Branding' icon={<FontAwesomeIcon icon={faTags} />}>
      {loading ? (
        <Spinner.Centered />
      ) : loadError ? (
        <ErrorState error={loadError} onRetry={loadSettings} />
      ) : (
        <form onSubmit={form.onSubmit(doSave)}>
          <Stack>
            <Text size='sm' c='dimmed'>
              These values are published to every user in the MCP setup page and reported to MCP clients when they
              connect.
            </Text>

            <TextInput
              label='Integration name'
              description='Name clients use to register the MCP server, for example in a claude mcp add command or an mcpServers config. Letters, numbers, dots, dashes and underscores only.'
              placeholder='calagopus'
              {...form.getInputProps('integrationName')}
            />

            <TextInput
              label='Display title'
              description='Title MCP clients show for this server.'
              placeholder='Cal MCP'
              {...form.getInputProps('displayTitle')}
            />

            <TextInput
              label='Panel name'
              description='Panel name used in the assistant instructions and the setup prompt.'
              placeholder='Calagopus'
              {...form.getInputProps('panelName')}
            />

            <Button type='submit' loading={saving} disabled={saving || !loaded || !form.isValid()} className='w-fit!'>
              Save
            </Button>
          </Stack>
        </form>
      )}
    </TitleCard>
  );
}
