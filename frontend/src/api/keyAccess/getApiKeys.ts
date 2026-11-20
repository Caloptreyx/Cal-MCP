import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';
import { type ApiKey, apiKeySchema } from '../../lib/schemas.ts';

// The panel's own helper changed its signature between releases, so the extension asks directly.
export default async (): Promise<ApiKey[]> => {
  const { data } = await axiosInstance.get('/api/client/account/api-keys', {
    params: { page: 1, per_page: 100 },
  });
  return parseFromApi(z.array(apiKeySchema), data.api_keys.data);
};
