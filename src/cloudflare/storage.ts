import type { Storage } from '../nostr/wallet-registry.ts';

export class CloudflareStorage implements Storage {
  constructor(private storage: DurableObjectStorage) {}

  async get(key: string): Promise<unknown | null> {
    const val = await this.storage.get(key);
    return val !== undefined ? val : null;
  }

  async putBatch(entries: Record<string, unknown>): Promise<void> {
    await this.storage.put(entries);
  }

  async deleteBatch(keys: string[]): Promise<void> {
    if (keys.length === 0) return;
    await this.storage.delete(keys);
  }
}
