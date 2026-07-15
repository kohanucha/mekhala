import { describe, it, expect } from 'vitest';
import { MockStorage, newTestEngine, testNow, setTestTime, seedSubscription } from '../common/test_helpers.ts';
import { DEFAULT_LIMITS } from './limits.ts';
import { NostrEngine } from './engine.ts';
import type { EngineResponse } from './engine.ts';
import type { Event } from './event.ts';
import type { ClientMessage } from './nip01.ts';
import { Tag } from './tag.ts';
import { NwcClient } from './nip47.ts';

function makeEvent(overrides: Partial<Event> = {}): Event {
  return {
    id: 'id1',
    pubkey: 'pk1',
    createdAt: 1000,
    kind: 13194,
    tags: [],
    content: '',
    sig: 'sig1',
    ...overrides,
  };
}

function hasResponse(
  responses: EngineResponse[],
  predicate: (r: EngineResponse) => boolean,
): boolean {
  return responses.some(predicate);
}

function findOk(responses: EngineResponse[]): { id: string; ok: boolean; message: string } | null {
  for (const r of responses) {
    if (r.kind === 'send' && r.message.type === 'OK') {
      return r.message;
    }
  }
  return null;
}

describe('NostrEngine', () => {
  describe('REQ and EOSE', () => {
    it('handles REQ and returns EOSE', async () => {
      const engine = newTestEngine();
      await engine.onConnect(1);

      const msg: ClientMessage = {
        type: 'REQ',
        subscriptionId: 'sub1',
        filters: [{ kinds: [23194], authors: ['pk1'] }],
      };
      const responses = await engine.handleTyped(1, msg);

      expect(hasResponse(responses, r =>
        r.kind === 'send' && r.message.type === 'EOSE',
      )).toBe(true);
    });
  });

  describe('info event routing', () => {
    it('caches and retrieves wallet info', async () => {
      const engine = newTestEngine();
      await engine.onConnect(1);

      const event = makeEvent({ kind: 13194 });
      await engine['processInfoEvent'](event);

      const info = await engine.getWalletInfo('pk1');
      expect(info).not.toBeNull();
    });

    it('returns none for unknown pubkey', async () => {
      const engine = newTestEngine();
      const info = await engine.getWalletInfo('pk1');
      expect(info).toBeNull();
    });

    it('parses encryption tags from info event', async () => {
      const engine = newTestEngine();
      const event = makeEvent({
        kind: 13194,
        tags: [Tag.encryption('nip44_v2 nip04')],
      });
      await engine['processInfoEvent'](event);

      const info = await engine.getWalletInfo('pk1');
      expect(info).not.toBeNull();
      expect(info!.encryptionAlgorithms.length).toBeGreaterThanOrEqual(2);
    });

    it('defaults to NIP-04 when no encryption tag', async () => {
      const engine = newTestEngine();
      const event = makeEvent({ kind: 13194, tags: [] });
      await engine['processInfoEvent'](event);

      const info = await engine.getWalletInfo('pk1');
      expect(info).not.toBeNull();
      expect(info!.encryptionAlgorithms).toEqual(['Nip04']);
    });
  });

  describe('event lifecycle', () => {
    it('runs REQ → EVENT → OK → routing', async () => {
      setTestTime(testNow());
      const engine = newTestEngine();
      await engine.onConnect(1);

      const walletPk = '1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f';
      await engine['processReq'](1, 'sub1', [{ kinds: [23194], pTags: [walletPk] }]);

      const bridgeSk = '0202020202020202020202020202020202020202020202020202020202020202';
      const bridgeClient = new NwcClient(
        { walletPubkey: walletPk, secret: bridgeSk },
        () => testNow(),
      );

      const bridgeId = 100;
      const msg: ClientMessage = {
        type: 'REQ',
        subscriptionId: 'sub_bridge',
        filters: [{ kinds: [23194], pTags: [bridgeClient.myPubkey] }],
      };
      const bridgeResponses = await engine.handleTyped(bridgeId, msg);
      expect(hasResponse(bridgeResponses, r =>
        r.kind === 'send' && r.message.type === 'EOSE',
      )).toBe(true);

      const { event: bridgeEvent } = await bridgeClient.createRequestEvent('make_invoice', {}, []);

      const eventMsg: ClientMessage = { type: 'EVENT', event: bridgeEvent };
      const responses = await engine.handleTyped(bridgeId, eventMsg);

      // Event should be routed to connection 1
      expect(hasResponse(responses, r =>
        r.kind === 'send' && r.recipientId === 1 && r.message.type === 'EVENT',
      )).toBe(true);

      // Should get OK: true for the event
      expect(hasResponse(responses, r =>
        r.kind === 'send' && r.recipientId === bridgeId && r.message.type === 'OK' && r.message.ok === true,
      )).toBe(true);
    });
  });

  describe('virtual connection lifecycle', () => {
    it('routes events and respects close', async () => {
      setTestTime(testNow());
      const engine = newTestEngine();
      await engine.onConnect(1);

      const id = 100;
      await engine['processReq'](id, 'sub1', [{ authors: ['alice'], kinds: [23194] }]);

      const event = makeEvent({
        id: 'event1',
        pubkey: 'alice',
        createdAt: testNow(),
        kind: 23194,
        content: 'test',
        sig: 'sig',
      });

      // Don't use handleTyped here — it would fail verification (bad sig).
      // Use processEvent directly like the Rust test
      const responses = await engine['processEvent'](id, event);
      expect(hasResponse(responses, r =>
        r.kind === 'send' && r.recipientId === id && r.message.type === 'EVENT',
      )).toBe(true);

      await engine.processClose(id, 'sub1');

      const responsesAfter = await engine['processEvent'](2, {
        ...event,
        id: 'event2',
      });
      expect(hasResponse(responsesAfter, r =>
        r.kind === 'send' && r.recipientId === id,
      )).toBe(false);
    });
  });

  describe('wake-up logic', () => {
    it('produces WakeUp and Send for hibernated connections', async () => {
      const id = 42;
      const pk = 'hibernated_pk';
      const storage = new MockStorage();
      await seedSubscription(storage, id, 'sub1', pk, [
        { kinds: [23194], authors: [pk] },
      ]);

      const engine = new NostrEngine(storage, DEFAULT_LIMITS, testNow);

      const event = makeEvent({
        id: 'event1',
        pubkey: pk,
        createdAt: testNow(),
        kind: 23194,
        tags: [Tag.p(pk)],
        content: 'wake up!',
        sig: 'sig',
      });

      const responses = await engine['processEvent'](99, event);

      expect(hasResponse(responses, r =>
        r.kind === 'wakeUp' && r.connectionId === id,
      )).toBe(true);

      expect(hasResponse(responses, r =>
        r.kind === 'send' && r.recipientId === id,
      )).toBe(true);
    });
  });

  describe('clock-based verification', () => {
    it('rejects events too far in the future', async () => {
      const now = 1700000000;
      setTestTime(now);
      const engine = newTestEngine();
      await engine.onConnect(1);

      const eventJson = JSON.stringify(['EVENT', {
        id: 'fake_id',
        pubkey: 'pk1',
        created_at: now + 901,
        kind: 23194,
        tags: [],
        content: 'test',
        sig: 'badsig',
      }]);

      let msg: ClientMessage;
      try {
        msg = JSON.parse(JSON.stringify(JSON.parse(eventJson))) as unknown as ClientMessage;
        // Actually just use parseClientMessage
        const { parseClientMessage } = await import('./nip01.ts');
        msg = parseClientMessage(eventJson);
      } catch {
        const { parseClientMessage } = await import('./nip01.ts');
        msg = parseClientMessage(eventJson);
      }

      const responses = await engine.handleTyped(1, msg);
      const ok = findOk(responses);
      expect(ok).not.toBeNull();
      expect(ok!.ok).toBe(false);
      expect(ok!.message).toContain('too far');
    });
  });

  describe('kind-5 deletion', () => {
    it('deletes info by e-tag', async () => {
      const engine = newTestEngine();
      await engine.onConnect(1);

      const infoEvent = makeEvent({ id: 'info1', kind: 13194, pubkey: 'alice', sig: 'sig1' });
      await engine['processInfoEvent'](infoEvent);
      expect(await engine.getWalletInfo('alice')).not.toBeNull();

      const deletion = makeEvent({
        id: 'del1',
        kind: 5,
        pubkey: 'alice',
        tags: [Tag.e('info1')],
        content: 'deleted',
        sig: 'sig2',
      });
      await engine['processDeletionEvent'](deletion);

      expect(await engine.getWalletInfo('alice')).toBeNull();
    });

    it('rejects unauthorized deletion', async () => {
      const engine = newTestEngine();
      await engine.onConnect(1);

      const infoEvent = makeEvent({ id: 'info1', kind: 13194, pubkey: 'alice', sig: 'sig1' });
      await engine['processInfoEvent'](infoEvent);
      expect(await engine.getWalletInfo('alice')).not.toBeNull();

      const deletion = makeEvent({
        id: 'del1',
        kind: 5,
        pubkey: 'bob',
        tags: [Tag.e('info1')],
        content: 'deleted',
        sig: 'sig2',
      });
      await engine['processDeletionEvent'](deletion);

      expect(await engine.getWalletInfo('alice')).not.toBeNull();
    });

    it('deletes info by k-tag', async () => {
      const engine = newTestEngine();
      await engine.onConnect(1);

      const infoEvent = makeEvent({ id: 'info1', kind: 13194, pubkey: 'alice', sig: 'sig1' });
      await engine['processInfoEvent'](infoEvent);
      expect(await engine.getWalletInfo('alice')).not.toBeNull();

      const deletion = makeEvent({
        id: 'del1',
        kind: 5,
        pubkey: 'alice',
        tags: [Tag.other('k', ['13194'])],
        content: 'deleted',
        sig: 'sig2',
      });
      await engine['processDeletionEvent'](deletion);

      expect(await engine.getWalletInfo('alice')).toBeNull();
    });

    it('deletes without tags (all events)', async () => {
      const engine = newTestEngine();
      await engine.onConnect(1);

      const infoEvent = makeEvent({ id: 'info1', kind: 13194, pubkey: 'alice', sig: 'sig1' });
      await engine['processInfoEvent'](infoEvent);
      expect(await engine.getWalletInfo('alice')).not.toBeNull();

      const deletion = makeEvent({
        id: 'del1',
        kind: 5,
        pubkey: 'alice',
        tags: [],
        content: 'deleted',
        sig: 'sig2',
      });
      await engine['processDeletionEvent'](deletion);

      expect(await engine.getWalletInfo('alice')).toBeNull();
    });
  });

  describe('validateEvent', () => {
    it('rejects invalid kind', () => {
      const engine = newTestEngine();
      const event = makeEvent({ kind: 1, createdAt: testNow() });
      const result = engine.validateEvent(event);
      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.error).toContain('kind not allowed');
      }
    });

    it('accepts valid kind (fails at signature, not kind)', () => {
      const engine = newTestEngine();
      const event = makeEvent({ kind: 13194, createdAt: testNow(), sig: 'badsig' });
      const result = engine.validateEvent(event);
      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.error).not.toContain('kind not allowed');
      }
    });
  });

  describe('routeVerifiedEvent', () => {
    it('caches info on route for kind 13194', async () => {
      const engine = newTestEngine();
      await engine.onConnect(1);

      const infoEvent = makeEvent({
        id: 'info1',
        kind: 13194,
        pubkey: 'alice',
        content: 'wallet info',
        sig: 'sig1',
      });

      await engine.routeVerifiedEvent(1, infoEvent);
      expect(await engine.getWalletInfo('alice')).not.toBeNull();
    });
  });

  describe('CLOSE and cleanup', () => {
    it('close returns no responses', async () => {
      const engine = newTestEngine();
      await engine.onConnect(1);

      const responses = await engine.processClose(1, 'sub1');
      expect(responses).toEqual([]);
    });

    it('terminate removes all state', async () => {
      setTestTime(testNow());
      const engine = newTestEngine();
      await engine.onConnect(1);

      await engine['processReq'](1, 'sub1', [{ authors: ['alice'] }]);
      await engine.onTerminate(1);

      const responsesAfter = await engine['processEvent'](2, makeEvent({
        id: 'event2',
        pubkey: 'alice',
        createdAt: testNow(),
        kind: 23194,
        content: 'test',
        sig: 'sig',
      }));
      expect(hasResponse(responsesAfter, r =>
        r.kind === 'send' && r.recipientId === 1,
      )).toBe(false);
    });
  });

  describe('filter validation', () => {
    it('rejects too-broad filter with CLOSED', async () => {
      const engine = newTestEngine();
      await engine.onConnect(1);

      const msg: ClientMessage = {
        type: 'REQ',
        subscriptionId: 'sub1',
        filters: [{}],
      };
      const responses = await engine.handleTyped(1, msg);
      expect(hasResponse(responses, r =>
        r.kind === 'send' && r.message.type === 'CLOSED',
      )).toBe(true);
    });
  });

  describe('storage failure handling', () => {
    it('handleReq returns CLOSED on storage failure', async () => {
      const storage = new MockStorage();
      storage.failPutBatch = true;
      const engine = new NostrEngine(storage, DEFAULT_LIMITS, testNow);
      await engine.onConnect(1);

      const responses = await engine.handleReq(1, 'sub1', [
        { kinds: [23194], authors: ['alice'] },
      ]);

      expect(responses.length).toBe(1);
      const r = responses[0];
      expect(r.kind).toBe('send');
      if (r.kind === 'send') {
        expect(r.recipientId).toBe(1);
        expect(r.message.type).toBe('CLOSED');
        if (r.message.type === 'CLOSED') {
          expect(r.message.subscriptionId).toBe('sub1');
          expect(r.message.message).toContain('persist failed');
        }
      }
    });

    it('handleReqInternal returns CLOSED on storage failure', async () => {
      const storage = new MockStorage();
      storage.failPutBatch = true;
      const engine = new NostrEngine(storage, DEFAULT_LIMITS, testNow);
      await engine.onConnect(1);

      const responses = await engine.handleReqInternal(1, 'sub1', [
        { kinds: [23194], authors: ['alice'] },
      ]);

      expect(responses.length).toBe(1);
      const r = responses[0];
      expect(r.kind).toBe('send');
      if (r.kind === 'send') {
        expect(r.message.type).toBe('CLOSED');
        if (r.message.type === 'CLOSED') {
          expect(r.message.message).toContain('persist failed');
        }
      }
    });
  });
});
