import { describe, it, expect } from 'vitest';
import { now, short, hexEncode, hexDecode, base64Encode, base64Decode } from './util.ts';

describe('short', () => {
  it('no truncation when short enough', () => {
    expect(short('hello', 10)).toBe('hello');
  });

  it('exact length', () => {
    expect(short('hello', 5)).toBe('hello');
  });

  it('truncation', () => {
    expect(short('hello world', 5)).toBe('hello');
  });

  it('empty string', () => {
    expect(short('', 5)).toBe('');
  });

  it('zero length', () => {
    expect(short('a', 0)).toBe('');
  });
});

describe('now', () => {
  it('returns recent unix timestamp', () => {
    const t = now();
    expect(t).toBeGreaterThan(1700000000);
  });
});

describe('hexEncode/hexDecode', () => {
  it('round-trips correctly', () => {
    const original = new Uint8Array([0xde, 0xad, 0xbe, 0xef]);
    const hex = hexEncode(original);
    expect(hex).toBe('deadbeef');
    const decoded = hexDecode(hex);
    expect(decoded).toEqual(original);
  });

  it('handles empty bytes', () => {
    expect(hexEncode(new Uint8Array(0))).toBe('');
    expect(hexDecode('')).toEqual(new Uint8Array(0));
  });
});

describe('base64Encode/base64Decode', () => {
  it('round-trips correctly', () => {
    const original = new TextEncoder().encode('hello world');
    const encoded = base64Encode(original);
    const decoded = base64Decode(encoded);
    expect(decoded).toEqual(original);
  });

  it('handles empty bytes', () => {
    expect(base64Encode(new Uint8Array(0))).toBe('');
    expect(base64Decode('')).toEqual(new Uint8Array(0));
  });
});
