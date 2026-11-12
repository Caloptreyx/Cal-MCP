import { z } from 'zod';

export const settingsSchema = z.object({
  integrationName: z
    .string()
    .min(1)
    .max(64)
    .regex(
      /^[A-Za-z0-9][A-Za-z0-9._-]*$/,
      'Use letters, numbers, dots, dashes and underscores, starting with a letter or number.',
    ),
  displayTitle: z
    .string()
    .min(1)
    .max(96)
    .refine((value) => value.trim().length > 0, 'Must not be blank.'),
  panelName: z
    .string()
    .min(1)
    .max(64)
    .refine((value) => value.trim().length > 0, 'Must not be blank.'),
});

export type Settings = z.infer<typeof settingsSchema>;
