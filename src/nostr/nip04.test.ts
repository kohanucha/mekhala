import { describe, it, expect } from 'vitest';
import { encryptNip04, decryptNip04 } from './nip04.ts';

const SHARED_SECRET = new Uint8Array(32).fill(42);

describe('NIP-04', () => {
  it('roundtrip', async () => {
    const plaintext = 'Hello, Nostr!';
    const encrypted = await encryptNip04(SHARED_SECRET, plaintext);
    expect(encrypted).toContain('?iv=');
    const decrypted = await decryptNip04(SHARED_SECRET, encrypted);
    expect(decrypted).toBe(plaintext);
  });

  it('rejects invalid format', async () => {
    await expect(decryptNip04(SHARED_SECRET, 'not-base64-content')).rejects.toThrow('Invalid NIP-04 format');
  });

  it('wrong key fails to decrypt', async () => {
    const key1 = new Uint8Array(32).fill(1);
    const key2 = new Uint8Array(32).fill(2);
    const encrypted = await encryptNip04(key1, 'Sensitive data');
    await expect(decryptNip04(key2, encrypted)).rejects.toThrow();
  });

  it('tampered ciphertext fails', async () => {
    const encrypted = await encryptNip04(SHARED_SECRET, 'Another secret');
    const parts = encrypted.split('?iv=');
    const tamperedCt = parts[0].slice(0, -1);
    const tamperedEncrypted = `${tamperedCt}?iv=${parts[1]}`;
    await expect(decryptNip04(SHARED_SECRET, tamperedEncrypted)).rejects.toThrow();
  });
});
