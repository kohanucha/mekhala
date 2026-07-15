import { secp256k1 } from '@noble/curves/secp256k1.js';
import { schnorr } from '@noble/curves/secp256k1.js';
import { Tag } from './tag.ts';
import { RelayError } from './error.ts';
import { hexEncode, hexDecode } from '../util.ts';
import { encryptNip04, decryptNip04 } from './nip04.ts';
import { encryptNip44, decryptNip44 } from './nip44.ts';
import { computeEventId, verifyEvent } from './event.ts';
import { DEFAULT_LIMITS } from './limits.ts';
import type { Event } from './event.ts';

export const KIND_NWC_REQUEST = 23194;

function getSharedSecret(secretKeyHex: string, publicKeyHex: string): Uint8Array {
  const skBytes = hexDecode(secretKeyHex);
  const pkBytes = hexDecode(publicKeyHex);
  const compressedPk = new Uint8Array(33);
  compressedPk[0] = 0x02;
  compressedPk.set(pkBytes, 1);
  const sharedPoint = secp256k1.getSharedSecret(skBytes, compressedPk, true);
  return sharedPoint.subarray(1);
}

export type NwcMethod = 'make_invoice';

export interface NwcRequest {
  method: NwcMethod;
  params: Record<string, unknown>;
}

export interface NwcResponse {
  result?: unknown;
  error?: NwcError;
}

export interface NwcError {
  code: string;
  message: string;
}

export enum EncryptionMethod {
  Nip04 = 'Nip04',
  Nip44 = 'Nip44',
}

export function encryptionToProtocol(method: EncryptionMethod): string {
  return method === EncryptionMethod.Nip04 ? 'nip04' : 'nip44_v2';
}

export function encryptionFromProtocol(s: string): EncryptionMethod | null {
  if (s === 'nip04') return EncryptionMethod.Nip04;
  if (s === 'nip44_v2') return EncryptionMethod.Nip44;
  return null;
}

export interface WalletInfo {
  encryptionAlgorithms: EncryptionMethod[];
}

export function parseWalletInfo(event: Event): WalletInfo {
  const encryption: EncryptionMethod[] = [];
  let hasEncryptionTag = false;

  for (const tag of event.tags) {
    const scheme = tag.encryptionScheme();
    if (scheme !== null) {
      hasEncryptionTag = true;
      for (const s of scheme.split(/\s+/)) {
        const method = encryptionFromProtocol(s);
        if (method !== null && !encryption.includes(method)) {
          encryption.push(method);
        }
      }
    }
  }

  if (!hasEncryptionTag) {
    encryption.push(EncryptionMethod.Nip04);
  }

  return { encryptionAlgorithms: encryption };
}

export class NwcUriError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'NwcUriError';
  }

  static invalidUrl(msg: string): NwcUriError {
    return new NwcUriError(`error: url failure: ${msg}`);
  }

  static invalidScheme(): NwcUriError {
    return new NwcUriError('error: Invalid scheme');
  }

  static missingPubkey(): NwcUriError {
    return new NwcUriError('error: Missing wallet pubkey');
  }

  static missingSecret(): NwcUriError {
    return new NwcUriError('error: Missing secret');
  }
}

export interface NwcUri {
  walletPubkey: string;
  secret: string;
}

export function parseNwcUri(uri: string): NwcUri {
  let url: URL;
  try {
    url = new URL(uri);
  } catch (e) {
    throw NwcUriError.invalidUrl((e as Error).message);
  }

  if (url.protocol !== 'nostr+walletconnect:') {
    throw NwcUriError.invalidScheme();
  }

  const walletPubkey = url.hostname;
  if (!walletPubkey) {
    throw NwcUriError.missingPubkey();
  }

  const secret = url.searchParams.get('secret');
  if (!secret) {
    throw NwcUriError.missingSecret();
  }

  return { walletPubkey, secret };
}

export class NwcClient {
  walletPubkey: string;
  private sharedSecret: Uint8Array;
  private skBytes: Uint8Array;
  myPubkey: string;
  encryptionMethod: EncryptionMethod = EncryptionMethod.Nip04;
  private clock: () => number;

  constructor(uri: NwcUri, clock: () => number = () => Math.floor(Date.now() / 1000)) {
    this.sharedSecret = getSharedSecret(uri.secret, uri.walletPubkey);
    this.skBytes = hexDecode(uri.secret);
    this.walletPubkey = uri.walletPubkey;
    this.myPubkey = derivePubkey(this.skBytes);
    this.clock = clock;
  }

  async encrypt(payload: unknown): Promise<string> {
    const plaintext = JSON.stringify(payload);
    if (this.encryptionMethod === EncryptionMethod.Nip44) {
      return encryptNip44(this.sharedSecret, plaintext);
    }
    return encryptNip04(this.sharedSecret, plaintext);
  }

  async decrypt(encrypted: string): Promise<string> {
    if (this.encryptionMethod === EncryptionMethod.Nip44) {
      return decryptNip44(this.sharedSecret, encrypted);
    }
    return decryptNip04(this.sharedSecret, encrypted);
  }

  async createRequestEvent(
    method: NwcMethod,
    params: Record<string, unknown>,
    extraTags: Tag[],
  ): Promise<{ event: Event; requestId: string }> {
    const payload: NwcRequest = { method, params };

    const tags: Tag[] = [
      Tag.p(this.walletPubkey),
      Tag.expiration(this.clock() + 60),
      ...extraTags,
      Tag.encryption(encryptionToProtocol(this.encryptionMethod)),
    ];

    const encryptedContent = await this.encrypt(payload);
    const event = this.createEvent(KIND_NWC_REQUEST, encryptedContent, tags);
    return { event, requestId: event.id };
  }

  async parseResponseEvent(event: Event, requestId: string): Promise<unknown> {
    verifyEvent(event, this.clock(), DEFAULT_LIMITS);

    if (event.pubkey !== this.walletPubkey) {
      throw RelayError.Generic('Response pubkey mismatch');
    }

    const hasETag = event.tags.some(t => t.eventId() === requestId);
    if (!hasETag) {
      throw RelayError.Generic("Response missing 'e' tag for request");
    }

    const decrypted = await this.decrypt(event.content);
    return JSON.parse(decrypted);
  }

  createEvent(kind: number, content: string, tags: Tag[]): Event {
    const createdAt = this.clock();
    const { id, idBytes } = computeEventId(this.myPubkey, createdAt, kind, tags, content);
    const sigBytes = schnorr.sign(idBytes, this.skBytes);
    const sig = hexEncode(sigBytes);

    return {
      id,
      pubkey: this.myPubkey,
      createdAt,
      kind,
      tags,
      content,
      sig,
    };
  }
}

function derivePubkey(skBytes: Uint8Array): string {
  const pkCompressed = secp256k1.getPublicKey(skBytes, true);
  return hexEncode(pkCompressed.subarray(1));
}
