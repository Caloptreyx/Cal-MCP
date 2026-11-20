import { axiosInstance } from '@/api/axios.ts';
import { serializeForApi } from '@/lib/serialization/api-transform.ts';
import { PACKAGE } from '../../lib/clients.ts';
import { type Access, accessSchema } from '../../lib/schemas.ts';

export default async (apiKey: string, access: Access): Promise<void> => {
  await axiosInstance.put(
    `/api/client/extensions/${PACKAGE}/restrictions/${apiKey}`,
    serializeForApi(accessSchema, access),
  );
};
