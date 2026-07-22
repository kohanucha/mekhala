import { describe, it, expect } from 'vitest';
import { Filter, filterMatches, filterIsValid, filterPubkeys, filterFromJSON } from './filter.ts';
import { Tag } from './tag.ts';
import { Event } from './event.ts';

function makeEvent(id: string, pubkey: string, kind: number, tags: Tag[], createdAt: number): Event {
  return {
    id,
    pubkey,
    createdAt,
    kind,
    tags,
    content: 'test',
    sig: 'sig',
  };
}

describe('filterMatches', () => {
  it('matches by ids', () => {
    const filter: Filter = { ids: ['id1'] };
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [], 1000))).toBe(true);
    expect(filterMatches(filter, makeEvent('id2', 'author1', 1, [], 1000))).toBe(false);
  });

  it('matches by authors', () => {
    const filter: Filter = { authors: ['author1'] };
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [], 1000))).toBe(true);
    expect(filterMatches(filter, makeEvent('id1', 'author2', 1, [], 1000))).toBe(false);
  });

  it('matches by kinds', () => {
    const filter: Filter = { kinds: [1, 2] };
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [], 1000))).toBe(true);
    expect(filterMatches(filter, makeEvent('id1', 'author1', 3, [], 1000))).toBe(false);
  });

  it('matches by since/until', () => {
    const filter: Filter = { since: 1000, until: 2000 };
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [], 1500))).toBe(true);
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [], 500))).toBe(false);
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [], 2500))).toBe(false);
  });

  it('matches by p-tags', () => {
    const filter: Filter = { pTags: ['pubkey1'] };
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [Tag.p('pubkey1')], 1000))).toBe(true);
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [Tag.p('pubkey2')], 1000))).toBe(false);
  });

  it('matches info event (kind 13194) by #p via event.pubkey', () => {
    const filter: Filter = { pTags: ['walletPk'] };
    expect(filterMatches(filter, makeEvent('id1', 'walletPk', 13194, [], 1000))).toBe(true);
    expect(filterMatches(filter, makeEvent('id1', 'otherPk', 13194, [], 1000))).toBe(false);
  });

  it('does not match non-info event by #p via event.pubkey', () => {
    const filter: Filter = { pTags: ['pk1'] };
    expect(filterMatches(filter, makeEvent('id1', 'pk1', 1, [Tag.p('pk1')], 1000))).toBe(true);
    expect(filterMatches(filter, makeEvent('id1', 'pk1', 23194, [], 1000))).toBe(false);
  });

  it('matches by e-tags', () => {
    const filter: Filter = { eTags: ['event1'] };
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [Tag.e('event1')], 1000))).toBe(true);
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [Tag.e('event2')], 1000))).toBe(false);
  });

  it('matches all criteria', () => {
    const filter: Filter = { ids: ['id1'], authors: ['author1'], kinds: [1], since: 500, until: 2000 };
    expect(filterMatches(filter, makeEvent('id1', 'author1', 1, [], 1000))).toBe(true);
    expect(filterMatches(filter, makeEvent('id2', 'author1', 1, [], 1000))).toBe(false);
    expect(filterMatches(filter, makeEvent('id1', 'author2', 1, [], 1000))).toBe(false);
  });

  it('matches empty filter', () => {
    expect(filterMatches({}, makeEvent('id1', 'author1', 1, [], 1000))).toBe(true);
  });
});

describe('filterIsValid', () => {
  it('requires narrowing', () => {
    expect(filterIsValid({})).toBe(false);
  });

  it('valid with narrowing', () => {
    expect(filterIsValid({ kinds: [13194], pTags: ['author1'] })).toBe(true);
  });

  it('requires kinds without narrowing', () => {
    expect(filterIsValid({ authors: ['author1'] })).toBe(false);
  });

  it('rejects non-NWC kinds', () => {
    expect(filterIsValid({ kinds: [1], authors: ['author1'] })).toBe(false);
    expect(filterIsValid({ kinds: [13194, 1], authors: ['author1'] })).toBe(false);
  });

  it('accepts all NWC kinds', () => {
    for (const kind of [5, 13194, 23194, 23195, 23196, 23197]) {
      expect(filterIsValid({ kinds: [kind], authors: ['author1'] })).toBe(true);
    }
  });

  it('accepts kind 5', () => {
    expect(filterIsValid({ kinds: [5], authors: ['author1'] })).toBe(true);
  });

  it('valid with p-tags only', () => {
    expect(filterIsValid({ pTags: ['pubkey1'] })).toBe(true);
  });

  it('rejects non-NWC kinds with narrowing', () => {
    expect(filterIsValid({ kinds: [1], pTags: ['pk1'] })).toBe(false);
  });

  it('rejects mixed kinds with narrowing', () => {
    expect(filterIsValid({ kinds: [23194, 1], pTags: ['pk1'] })).toBe(false);
  });

  it('rejects filter with no authors or tags and only kinds', () => {
    expect(filterIsValid({ kinds: [13194] })).toBe(false);
  });

  it('valid with empty authors array and valid kinds', () => {
    expect(filterIsValid({ kinds: [23194], authors: [] })).toBe(true);
  });
});

describe('filterPubkeys', () => {
  it('returns from authors', () => {
    const keys = filterPubkeys({ authors: ['author1', 'author2'] });
    expect(keys).toHaveLength(2);
    expect(keys).toContain('author1');
    expect(keys).toContain('author2');
  });

  it('returns from p-tags', () => {
    const keys = filterPubkeys({ pTags: ['pubkey1', 'pubkey2'] });
    expect(keys).toHaveLength(2);
  });

  it('returns from both', () => {
    const keys = filterPubkeys({ authors: ['author1'], pTags: ['pubkey1'] });
    expect(keys).toHaveLength(2);
  });
});

describe('filterFromJSON', () => {
  it('parses #p and #e fields', () => {
    const f = filterFromJSON({ '#p': ['pk1'], '#e': ['e1'], kinds: [23194] });
    expect(f.pTags).toEqual(['pk1']);
    expect(f.eTags).toEqual(['e1']);
    expect(f.kinds).toEqual([23194]);
  });

  it('handles limit as string', () => {
    const f = filterFromJSON({ kinds: [23194], limit: '1' });
    expect(f.limit).toBe(1);
  });

  it('handles limit as number', () => {
    const f = filterFromJSON({ kinds: [23194], limit: 5 });
    expect(f.limit).toBe(5);
  });
});
