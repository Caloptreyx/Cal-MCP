import { z } from 'zod';

const hasControlCharacters = (value: string) =>
  Array.from(value).some((character) => {
    const code = character.codePointAt(0) ?? 0;

    return code <= 0x1f || (code >= 0x7f && code <= 0x9f);
  });

const displayTextSchema = (max: number) =>
  z
    .string()
    .trim()
    .refine((value) => value.length > 0, 'Must not be blank.')
    .refine((value) => Array.from(value).length <= max, `Must be at most ${max} characters.`)
    .refine((value) => !hasControlCharacters(value), 'Must not contain control characters.');

export const settingsSchema = z.object({
  integrationName: z
    .string()
    .trim()
    .min(1)
    .max(64)
    .regex(
      /^[A-Za-z0-9][A-Za-z0-9._-]*$/,
      'Use letters, numbers, dots, dashes and underscores, starting with a letter or number.',
    ),
  displayTitle: displayTextSchema(96),
  panelName: displayTextSchema(64),
});

export type Settings = z.infer<typeof settingsSchema>;
