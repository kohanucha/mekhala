import { describe, it, expect } from 'vitest';
import { payRequestInfo, createInvoice, buildCallbackUrl, generateMetadata, getDescriptionHash } from './gateway.ts';
import { MockTransport } from '../common/test_helpers.ts';

describe('buildCallbackUrl', () => {
  it('local dev', () => {
    const url = new URL('http://localhost:8787/.well-known/lnurlp/alice');
    const callback = buildCallbackUrl('alice', url);
    expect(callback).toBe('http://localhost:8787/lnaddress/alice/callback');
  });

  it('remote', () => {
    const url = new URL('https://relay.com/.well-known/lnurlp/bob');
    const callback = buildCallbackUrl('bob', url);
    expect(callback).toBe('https://relay.com/lnaddress/bob/callback');
  });

  it('no port', () => {
    const url = new URL('https://relay.com/.well-known/lnurlp/charlie');
    const callback = buildCallbackUrl('charlie', url);
    expect(callback).toBe('https://relay.com/lnaddress/charlie/callback');
  });
});

describe('generateMetadata', () => {
  it('formats metadata', () => {
    expect(generateMetadata('alice')).toBe('[["text/plain","Payment to alice"]]');
  });
});

describe('getDescriptionHash', () => {
  it('produces 64-char hex hash', () => {
    const hash = getDescriptionHash('testuser');
    expect(hash).toHaveLength(64);
  });

  it('deterministic', () => {
    const hash1 = getDescriptionHash('testuser');
    const hash2 = getDescriptionHash('testuser');
    expect(hash1).toBe(hash2);
  });
});

describe('payRequestInfo', () => {
  it('returns correct structure', () => {
    const url = new URL('https://relay.com/.well-known/lnurlp/alice');
    const info = payRequestInfo('alice', url);
    expect(info.tag).toBe('payRequest');
    expect(info.callback).toBe('https://relay.com/lnaddress/alice/callback');
    expect(info.maxSendable).toBe(100_000_000);
    expect(info.minSendable).toBe(1000);
    expect(info.metadata).toBe('[["text/plain","Payment to alice"]]');
  });

  it('uses different username', () => {
    const url = new URL('https://other.com/.well-known/lnurlp/bob');
    const info = payRequestInfo('bob', url);
    expect(info.callback).toBe('https://other.com/lnaddress/bob/callback');
    expect(info.metadata).toBe('[["text/plain","Payment to bob"]]');
  });
});

describe('createInvoice', () => {
  it('rejects invalid URI', async () => {
    const transport = MockTransport.walletNotFound();
    await expect(createInvoice(transport, 'not-a-valid-uri', 'test', 1000)).rejects.toThrow();
  });
});
