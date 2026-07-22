import { describe, it, expect } from 'vitest';
import { hexEncode, hexDecode } from '../common/util.ts';
import { schnorr } from '@noble/curves/secp256k1.js';
import { Tag } from './tag.ts';
import { computeEventId } from './event.ts';
import type { Event } from './event.ts';
import {
  KIND_NWC_REQUEST,
  EncryptionMethod,
  encryptionToProtocol,
  encryptionFromProtocol,
  NwcUriError,
  parseNwcUri,
  NwcClient,
  parseWalletInfo,
} from './nip47.ts';

const TEST_WALLET_SK = '0101010101010101010101010101010101010101010101010101010101010101';
const TEST_WALLET_PK = '1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f';
const TEST_NWC_URI = 'nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101';

const NOW = 1700000000;
const mockClock = () => NOW;

describe('NIP-47', () => {

  describe('NwcUri', () => {
    it('parses a valid NWC URI', () => {
      const nwcUri = parseNwcUri(TEST_NWC_URI);
      expect(nwcUri.walletPubkey).toBe(TEST_WALLET_PK);
      expect(nwcUri.secret).toBe(TEST_WALLET_SK);
    });

    it('throws on invalid scheme', () => {
      const uri = 'http://invalid.example.com?secret=0101010101010101010101010101010101010101010101010101010101010101';
      expect(() => parseNwcUri(uri)).toThrow(NwcUriError);
    });

    it('throws on missing pubkey', () => {
      const uri = 'nostr+walletconnect://?secret=0101010101010101010101010101010101010101010101010101010101010101';
      expect(() => parseNwcUri(uri)).toThrow(NwcUriError);
    });

    it('throws on missing secret', () => {
      const uri = 'nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f';
      expect(() => parseNwcUri(uri)).toThrow(NwcUriError);
    });
  });

  describe('NwcClient', () => {
    function createClient() {
      const nwcUri = parseNwcUri(TEST_NWC_URI);
      return new NwcClient(nwcUri, mockClock);
    }

    it('has required fields on construction', () => {
      const client = createClient();
      expect(client.myPubkey).toBeTruthy();
      expect(client.myPubkey.length).toBeGreaterThan(0);
    });

    it('encrypt/decrypt roundtrip (NIP-04)', async () => {
      const client = createClient();
      const payload = { test: 'data' };
      const encrypted = await client.encrypt(payload);
      const decrypted = await client.decrypt(encrypted);
      expect(JSON.parse(decrypted)).toEqual(payload);
    });

    it('encrypt/decrypt roundtrip (NIP-44)', async () => {
      const client = createClient();
      client.encryptionMethod = EncryptionMethod.Nip44;
      const payload = { test: 'nip44 data' };
      const encrypted = await client.encrypt(payload);
      const decrypted = await client.decrypt(encrypted);
      expect(JSON.parse(decrypted)).toEqual(payload);
    });

    it('produces different ciphertexts for same plaintext', async () => {
      const client = createClient();
      const payload = { same: 'data' };
      const encrypted1 = await client.encrypt(payload);
      const encrypted2 = await client.encrypt(payload);
      expect(encrypted1).not.toBe(encrypted2);
    });

    it('create request event has required fields', async () => {
      const client = createClient();
      const { event, requestId } = await client.createRequestEvent('make_invoice', {}, []);
      expect(event.pubkey).toBe(client.myPubkey);
      expect(event.kind).toBe(KIND_NWC_REQUEST);
      expect(event.id).toBe(requestId);
    });

    it('full request/response roundtrip', async () => {
      const client = createClient();
      const payload = { test: 'data' };

      const { requestId } = await client.createRequestEvent('make_invoice', payload, []);

      const respPayload = { result: { invoice: 'lnbc1...' } };
      const respEncrypted = await client.encrypt(respPayload);

      const respCreatedAt = NOW;
      const respTags = [
        Tag.e(requestId),
        Tag.p(client.myPubkey),
      ];
      const { id: respId, idBytes: respIdBytes } = computeEventId(client.walletPubkey, respCreatedAt, 23195, respTags, respEncrypted);
      const walletSkBytes = hexDecode(TEST_WALLET_SK);
      const respSigBytes = schnorr.sign(respIdBytes, walletSkBytes);
      const respSig = hexEncode(respSigBytes);

      const signedRespEvent: Event = {
        id: respId,
        pubkey: client.walletPubkey,
        createdAt: respCreatedAt,
        kind: 23195,
        tags: respTags,
        content: respEncrypted,
        sig: respSig,
      };

      const parsedResp = await client.parseResponseEvent(signedRespEvent, requestId);
      expect(parsedResp).toEqual(respPayload);
    });
  });

  describe('EncryptionMethod', () => {
    it('converts to protocol string', () => {
      expect(encryptionToProtocol(EncryptionMethod.Nip04)).toBe('nip04');
      expect(encryptionToProtocol(EncryptionMethod.Nip44)).toBe('nip44_v2');
    });

    it('converts from protocol string', () => {
      expect(encryptionFromProtocol('nip04')).toBe(EncryptionMethod.Nip04);
      expect(encryptionFromProtocol('nip44_v2')).toBe(EncryptionMethod.Nip44);
      expect(encryptionFromProtocol('unknown')).toBeNull();
    });
  });

  describe('NwcUriError', () => {
    it('has expected error messages', () => {
      expect(NwcUriError.invalidUrl('bad').message).toContain('url failure');
      expect(NwcUriError.invalidScheme().message).toContain('Invalid scheme');
      expect(NwcUriError.missingPubkey().message).toContain('Missing wallet pubkey');
      expect(NwcUriError.missingSecret().message).toContain('Missing secret');
    });
  });

  describe('parseWalletInfo', () => {
    it('defaults to NIP-04 when no encryption tag', () => {
      const event: Event = {
        id: 'id1',
        pubkey: 'pk1',
        createdAt: 1000,
        kind: 13194,
        tags: [],
        content: '',
        sig: 'sig',
      };
      const info = parseWalletInfo(event);
      expect(info.encryptionAlgorithms).toEqual([EncryptionMethod.Nip04]);
    });
  });

  describe('kind constants', () => {
    it('KIND_NWC_REQUEST is 23194', () => {
      expect(KIND_NWC_REQUEST).toBe(23194);
    });
  });
});
