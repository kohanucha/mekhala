import { RelayError } from './error.ts';
import { base64Encode, base64Decode } from '../util.ts';
export async function encryptNip04(sharedSecret: Uint8Array, plaintext: string): Promise<string> {
  const iv = crypto.getRandomValues(new Uint8Array(16));
  const key = await crypto.subtle.importKey('raw', sharedSecret as BufferSource, { name: 'AES-CBC' }, false, ['encrypt']);
  const ptBytes = new TextEncoder().encode(plaintext);
  const encrypted = await crypto.subtle.encrypt({ name: 'AES-CBC', iv }, key, ptBytes as BufferSource);

  const ctB64 = base64Encode(new Uint8Array(encrypted));
  const ivB64 = base64Encode(iv);
  return `${ctB64}?iv=${ivB64}`;
}

export async function decryptNip04(sharedSecret: Uint8Array, encryptedContent: string): Promise<string> {
  const parts = encryptedContent.split('?iv=');
  if (parts.length !== 2) {
    throw RelayError.Generic('Invalid NIP-04 format');
  }

  // Strip trailing &mac=... that some clients append
  const ivPart = parts[1].split('&')[0];

  const ctBytes = base64Decode(parts[0]);
  const ivBytes = base64Decode(ivPart);

  const key = await crypto.subtle.importKey('raw', sharedSecret as BufferSource, { name: 'AES-CBC' }, false, ['decrypt']);
  const decrypted = await crypto.subtle.decrypt({ name: 'AES-CBC', iv: ivBytes as BufferSource }, key, ctBytes as BufferSource);

  return new TextDecoder().decode(decrypted);
}
