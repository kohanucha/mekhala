import { describe, it, expect } from 'vitest';
import { encryptNip44, decryptNip44, pad, unpad } from './nip44.ts';
import { base64Encode, base64Decode } from '../util.ts';

describe('NIP-44 padding', () => {
  it('pads short messages to 32 bytes + 2 byte header', () => {
    const padded = pad('short');
    expect(padded.length).toBe(2 + 32);
    expect(unpad(padded)).toBe('short');
  });

  it('pads 33-byte messages to next power-of-two block', () => {
    const p2 = 'a'.repeat(33);
    const padded = pad(p2);
    expect(padded.length).toBe(2 + 64);
    expect(unpad(padded)).toBe(p2);
  });
});

describe('NIP-44 roundtrip', () => {
  it('encrypts and decrypts a message', () => {
    const sharedSecret = new Uint8Array(32).fill(42);
    const encrypted = encryptNip44(sharedSecret, 'Hello NIP-44 v2!');
    const decrypted = decryptNip44(sharedSecret, encrypted);
    expect(decrypted).toBe('Hello NIP-44 v2!');
  });

  it('handles empty message', () => {
    const sharedSecret = new Uint8Array(32).fill(1);
    const encrypted = encryptNip44(sharedSecret, '');
    const decrypted = decryptNip44(sharedSecret, encrypted);
    expect(decrypted).toBe('');
  });

  it('handles various message lengths', () => {
    const sharedSecret = new Uint8Array(32).fill(1);
    const lengths = [1, 31, 32, 33, 127, 128, 129, 255, 256, 257, 511, 512, 513];
    for (const len of lengths) {
      const plaintext = 'a'.repeat(len);
      const encrypted = encryptNip44(sharedSecret, plaintext);
      const decrypted = decryptNip44(sharedSecret, encrypted);
      expect(decrypted).toBe(plaintext);
    }
  });
});

describe('NIP-44 error cases', () => {
  it('fails on MAC tampering', () => {
    const sharedSecret = new Uint8Array(32).fill(1);
    const encrypted = encryptNip44(sharedSecret, 'secret message');
    const bytes = base64Decode(encrypted);
    bytes[bytes.length - 33] ^= 0xff;
    const tampered = base64Encode(bytes);
    expect(() => decryptNip44(sharedSecret, tampered)).toThrow('Invalid NIP-44 MAC');
  });

  it('rejects unsupported version', () => {
    const sharedSecret = new Uint8Array(32).fill(1);
    const payload = new Uint8Array(65);
    payload[0] = 0x03;
    const encrypted = base64Encode(payload);
    expect(() => decryptNip44(sharedSecret, encrypted)).toThrow('Unsupported NIP-44 version');
  });

  it('rejects invalid length', () => {
    const sharedSecret = new Uint8Array(32).fill(1);
    const payload = new Uint8Array(10);
    payload[0] = 0x02;
    const encrypted = base64Encode(payload);
    expect(() => decryptNip44(sharedSecret, encrypted)).toThrow('Invalid NIP-44 payload length');
  });

  it('fails with wrong key', () => {
    const key1 = new Uint8Array(32).fill(1);
    const key2 = new Uint8Array(32).fill(2);
    const encrypted = encryptNip44(key1, 'Super secret');
    expect(() => decryptNip44(key2, encrypted)).toThrow('Invalid NIP-44 MAC');
  });

  it('rejects payload too large (base64 string)', () => {
    const sharedSecret = new Uint8Array(32).fill(1);
    const oversized = 'a'.repeat(87473);
    expect(() => decryptNip44(sharedSecret, oversized)).toThrow('NIP-44 payload too large');
  });

  it('rejects decoded payload too large', () => {
    const sharedSecret = new Uint8Array(32).fill(1);
    const payload = new Uint8Array(65604);
    payload[0] = 0x02;
    const encrypted = base64Encode(payload);
    expect(() => decryptNip44(sharedSecret, encrypted)).toThrow('NIP-44 decoded payload too large');
  });

  it('rejects unpad of too short data', () => {
    expect(() => unpad(new Uint8Array(0))).toThrow('Invalid padding');
    expect(() => unpad(new Uint8Array([0x00]))).toThrow('Invalid padding');
  });

  it('rejects unpad with invalid length', () => {
    const padded = new Uint8Array([0x00, 0x05, 0x01, 0x02]);
    expect(() => unpad(padded)).toThrow('Invalid padding length');
  });
});
