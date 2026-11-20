import { axiosInstance } from '@/api/axios.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';
import { PACKAGE } from '../../lib/clients.ts';
import { type KeyAccess, keyAccessSchema } from '../../lib/schemas.ts';

export default async (): Promise<KeyAccess> => {
  const { data } = await axiosInstance.get(`/api/client/extensions/${PACKAGE}/restrictions`);
  return parseFromApi(keyAccessSchema, data);
};
