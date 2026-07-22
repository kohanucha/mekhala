/* eslint-disable @typescript-eslint/require-await */

import { NwcClient, EncryptionMethod, type NwcUri, type WalletInfo, type Event, Tag, NostrEngine, DEFAULT_LIMITS } from '../nostr/index.ts';
import { NwcError } from './index.ts';

// ── Constants ──

export const TEST_WALLET_SK = '0101010101010101010101010101010101010101010101010101010101010101';
export const TEST_WALLET_PK = '1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f';
export const TEST_NWC_URI = 'nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101';

// ── Clock seam ──

let testTime = 1700000000;

export function testNow(): number {
  return testTime;
}

export function setTestTime(t: number): void {
  testTime = t;
}

// ── MockStorage ──

export class MockStorage {
  data = new Map<string, unknown>();
  failPutBatch = false;

  async get(key: string): Promise<unknown> {
    return this.data.get(key) ?? null;
  }

  async putBatch(entries: Record<string, unknown>): Promise<void> {
    if (this.failPutBatch) {
      throw new Error('mock storage unavailable');
    }
    for (const [k, v] of Object.entries(entries)) {
      this.data.set(k, v);
    }
  }

  async deleteBatch(keys: string[]): Promise<void> {
    for (const k of keys) {
      this.data.delete(k);
    }
  }

  snapshot(): Record<string, unknown> {
    const obj: Record<string, unknown> = {};
    for (const [k, v] of this.data) {
      obj[k] = v;
    }
    return obj;
  }
}

// ── Event factory ──

export function testEvent(id: string, pubkey: string, kind: number, tags: Tag[], createdAt: number): Event {
  return {
    id,
    pubkey,
    kind,
    tags,
    content: 'test',
    sig: 'sig',
    createdAt,
  };
}

// ── Hibernation simulator ──

export async function simulateHibernation(original: MockStorage): Promise<{ storage: MockStorage }> {
  const snapshot = original.snapshot();
  const newStorage = new MockStorage();
  await newStorage.putBatch(snapshot);
  return { storage: newStorage };
}

// ── Storage seed helpers ──

export async function seedSubscription(
  storage: MockStorage,
  connId: number,
  subId: string,
  pk: string,
  filters: unknown[],
): Promise<void> {
  const subs: Record<string, unknown> = {};
  subs[subId] = filters;
  await storage.putBatch({
    [`pk:${pk}`]: [connId],
    [`conn:${connId}`]: {
      subscriptions: subs,
      info_event: null,
    },
  });
}

// ── Engine factory ──

export function newTestEngine(): NostrEngine<MockStorage> {
  return new NostrEngine(new MockStorage(), DEFAULT_LIMITS, testNow);
}

// ── MockTransport ──

export class MockTransport {
  walletInfo: WalletInfo | null = null;
  walletUri: NwcUri | null = null;
  errorCode: string | null = null;

  static walletNotFound(): MockTransport {
    const t = new MockTransport();
    t.walletInfo = null;
    t.walletUri = null;
    return t;
  }

  async getWalletInfo(_pubkey: string): Promise<WalletInfo | null> {
    return this.walletInfo;
  }

  async executeNwcRpc(request: Event): Promise<Event> {
    if (!this.walletUri) throw NwcError.walletNotFound();

    const client = new NwcClient(this.walletUri);

    const isNip44 = request.tags.some(t => t.encryptionScheme() === 'nip44_v2');
    if (isNip44) {
      client.encryptionMethod = EncryptionMethod.Nip44;
    }

    await client.decrypt(request.content);

    const respPayload = this.errorCode
      ? { error: { code: this.errorCode, message: 'insufficient balance' } }
      : { result: { invoice: 'lnbc1test' } };

    const encrypted = await client.encrypt(respPayload);

    const tags: Tag[] = [
      Tag.p(client.myPubkey),
      Tag.e(request.id),
    ];
    if (isNip44) {
      tags.push(Tag.encryption('nip44_v2'));
    }

    return client.createEvent(23195, encrypted, tags);
  }
}
