import { handleRequest } from './router.ts';
import { CloudflareTransport } from './transport.ts';

export { CloudflareTransport };

export default {
  async fetch(request: Request, env: Record<string, unknown>): Promise<Response> {
    return handleRequest(request, env);
  },
};
