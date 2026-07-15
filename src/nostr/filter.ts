import { Event } from './event.ts';

export interface Filter {
  ids?: string[];
  authors?: string[];
  kinds?: number[];
  pTags?: string[];
  eTags?: string[];
  since?: number;
  until?: number;
  limit?: number;
}

const NWC_KINDS = new Set([5, 13194, 23194, 23195, 23196, 23197]);

function isNwcKind(k: number): boolean {
  return NWC_KINDS.has(k);
}

export function filterFromJSON(json: Record<string, unknown>): Filter {
  const f: Filter = {};
  if (Array.isArray(json.ids)) f.ids = json.ids as string[];
  if (Array.isArray(json.authors)) f.authors = json.authors as string[];
  if (Array.isArray(json.kinds)) f.kinds = json.kinds as number[];
  const pRaw = json['#p'];
  if (Array.isArray(pRaw)) f.pTags = pRaw as string[];
  const eRaw = json['#e'];
  if (Array.isArray(eRaw)) f.eTags = eRaw as string[];
  if (typeof json.since === 'number') f.since = json.since as number;
  if (typeof json.until === 'number') f.until = json.until as number;
  if (typeof json.limit === 'number' || typeof json.limit === 'string') {
    f.limit = Number(json.limit);
  }
  return f;
}

export function filterToJSON(f: Filter): Record<string, unknown> {
  const json: Record<string, unknown> = {};
  if (f.ids) json.ids = f.ids;
  if (f.authors) json.authors = f.authors;
  if (f.kinds) json.kinds = f.kinds;
  if (f.pTags) json['#p'] = f.pTags;
  if (f.eTags) json['#e'] = f.eTags;
  if (f.since !== undefined) json.since = f.since;
  if (f.until !== undefined) json.until = f.until;
  if (f.limit !== undefined) json.limit = f.limit;
  return json;
}

export function filterMatches(filter: Filter, event: Event): boolean {
  if (filter.ids && !filter.ids.includes(event.id)) {
    return false;
  }
  if (filter.authors && !filter.authors.includes(event.pubkey)) {
    return false;
  }
  if (filter.kinds && !filter.kinds.includes(event.kind)) {
    return false;
  }
  if (filter.pTags) {
    const hasMatch = event.tags.some(t => {
      const pk = t.pubkey();
      return pk !== null && filter.pTags!.includes(pk);
    });
    if (!hasMatch) return false;
  }
  if (filter.eTags) {
    const hasMatch = event.tags.some(t => {
      const eid = t.eventId();
      return eid !== null && filter.eTags!.includes(eid);
    });
    if (!hasMatch) return false;
  }
  if (filter.since !== undefined && event.createdAt < filter.since) {
    return false;
  }
  if (filter.until !== undefined && event.createdAt > filter.until) {
    return false;
  }
  return true;
}

export function filterIsValid(filter: Filter): boolean {
  const hasSpecificNarrowing =
    filter.pTags !== undefined ||
    filter.eTags !== undefined ||
    filter.ids !== undefined;

  if (hasSpecificNarrowing) {
    if (filter.kinds) {
      if (filter.kinds.some(k => !isNwcKind(k))) {
        return false;
      }
    }
  } else {
    const kinds = filter.kinds;
    if (!kinds || kinds.length === 0) return false;
    if (kinds.some(k => !isNwcKind(k))) return false;
    if (
      filter.ids === undefined &&
      filter.authors === undefined &&
      filter.pTags === undefined &&
      filter.eTags === undefined
    ) {
      return false;
    }
  }
  return true;
}

export function filterPubkeys(filter: Filter): string[] {
  const keys: string[] = [];
  if (filter.authors) keys.push(...filter.authors);
  if (filter.pTags) keys.push(...filter.pTags);
  return keys;
}
