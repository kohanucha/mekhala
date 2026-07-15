import { sha256 } from '@noble/hashes/sha2.js';
import { hmac } from '@noble/hashes/hmac.js';
import { hkdf } from '@noble/hashes/hkdf.js';
import { chacha20 } from '@noble/ciphers/chacha.js';
import { RelayError } from './error.ts';
import { base64Encode, base64Decode } from '../util.ts';

function deriveConversationKey(sharedSecret: Uint8Array): Uint8Array {
  return hmac(sha256, new TextEncoder().encode('nip44-v2'), sharedSecret);
}

function deriveMessageKeys(conversationKey: Uint8Array, nonce: Uint8Array): { chachaKey: Uint8Array; chachaNonce: Uint8Array; hmacKey: Uint8Array } {
  const okm = hkdf(sha256, conversationKey, new Uint8Array(0), nonce, 76);
  return {
    chachaKey: okm.slice(0, 32),
    chachaNonce: okm.slice(32, 44),
    hmacKey: okm.slice(44, 76),
  };
}

export function pad(plaintext: string): Uint8Array {
  const bytes = new TextEncoder().encode(plaintext);
  const unpaddedLen = bytes.length;

  let paddedLen: number;
  if (unpaddedLen <= 32) {
    paddedLen = 32;
  } else {
    const nextPower = 1 << (Math.floor(Math.log2(unpaddedLen - 1)) + 1);
    const chunk = nextPower <= 256 ? 32 : nextPower / 8;
    paddedLen = chunk * Math.ceil(unpaddedLen / chunk);
  }

  const padded = new Uint8Array(2 + paddedLen);
  padded[0] = (unpaddedLen >> 8) & 0xff;
  padded[1] = unpaddedLen & 0xff;
  padded.set(bytes, 2);
  return padded;
}

export function unpad(padded: Uint8Array): string {
  if (padded.length < 2) throw RelayError.Generic('Invalid padding');
  const len = (padded[0] << 8) | padded[1];
  if (len + 2 > padded.length) throw RelayError.Generic('Invalid padding length');
  return new TextDecoder().decode(padded.slice(2, 2 + len));
}

export function encryptNip44(sharedSecret: Uint8Array, plaintext: string): string {
  const conversationKey = deriveConversationKey(sharedSecret);
  const nonce = crypto.getRandomValues(new Uint8Array(32));
  const { chachaKey, chachaNonce, hmacKey } = deriveMessageKeys(conversationKey, nonce);

  const padded = pad(plaintext);
  const ciphertext = chacha20(chachaKey, chachaNonce, padded);

  const mac = hmac(sha256, hmacKey, new Uint8Array([...nonce, ...ciphertext]));

  const payload = new Uint8Array(1 + 32 + ciphertext.length + 32);
  payload[0] = 0x02;
  payload.set(nonce, 1);
  payload.set(ciphertext, 33);
  payload.set(mac, 33 + ciphertext.length);

  return base64Encode(payload);
}

export function decryptNip44(sharedSecret: Uint8Array, encryptedContent: string): string {
  if (encryptedContent.length > 87472) {
    throw RelayError.Generic('NIP-44 payload too large');
  }

  const payload = base64Decode(encryptedContent);

  if (payload.length > 65603) {
    throw RelayError.Generic('NIP-44 decoded payload too large');
  }

  if (payload.length === 0 || payload[0] !== 0x02) {
    throw RelayError.Generic('Unsupported NIP-44 version');
  }

  if (payload.length < 1 + 32 + 32) {
    throw RelayError.Generic('Invalid NIP-44 payload length');
  }

  const nonce = payload.slice(1, 33);
  const macStart = payload.length - 32;
  const ciphertext = payload.slice(33, macStart);
  const mac = payload.slice(macStart);

  const conversationKey = deriveConversationKey(sharedSecret);
  const { chachaKey, chachaNonce, hmacKey } = deriveMessageKeys(conversationKey, nonce);

  const expectedMac = hmac(sha256, hmacKey, new Uint8Array([...nonce, ...ciphertext]));
  if (!constantTimeEqual(mac, expectedMac)) {
    throw RelayError.CryptoError('Invalid NIP-44 MAC');
  }

  const plaintext = chacha20(chachaKey, chachaNonce, ciphertext);

  return unpad(plaintext);
}

function constantTimeEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a[i] ^ b[i];
  }
  return diff === 0;
}
