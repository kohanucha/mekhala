import { describe, it, expect } from 'vitest';
import { RelayError } from './error.ts';

describe('RelayError display', () => {
  it('produces correct messages for all variants', () => {
    expect(RelayError.InvalidKind().message).toBe('blocked: event kind not allowed');
    expect(RelayError.TimestampTooFar('skew').message).toBe('invalid: skew');
    expect(RelayError.MissingTag('p').message).toBe('invalid: missing p');
    expect(RelayError.InvalidId().message).toBe('invalid: event ID mismatch');
    expect(RelayError.InvalidSignature().message).toBe('invalid: signature verification failed');
    expect(RelayError.MalformedHex('key').message).toBe('invalid: malformed key');
    expect(RelayError.SerializationError('err').message).toBe('error: serialization failed: err');
    expect(RelayError.LimitExceeded('max').message).toBe('rejected: max');
  });

  it('produces messages for remaining variants', () => {
    expect(RelayError.CryptoError('bad').message).toBe('error: crypto failure: bad');
    expect(RelayError.Base64Error('b64 bad').message).toBe('error: base64 failure: b64 bad');
    expect(RelayError.Utf8Error('utf8 bad').message).toBe('error: utf8 failure: utf8 bad');
    expect(RelayError.Generic('oops').message).toBe('error: oops');
  });
});

describe('RelayError kind discrimination', () => {
  it('can be matched by kind', () => {
    const err = RelayError.InvalidKind();
    expect(err.kind).toBe('InvalidKind');
  });
});
