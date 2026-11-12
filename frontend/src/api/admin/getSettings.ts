import { axiosInstance } from '@/api/axios.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';
import { PACKAGE } from '../../lib/clients.ts';
import { type Settings, settingsSchema } from '../../lib/schemas.ts';

export default async (): Promise<Settings> => {
  const { data } = await axiosInstance.get(`/api/admin/extensions/${PACKAGE}/settings`);
  return parseFromApi(settingsSchema, data);
};
