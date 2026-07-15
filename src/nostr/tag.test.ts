import { describe, it, expect } from 'vitest';
import { Tag, tagsArrayFromJSON } from './tag.ts';

describe('Tag p roundtrip', () => {
  it('serializes and deserializes basic p-tag', () => {
    const tag = Tag.p('abc123');
    const json = JSON.parse(JSON.stringify(tag));
    expect(json).toEqual(['p', 'abc123']);
    const deserialized = Tag.fromJSON(json);
    expect(deserialized.equals(tag)).toBe(true);
    expect(deserialized.pubkey()).toBe('abc123');
  });

  it('serializes and deserializes p-tag with extras', () => {
    const tag = Tag.p('abc123', ['wss://relay.example.com', 'petname']);
    const json = JSON.parse(JSON.stringify(tag));
    expect(json).toEqual(['p', 'abc123', 'wss://relay.example.com', 'petname']);
    const deserialized = Tag.fromJSON(json);
    expect(deserialized.equals(tag)).toBe(true);
  });
});

describe('Tag e roundtrip', () => {
  it('serializes and deserializes e-tag', () => {
    const tag = Tag.e('event_id_1');
    const json = JSON.parse(JSON.stringify(tag));
    expect(json).toEqual(['e', 'event_id_1']);
    const deserialized = Tag.fromJSON(json);
    expect(deserialized.equals(Tag.e('event_id_1'))).toBe(true);
    expect(deserialized.eventId()).toBe('event_id_1');
  });
});

describe('Tag encryption roundtrip', () => {
  it('serializes and deserializes encryption tag', () => {
    const tag = Tag.encryption('nip44_v2 nip04');
    const json = JSON.parse(JSON.stringify(tag));
    expect(json).toEqual(['encryption', 'nip44_v2 nip04']);
    const deserialized = Tag.fromJSON(json);
    expect(deserialized.equals(tag)).toBe(true);
    expect(deserialized.encryptionScheme()).toBe('nip44_v2 nip04');
  });
});

describe('Tag expiration roundtrip', () => {
  it('serializes and deserializes expiration tag from number', () => {
    const tag = Tag.expiration(1234567890);
    const json = JSON.parse(JSON.stringify(tag));
    expect(json).toEqual(['expiration', '1234567890']);
    const deserialized = Tag.fromJSON(json);
    expect(deserialized.equals(Tag.expiration(1234567890))).toBe(true);
  });

  it('preserves string form', () => {
    const jsonIn = ['expiration', '1700000000'];
    const tag = Tag.fromJSON(jsonIn);
    expect(tag.equals(Tag.expiration('1700000000'))).toBe(true);
    const jsonOut = JSON.parse(JSON.stringify(tag));
    expect(jsonOut).toEqual(jsonIn);
  });

  it('handles numeric JSON value', () => {
    const tag = Tag.fromJSON(['expiration', 1234567890]);
    expect(tag.equals(Tag.expiration(1234567890))).toBe(true);
    const jsonOut = JSON.parse(JSON.stringify(tag));
    expect(jsonOut).toEqual(['expiration', '1234567890']);
  });
});

describe('Tag other roundtrip', () => {
  it('serializes and deserializes custom tags', () => {
    const tag = Tag.other('custom', ['value1', 'value2']);
    const json = JSON.parse(JSON.stringify(tag));
    expect(json).toEqual(['custom', 'value1', 'value2']);
    const deserialized = Tag.fromJSON(json);
    expect(deserialized.equals(tag)).toBe(true);
  });
});

describe('Tag accessors', () => {
  it('p-tag accessors work', () => {
    const p = Tag.p('pk1');
    expect(p.isP()).toBe(true);
    expect(p.isE()).toBe(false);
    expect(p.pubkey()).toBe('pk1');
    expect(p.eventId()).toBeNull();
    expect(p.encryptionScheme()).toBeNull();
  });

  it('e-tag accessors work', () => {
    const e = Tag.e('eid1');
    expect(e.isP()).toBe(false);
    expect(e.isE()).toBe(true);
    expect(e.eventId()).toBe('eid1');
    expect(e.pubkey()).toBeNull();
  });

  it('encryption tag accessors work', () => {
    const enc = Tag.encryption('nip04');
    expect(enc.encryptionScheme()).toBe('nip04');
    expect(enc.pubkey()).toBeNull();
  });

  it('expiration tag returns null pubkey/eventId', () => {
    const exp = Tag.expiration(999);
    expect(exp.pubkey()).toBeNull();
    expect(exp.eventId()).toBeNull();
  });
});

describe('Tags in Event JSON', () => {
  it('parses tags from event structure', () => {
    const eventJson = {
      id: 'abc',
      pubkey: 'pk1',
      created_at: 1000,
      kind: 23194,
      tags: [
        ['p', 'wallet_pk'],
        ['e', 'event1'],
        ['encryption', 'nip44_v2 nip04'],
      ],
      content: 'hello',
      sig: 'sig1',
    };

    const tags = tagsArrayFromJSON(eventJson.tags as any);
    expect(tags.length).toBe(3);
    expect(tags[0].equals(Tag.p('wallet_pk'))).toBe(true);
    expect(tags[1].equals(Tag.e('event1'))).toBe(true);
    expect(tags[2].equals(Tag.encryption('nip44_v2 nip04'))).toBe(true);
  });
});

describe('Tag preserves non-string values', () => {
  it('preserves extra values in extras array', () => {
    const json = ['p', 'abc123', 'wss://relay.example.com', 'petname'];
    const tag = Tag.fromJSON(json);
    expect(tag.pubkey()).toBe('abc123');
    expect(JSON.parse(JSON.stringify(tag))).toEqual(json);
  });
});

describe('Tag kind_value', () => {
  it('extracts numeric kind', () => {
    const tag = Tag.fromJSON(['k', 13194]);
    expect(tag.kindValue()).toBe(13194);
  });

  it('extracts string kind', () => {
    const tag = Tag.fromJSON(['k', '13194']);
    expect(tag.kindValue()).toBe(13194);
  });

  it('returns null for non-k tags', () => {
    expect(Tag.p('pk1').kindValue()).toBeNull();
    expect(Tag.e('event1').kindValue()).toBeNull();
  });
});
