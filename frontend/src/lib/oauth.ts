import { z } from 'zod';

export const scopeSchema = z.enum(['read', 'client', 'full']);
export type Scope = z.infer<typeof scopeSchema>;

export const SCOPES: Scope[] = ['read', 'client', 'full'];

/** Either a consent request to show, or an error redirect the user can follow back to the client. */
export const consentSchema = z.object({
  redirect: z.string().nullable(),
  consent: z
    .object({
      clientName: z.string(),
      redirectUri: z.string(),
      redirectHost: z.string(),
      loopback: z.boolean(),
      requestedScope: scopeSchema,
      admin: z.boolean(),
    })
    .nullable(),
});
export type Consent = z.infer<typeof consentSchema>;

export const decisionSchema = z.object({ redirect: z.string() });

export const grantSchema = z.object({
  uuid: z.string(),
  clientName: z.string(),
  redirectUris: z.array(z.string()),
  scope: scopeSchema,
  created: z.string(),
  lastUsed: z.string().nullable(),
});
export type Grant = z.infer<typeof grantSchema>;

export const grantsSchema = z.object({ grants: z.array(grantSchema) });

/** The host part of a redirect URI, as the consent page shows it. */
export function redirectHost(uri: string): string {
  try {
    const url = new URL(uri);
    return url.host || `${url.protocol}`;
  } catch {
    return uri;
  }
}
