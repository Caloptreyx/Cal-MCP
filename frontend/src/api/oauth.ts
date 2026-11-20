import { axiosInstance } from '@/api/axios.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';
import { PACKAGE } from '../lib/clients.ts';
import { type Consent, consentSchema, decisionSchema, type Grant, grantsSchema, type Scope } from '../lib/oauth.ts';

const BASE = `/api/client/extensions/${PACKAGE}/oauth`;

/** Validates the authorization request in `query` (the consent page's query string). */
export async function getConsent(query: string): Promise<Consent> {
  const { data } = await axiosInstance.get(`${BASE}/authorize?${query}`);
  return parseFromApi(consentSchema, data);
}

/** Approves or declines; resolves to the URL the browser has to go to next. */
export async function decideConsent(query: string, approve: boolean, scope: Scope): Promise<string> {
  const { data } = await axiosInstance.post(`${BASE}/authorize`, { query, approve, scope });
  return parseFromApi(decisionSchema, data).redirect;
}

export async function getGrants(): Promise<Grant[]> {
  const { data } = await axiosInstance.get(`${BASE}/grants`);
  return parseFromApi(grantsSchema, data).grants;
}

export async function revokeGrant(uuid: string): Promise<void> {
  await axiosInstance.delete(`${BASE}/grants/${uuid}`);
}
