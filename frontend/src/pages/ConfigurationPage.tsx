import { faTags } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useForm } from '@mantine/form';
import { zod4Resolver } from 'mantine-form-zod-resolver';
import { useEffect, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import Button from '@/elements/Button.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Stack from '@/elements/Stack.tsx';
import Text from '@/elements/Text.tsx';
import TitleCard from '@/elements/TitleCard.tsx';
import { useToast } from '@/providers/ToastProvider.tsx';
import getSettings from '../api/admin/getSettings.ts';
import updateSettings from '../api/admin/updateSettings.ts';
import { DEFAULT_BRANDING } from '../lib/clients.ts';
import { type Settings, settingsSchema } from '../lib/schemas.ts';

export default function ConfigurationPage() {
  const { addToast } = useToast();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const form = useForm<Settings>({
    initialValues: DEFAULT_BRANDING,
    validateInputOnBlur: true,
    validate: zod4Resolver(settingsSchema),
  });

  useEffect(() => {
    getSettings()
      .then((settings) => form.setValues(settings))
      .catch((err) => addToast(httpErrorToHuman(err), 'error'))
      .finally(() => setLoading(false));
  }, []);

  const doSave = (values: Settings) => {
    setSaving(true);

    updateSettings(values)
      .then(() => addToast('Settings saved.', 'success'))
      .catch((err) => addToast(httpErrorToHuman(err), 'error'))
      .finally(() => setSaving(false));
  };

  return (
    <TitleCard title='Branding' icon={<FontAwesomeIcon icon={faTags} />}>
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
            disabled={loading}
            {...form.getInputProps('integrationName')}
          />

          <TextInput
            label='Display title'
            description='Title MCP clients show for this server.'
            placeholder='Cal MCP'
            disabled={loading}
            {...form.getInputProps('displayTitle')}
          />

          <TextInput
            label='Panel name'
            description='Panel name used in the assistant instructions and the setup prompt.'
            placeholder='Calagopus'
            disabled={loading}
            {...form.getInputProps('panelName')}
          />

          <Button type='submit' loading={saving} disabled={loading || !form.isValid()} className='w-fit!'>
            Save
          </Button>
        </Stack>
      </form>
    </TitleCard>
  );
}
