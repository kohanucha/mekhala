import { describe, it, expect } from 'vitest';
import { LnAddressHandler } from './handler.ts';
import type { UserStore } from '../common/mod.ts';

class MockUserStore implements UserStore {
  constructor(private uris: Record<string, string>) {}

  async getNwcUri(username: string): Promise<string | null> {
    const uri = this.uris[username];
    return uri !== undefined ? uri : null;
  }
}

describe('LnAddressHandler', () => {
  it('looks up existing user', async () => {
    const store = new MockUserStore({
      alice: 'nostr+walletconnect://pk?secret=s&relay=wss%3A%2F%2Frelay.com',
    });
    const handler = new LnAddressHandler(store);
    const result = await handler.lookupUser('alice');
    expect(result).not.toBeNull();
    expect(result).toContain('nostr+walletconnect');
  });

  it('returns null for unknown user', async () => {
    const store = new MockUserStore({});
    const handler = new LnAddressHandler(store);
    const result = await handler.lookupUser('nobody');
    expect(result).toBeNull();
  });

  it('returns empty string for user with empty URI', async () => {
    const store = new MockUserStore({ bob: '' });
    const handler = new LnAddressHandler(store);
    const result = await handler.lookupUser('bob');
    expect(result).toBe('');
  });
});
