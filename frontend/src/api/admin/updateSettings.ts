import { axiosInstance } from '@/api/axios.ts';
import { serializeForApi } from '@/lib/serialization/api-transform.ts';
import { PACKAGE } from '../../lib/clients.ts';
import { type Settings, settingsSchema } from '../../lib/schemas.ts';

export default async (data: Settings): Promise<void> => {
  await axiosInstance.put(`/api/admin/extensions/${PACKAGE}/settings`, serializeForApi(settingsSchema, data));
};
