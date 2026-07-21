import { sha256 } from '@noble/hashes/sha2.js';
import { schnorr } from '@noble/curves/secp256k1.js';
import { Tag } from './tag.ts';
import { RelayError } from './error.ts';
import { Limits, DEFAULT_LIMITS, isNwcKind } from './limits.ts';
import { hexEncode, hexDecode } from '../common/util.ts';

export interface Event {
  id: string;
  pubkey: string;
  createdAt: number;
  kind: number;
  tags: Tag[];
  content: string;
  sig: string;
}

export function computeEventId(pubkey: string, createdAt: number, kind: number, tags: Tag[], content: string): { id: string; idBytes: Uint8Array } {
  const serialized = JSON.stringify([0, pubkey, createdAt, kind, tags, content]);
  const idBytes = sha256(new TextEncoder().encode(serialized));
  const id = hexEncode(idBytes);
  return { id, idBytes };
}

export function targetPubkeys(event: Event): Set<string> {
  const keys = new Set<string>();
  keys.add(event.pubkey);
  for (const tag of event.tags) {
    const pk = tag.pubkey();
    if (pk !== null) keys.add(pk);
  }
  return keys;
}

export function serializeEvent(event: Event): Record<string, unknown> {
  return {
    id: event.id,
    pubkey: event.pubkey,
    created_at: event.createdAt,
    kind: event.kind,
    tags: event.tags,
    content: event.content,
    sig: event.sig,
  };
}

export function verifyEvent(event: Event, currentTime: number, limits: Limits = DEFAULT_LIMITS): void {
  if (!isNwcKind(event.kind)) {
    throw RelayError.InvalidKind();
  }

  switch (event.kind) {
    case 23195: {
      const hasP = event.tags.some(t => t.isP());
      const hasE = event.tags.some(t => t.isE());
      if (!hasP) throw RelayError.MissingTag('p');
      if (!hasE) throw RelayError.MissingTag('e');
      break;
    }
    case 23196:
    case 23197: {
      const hasP = event.tags.some(t => t.isP());
      if (!hasP) throw RelayError.MissingTag('p');
      break;
    }
  }

  if (event.content.length > limits.maxContentLength) {
    throw RelayError.LimitExceeded(`content too large (max ${limits.maxContentLength} bytes)`);
  }

  if (event.createdAt > currentTime + 900) {
    throw RelayError.TimestampTooFar('event creation date is too far off from the current time');
  }
  if (event.createdAt < currentTime - 31_536_000) {
    throw RelayError.TimestampTooFar('event creation date is too old');
  }

  const { id, idBytes } = computeEventId(event.pubkey, event.createdAt, event.kind, event.tags, event.content);
  if (event.id !== id) {
    throw RelayError.InvalidId();
  }

  try {
    const pubkeyBytes = hexDecode(event.pubkey);
    const sigBytes = hexDecode(event.sig);

    const isValid = schnorr.verify(sigBytes, idBytes, pubkeyBytes);
    if (!isValid) {
      throw RelayError.InvalidSignature();
    }
  } catch (err) {
    if (err instanceof RelayError) throw err;
    throw RelayError.MalformedHex('signature verification failed');
  }
}
