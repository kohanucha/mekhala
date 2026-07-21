import { describe, it, expect } from 'vitest';
import { MockStorage, seedSubscription } from '../common/test-helpers.ts';
import { DEFAULT_LIMITS, createLimits } from './limits.ts';
import { WalletRegistry } from './wallet-registry.ts';
import type { Event } from './event.ts';
import type { Filter } from './filter.ts';
import { Tag } from './tag.ts';

function hasSubscription(registry: WalletRegistry<MockStorage>, connId: number, subId: string): boolean {
  return registry.getConnSubscriptions(connId).has(subId);
}

function makeEvent(overrides: Partial<Event> = {}): Event {
  return {
    id: 'e1',
    pubkey: 'alice',
    createdAt: 1000,
    kind: 23194,
    tags: [],
    content: '',
    sig: 'sig',
    ...overrides,
  };
}

describe('WalletRegistry', () => {

  describe('subscription persistence', () => {
    it('stores subscription state in storage', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      const filters: Filter[] = [{ authors: ['alice'] }];
      await registry.subscribe(1, 'sub1', filters);

      expect(storage.data.has('conn:1')).toBe(true);
      expect(storage.data.has('pk:alice')).toBe(true);
    });
  });

  describe('hibernation contract', () => {
    function simulateHibernation(original: MockStorage): WalletRegistry<MockStorage> {
      const newStorage = new MockStorage();
      for (const [k, v] of original.data) {
        newStorage.data.set(k, v);
      }
      return new WalletRegistry(newStorage, DEFAULT_LIMITS);
    }

    it('produces WakeUp and Send after hibernation', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]);

      const registry2 = simulateHibernation(storage);

      const event = makeEvent();
      const responses = await registry2.matchEvent(event);
      expect(responses).toContainEqual({ kind: 'wakeUp', connectionId: 1 });
      expect(responses).toContainEqual({ kind: 'send', recipientId: 1, subId: 'sub1' });
    });

    it('info event survives hibernation', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]);

      const info = makeEvent({
        id: 'info1',
        kind: 13194,
        content: 'wallet info',
      });
      await registry.cacheInfo(info);

      const registry2 = simulateHibernation(storage);

      const retrieved = await registry2.getInfo('alice');
      expect(retrieved).not.toBeNull();
      expect(retrieved!.id).toBe('info1');

      const event = makeEvent();
      const responses = await registry2.matchEvent(event);
      expect(responses).toContainEqual({ kind: 'wakeUp', connectionId: 1 });
      expect(responses).toContainEqual({ kind: 'send', recipientId: 1, subId: 'sub1' });
    });

    it('unsubscribe and re-subscribe updates correctly after hibernation', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]);
      await registry.unsubscribe(1, 'sub1');
      await registry.subscribe(1, 'sub1', [{ authors: ['bob'] }]);

      const registry2 = simulateHibernation(storage);

      const aliceEvent = makeEvent({ pubkey: 'alice', id: 'e1' });
      const aliceResponses = await registry2.matchEvent(aliceEvent);
      expect(aliceResponses).not.toContainEqual({ kind: 'send', recipientId: 1, subId: 'sub1' });

      const bobEvent = makeEvent({ pubkey: 'bob', id: 'e2' });
      const bobResponses = await registry2.matchEvent(bobEvent);
      expect(bobResponses).toContainEqual({ kind: 'wakeUp', connectionId: 1 });
      expect(bobResponses).toContainEqual({ kind: 'send', recipientId: 1, subId: 'sub1' });
    });
  });

  describe('event matching', () => {
    it('routes events to matching subscriptions', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      const walletPk = 'wallet_pk';
      await registry.subscribe(1, 'sub1', [{ pTags: [walletPk] }]);

      const event = makeEvent({
        id: 'event1',
        pubkey: 'app_pk',
        tags: [Tag.p(walletPk)],
        content: 'test',
      });
      const matches = await registry.matchEvent(event);
      expect(matches).toContainEqual({ kind: 'wakeUp', connectionId: 1 });
      expect(matches).toContainEqual({ kind: 'send', recipientId: 1, subId: 'sub1' });
    });

    it('lazy loads from storage on match', async () => {
      const storage = new MockStorage();
      const walletPk = 'hibernated_pk';
      const connId = 42;

      await seedSubscription(storage, connId, 'sub1', walletPk, [{ pTags: [walletPk] }]);

      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      const event = makeEvent({
        id: 'event1',
        pubkey: 'app_pk',
        tags: [Tag.p(walletPk)],
        content: 'test',
      });

      const responses = await registry.matchEvent(event);
      expect(responses).toContainEqual({ kind: 'wakeUp', connectionId: connId });
      expect(responses).toContainEqual({ kind: 'send', recipientId: connId, subId: 'sub1' });
    });

    it('groups matching connections by latest ID', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]);
      await registry.subscribe(2, 'sub1', [{ authors: ['alice'] }]);

      const event = makeEvent({ id: 'id', kind: 1 });
      const responses = await registry.matchEvent(event);

      const sends = responses.filter(
        (r): r is { kind: 'send'; recipientId: number; subId: string } =>
          r.kind === 'send' && r.subId === 'sub1',
      );
      expect(sends.length).toBe(1);
      expect(sends[0].recipientId).toBe(2);
    });
  });

  describe('info event caching', () => {
    it('caches and retrieves info events', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      const event = makeEvent({ id: 'id1', kind: 13194, sig: 'sig1' });
      await registry.cacheInfo(event);

      const stored = await registry.getInfo('alice');
      expect(stored).not.toBeNull();
      expect(stored!.id).toBe('id1');
    });

    it('indexes info events by ID', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      const event = makeEvent({ id: 'id1', kind: 13194, sig: 'sig1' });
      await registry.cacheInfo(event);

      expect(registry.findInfoPubkeyById('id1')).toBe('alice');
      expect(registry.findInfoPubkeyById('nonexistent')).toBeNull();
    });

    it('deletes info events', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      const event = makeEvent({ id: 'id1', kind: 13194, sig: 'sig1' });
      await registry.cacheInfo(event);

      expect(await registry.getInfo('alice')).not.toBeNull();
      expect(registry.findInfoPubkeyById('id1')).toBe('alice');

      await registry.deleteInfo('alice');

      expect(await registry.getInfo('alice')).toBeNull();
      expect(registry.findInfoPubkeyById('id1')).toBeNull();
      expect(storage.data.has('info:alice')).toBe(false);
    });

    it('preserves subscriptions when deleting info', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]);

      const event = makeEvent({ id: 'id1', kind: 13194, sig: 'sig1' });
      await registry.cacheInfo(event);

      await registry.deleteInfo('alice');

      expect(await registry.getInfo('alice')).toBeNull();
      expect(hasSubscription(registry, 1, 'sub1')).toBe(true);
    });

    it('deleteInfo for nonexistent pubkey is a no-op', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await registry.deleteInfo('nobody');
      expect(await registry.getInfo('nobody')).toBeNull();
    });
  });

  describe('sync and terminate', () => {
    it('sync persists subscription state', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]);

      expect(storage.data.has('conn:1')).toBe(true);
      expect(storage.data.has('pk:alice')).toBe(true);
    });

    it('terminate removes all state', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]);
      await registry.onTerminate(1);

      expect(storage.data.has('conn:1')).toBe(false);
      expect(storage.data.has('pk:alice')).toBe(false);
      expect(hasSubscription(registry, 1, 'sub1')).toBe(false);
    });

    it('lazy deletion removes stale pubkey entries', async () => {
      const storage = new MockStorage();
      await storage.putBatch({ 'pk:stale': [99] });

      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);
      const result = await registry.loadByPubkey('stale');
      expect(result).toEqual([]);
      expect(storage.data.has('pk:stale')).toBe(false);
    });
  });

  describe('limits and error handling', () => {
    it('rejects subscriptions exceeding the limit', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, createLimits(65536, 0));

      await expect(
        registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]),
      ).rejects.toThrow('too many subscriptions');
    });

    it('rejects subscribe on storage failure', async () => {
      const storage = new MockStorage();
      storage.failPutBatch = true;
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await expect(
        registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]),
      ).rejects.toThrow('persist failed');
    });

    it('unsubscribe is graceful on storage failure', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      await registry.subscribe(1, 'sub1', [{ authors: ['alice'] }]);
      await registry.subscribe(1, 'sub2', [{ authors: ['alice'] }]);

      storage.failPutBatch = true;

      await registry.unsubscribe(1, 'sub1');

      expect(hasSubscription(registry, 1, 'sub1')).toBe(false);
      expect(hasSubscription(registry, 1, 'sub2')).toBe(true);
    });
  });

  describe('pk list edge cases', () => {
    it('reads pk list stored as a number', async () => {
      const storage = new MockStorage();
      await storage.putBatch({
        'pk:single': 42,
        'conn:42': {
          subscriptions: { sub1: [{ '#p': ['single'] } as unknown as Filter] },
          info_event: null,
        },
      });

      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);
      const ids = await registry.loadByPubkey('single');
      expect(ids).toContain(42);
    });

    it('handles unknown pk list value type', async () => {
      const storage = new MockStorage();
      await storage.putBatch({ 'pk:weird': 'string_value' });

      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);
      const ids = await registry.loadByPubkey('weird');
      expect(ids).toEqual([]);
    });
  });

  describe('two connections same pubkey', () => {
    it('routes events to correct connections', async () => {
      const storage = new MockStorage();
      const registry = new WalletRegistry(storage, DEFAULT_LIMITS);

      const sharedPk = 'shared_pk';

      await registry.subscribe(1, 'wallet_sub', [{ kinds: [23194], pTags: [sharedPk] }]);
      await registry.subscribe(2, 'app_sub', [{ kinds: [23195], authors: [sharedPk] }]);

      // Check pk entry contains both connections
      const pkVal = storage.data.get(`pk:${sharedPk}`);
      expect(Array.isArray(pkVal)).toBe(true);
      expect(pkVal).toContain(1);
      expect(pkVal).toContain(2);

      // Snapshot and create new registry
      const snapshot = new Map(storage.data);
      {
        const storage2 = new MockStorage();
        for (const [k, v] of snapshot) storage2.data.set(k, v);
        const registry2 = new WalletRegistry(storage2, DEFAULT_LIMITS);

        const reqEvent = makeEvent({
          id: 'req1',
          pubkey: 'app_pk',
          tags: [Tag.p(sharedPk)],
          content: 'pay invoice',
        });
        const responses = await registry2.matchEvent(reqEvent);
        expect(responses).toContainEqual({ kind: 'wakeUp', connectionId: 1 });
        expect(responses).toContainEqual({ kind: 'send', recipientId: 1, subId: 'wallet_sub' });
        expect(responses).not.toContainEqual({ kind: 'send', recipientId: 2, subId: 'app_sub' });
      }

      {
        const storage3 = new MockStorage();
        for (const [k, v] of snapshot) storage3.data.set(k, v);
        const registry3 = new WalletRegistry(storage3, DEFAULT_LIMITS);

        const respEvent = makeEvent({
          id: 'resp1',
          pubkey: sharedPk,
          kind: 23195,
          tags: [Tag.p('app_pk')],
          content: 'paid',
        });
        const responses2 = await registry3.matchEvent(respEvent);
        expect(responses2).toContainEqual({ kind: 'wakeUp', connectionId: 2 });
        expect(responses2).toContainEqual({ kind: 'send', recipientId: 2, subId: 'app_sub' });
        expect(responses2).not.toContainEqual({ kind: 'send', recipientId: 1, subId: 'wallet_sub' });
      }
    });
  });
});
