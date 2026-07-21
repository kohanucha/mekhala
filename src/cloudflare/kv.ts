import type { UserStore } from '../common/index.ts';

export class CloudflareKvStore implements UserStore {
  constructor(private kv: KVNamespace) {}

  async getNwcUri(username: string): Promise<string | null> {
    return this.kv.get(username);
  }
}

