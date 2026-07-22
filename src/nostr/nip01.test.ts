import { describe, it, expect } from 'vitest';
import {
  parseClientMessage,
  parseRelayMessage,
  relayMessageToJSON,
  parsePartialClientMessage,
} from './nip01.ts';
import { Event } from './event.ts';

describe('parseClientMessage', () => {
  it('parses EVENT', () => {
    const json = JSON.stringify(['EVENT', {
      id: 'id', pubkey: 'pk', created_at: 1000, kind: 1, tags: [], content: 'hi', sig: 'sig',
    }]);
    const msg = parseClientMessage(json);
    if (msg.type !== 'EVENT') throw new Error('Expected EVENT');
    expect(msg.event.content).toBe('hi');
  });

  it('parses REQ', () => {
    const json = JSON.stringify(['REQ', 'sub1', { authors: ['pk1'] }]);
    const msg = parseClientMessage(json);
    if (msg.type !== 'REQ') throw new Error('Expected REQ');
    expect(msg.subscriptionId).toBe('sub1');
    expect(msg.filters).toHaveLength(1);
  });

  it('parses CLOSE', () => {
    const json = JSON.stringify(['CLOSE', 'sub1']);
    const msg = parseClientMessage(json);
    if (msg.type !== 'CLOSE') throw new Error('Expected CLOSE');
    expect(msg.subscriptionId).toBe('sub1');
  });

  it('rejects empty array', () => {
    expect(() => parseClientMessage('[]')).toThrow('empty');
  });

  it('rejects unknown type', () => {
    expect(() => parseClientMessage(JSON.stringify(['UNKNOWN', 'data']))).toThrow('unknown message type');
  });

  it('rejects malformed JSON', () => {
    expect(() => parseClientMessage('{{{')).toThrow();
  });
});

describe('relayMessageToJSON', () => {
  it('serializes NOTICE', () => {
    const json = relayMessageToJSON({ type: 'NOTICE', message: 'hi' });
    expect(json).toBe(JSON.stringify(['NOTICE', 'hi']));
  });

  it('serializes EOSE', () => {
    const json = relayMessageToJSON({ type: 'EOSE', subscriptionId: 'sub1' });
    expect(json).toBe(JSON.stringify(['EOSE', 'sub1']));
  });

  it('serializes OK', () => {
    const json = relayMessageToJSON({ type: 'OK', id: 'id1', ok: true, message: '' });
    expect(JSON.parse(json)).toEqual(['OK', 'id1', true, '']);
  });

  it('serializes EVENT', () => {
    const event: Event = {
      id: 'test_id', pubkey: 'test_pubkey', createdAt: 1234567890, kind: 1,
      tags: [], content: 'test content', sig: 'test_sig',
    };
    const json = relayMessageToJSON({ type: 'EVENT', subscriptionId: 'sub1', event });
    expect(json).toContain('EVENT');
    expect(json).toContain('sub1');
    expect(json).toContain('test_id');
  });

  it('serializes CLOSED', () => {
    const json = relayMessageToJSON({ type: 'CLOSED', subscriptionId: 'sub1', message: 'reason' });
    expect(JSON.parse(json)).toEqual(['CLOSED', 'sub1', 'reason']);
  });
});

describe('parseRelayMessage', () => {
  it('parses OK', () => {
    const msg = parseRelayMessage(JSON.stringify(['OK', 'id1', true, '']));
    expect(msg).toEqual({ type: 'OK', id: 'id1', ok: true, message: '' });
  });

  it('parses CLOSED', () => {
    const msg = parseRelayMessage(JSON.stringify(['CLOSED', 'sub1', 'reason']));
    expect(msg).toEqual({ type: 'CLOSED', subscriptionId: 'sub1', message: 'reason' });
  });

  it('parses EOSE', () => {
    const msg = parseRelayMessage(JSON.stringify(['EOSE', 'sub1']));
    expect(msg).toEqual({ type: 'EOSE', subscriptionId: 'sub1' });
  });

  it('parses NOTICE', () => {
    const msg = parseRelayMessage(JSON.stringify(['NOTICE', 'hello']));
    expect(msg).toEqual({ type: 'NOTICE', message: 'hello' });
  });

  it('parses EVENT', () => {
    const json = JSON.stringify(['EVENT', 'sub1', {
      id: 'test_id', pubkey: 'pk', created_at: 1000, kind: 1, tags: [], content: 'hi', sig: 'sig',
    }]);
    const msg = parseRelayMessage(json);
    if (msg.type !== 'EVENT') throw new Error('Expected EVENT');
    expect(msg.subscriptionId).toBe('sub1');
    expect(msg.event.id).toBe('test_id');
  });

  it('rejects unknown type', () => {
    expect(() => parseRelayMessage(JSON.stringify(['UNKNOWN']))).toThrow('unknown message type');
  });

  it('rejects empty array', () => {
    expect(() => parseRelayMessage('[]')).toThrow('empty');
  });
});

describe('parsePartialClientMessage', () => {
  it('extracts id from EVENT', () => {
    const json = JSON.stringify(['EVENT', { id: 'abc123' }]);
    const msg = parsePartialClientMessage(json);
    expect(msg).toEqual({ type: 'EVENT', id: 'abc123' });
  });

  it('returns null for non-EVENT', () => {
    const json = JSON.stringify(['REQ', 'sub1', {}]);
    expect(parsePartialClientMessage(json)).toBeNull();
  });

  it('returns null for malformed input', () => {
    expect(parsePartialClientMessage('not json')).toBeNull();
    expect(parsePartialClientMessage(JSON.stringify(['ONLY_ELEMENT']))).toBeNull();
  });
});
