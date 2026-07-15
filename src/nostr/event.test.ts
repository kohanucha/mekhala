import { describe, it, expect } from 'vitest';
import { Event, computeEventId, targetPubkeys, verifyEvent } from './event.ts';
import { Tag } from './tag.ts';
import { RelayError } from './error.ts';
import { DEFAULT_LIMITS } from './limits.ts';
import { schnorr } from '@noble/curves/secp256k1.js';
import { hexDecode, hexEncode } from '../util.ts';

const TEST_WALLET_SK = '0101010101010101010101010101010101010101010101010101010101010101';
const TEST_WALLET_PK = '1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f';

function makeEvent(id: string, pubkey: string, kind: number, tags: Tag[], content: string, createdAt: number): Event {
  return {
    id, pubkey, createdAt, kind, tags, content, sig: 'sig',
  };
}

describe('targetPubkeys', () => {
  it('includes author only when no p-tags', () => {
    const event = makeEvent('id', 'pk1', 1, [], '', 0);
    const keys = targetPubkeys(event);
    expect(keys.size).toBe(1);
    expect(keys.has('pk1')).toBe(true);
  });

  it('includes author and p-tag recipients', () => {
    const event = makeEvent('id', 'author', 1, [Tag.p('recipient1'), Tag.p('recipient2'), Tag.e('event_id')], '', 0);
    const keys = targetPubkeys(event);
    expect(keys.size).toBe(3);
    expect(keys.has('author')).toBe(true);
    expect(keys.has('recipient1')).toBe(true);
    expect(keys.has('recipient2')).toBe(true);
  });

  it('deduplicates pubkeys', () => {
    const tagSelf = Tag.p('pk1');
    const event = makeEvent('id', 'pk1', 1, [tagSelf, Tag.p('pk2'), Tag.p('pk2')], '', 0);
    const keys = targetPubkeys(event);
    expect(keys.size).toBe(2);
    expect(keys.has('pk1')).toBe(true);
    expect(keys.has('pk2')).toBe(true);
  });
});

describe('verifyEvent', () => {
  it('kind 5 passes kind check but fails on id/sig', () => {
    const event = makeEvent('id5', 'pk1', 5, [Tag.e('event_to_delete')], 'deleting', 1700000000);
    try {
      verifyEvent(event, 1700000000, DEFAULT_LIMITS);
      expect.unreachable('should have thrown');
    } catch (err) {
      expect(err instanceof RelayError).toBe(true);
      expect((err as RelayError).kind).not.toBe('InvalidKind');
    }
  });

  it('rejects invalid kind', () => {
    const event = makeEvent('id1', 'pk1', 1, [], '', 1700000000);
    expect(() => verifyEvent(event, 1700000000, DEFAULT_LIMITS)).toThrow('blocked: event kind not allowed');
  });

  it('rejects content too large', () => {
    const event = makeEvent('id1', 'pk1', 13194, [], 'a'.repeat(65537), 1700000000);
    try {
      verifyEvent(event, 1700000000, DEFAULT_LIMITS);
      expect.unreachable('should have thrown');
    } catch (err) {
      expect((err as RelayError).kind).toBe('LimitExceeded');
    }
  });

  it('kind 23196 missing p-tag', () => {
    const event = makeEvent('id1', 'pk1', 23196, [], '', 1700000000);
    expect(() => verifyEvent(event, 1700000000, DEFAULT_LIMITS)).toThrow('invalid: missing p');
  });

  it('kind 23197 missing p-tag', () => {
    const event = makeEvent('id2', 'pk1', 23197, [Tag.e('eid1')], '', 1700000000);
    expect(() => verifyEvent(event, 1700000000, DEFAULT_LIMITS)).toThrow('invalid: missing p');
  });

  it('kind 23195 missing p-tag', () => {
    const event = makeEvent('id3', 'pk1', 23195, [Tag.e('eid1')], '', 1700000000);
    expect(() => verifyEvent(event, 1700000000, DEFAULT_LIMITS)).toThrow('invalid: missing p');
  });

  it('kind 23195 missing e-tag', () => {
    const event = makeEvent('id4', 'pk1', 23195, [Tag.p('pk2')], '', 1700000000);
    expect(() => verifyEvent(event, 1700000000, DEFAULT_LIMITS)).toThrow('invalid: missing e');
  });

  it('kind 23196 with p-tag passes kind check', () => {
    const event = makeEvent('id5', 'pk1', 23196, [Tag.p('pk2')], '', 1700000000);
    try {
      verifyEvent(event, 1700000000, DEFAULT_LIMITS);
      expect.unreachable('should have thrown');
    } catch (err) {
      expect((err as RelayError).kind).not.toBe('InvalidKind');
    }
  });

  it('verifies valid signature', () => {
    const skBytes = hexDecode(TEST_WALLET_SK);
    const { id, idBytes } = computeEventId(TEST_WALLET_PK, 1700000000, 23194, [], 'test');
    const sigRaw = schnorr.sign(idBytes, skBytes);
    const sig = hexEncode(sigRaw);

    const event: Event = {
      id,
      pubkey: TEST_WALLET_PK,
      createdAt: 1700000000,
      kind: 23194,
      tags: [],
      content: 'test',
      sig,
    };

    verifyEvent(event, 1700000000, DEFAULT_LIMITS);
  });
});
