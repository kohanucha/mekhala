export function now(): number {
  return Math.floor(Date.now() / 1000);
}

export function short(s: string, len: number): string {
  if (s.length <= len) return s;
  return s.slice(0, len);
}

const HEX_CHARS = '0123456789abcdef';

export function hexEncode(bytes: Uint8Array): string {
  let hex = '';
  for (let i = 0; i < bytes.length; i++) {
    hex += HEX_CHARS[bytes[i] >> 4] + HEX_CHARS[bytes[i] & 0x0f];
  }
  return hex;
}

export function hexDecode(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.slice(i, i + 2), 16);
  }
  return bytes;
}

export function base64Encode(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

export function base64Decode(str: string): Uint8Array {
  const binary = atob(str);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function logInfo(...args: unknown[]): void {
  console.log(...args);
}

export function logDebug(...args: unknown[]): void {
  console.debug(...args);
}

export function logWarn(...args: unknown[]): void {
  console.warn(...args);
}

export function logError(...args: unknown[]): void {
  console.error(...args);
}
