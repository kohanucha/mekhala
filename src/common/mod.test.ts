import { describe, it, expect } from 'vitest';
import { NwcError } from './mod.ts';

describe('NwcError', () => {
  it('WalletNotFound display', () => {
    const err = NwcError.walletNotFound();
    expect(err.message).toBe('Wallet not connected');
  });

  it('Timeout display', () => {
    const err = NwcError.timeout();
    expect(err.message).toBe('NWC RPC timeout');
  });

  it('ProtocolError display', () => {
    const err = NwcError.protocolError('bad thing');
    expect(err.message).toBe('bad thing');
  });

  it('RpcError display', () => {
    const err = NwcError.rpcError('PAYMENT_FAILED', 'no funds');
    expect(err.message).toBe('NWC Error (PAYMENT_FAILED): no funds');
  });

  it('fromRelayError', () => {
    const relayErr = new Error('rejected: rate limit');
    const nwc = NwcError.fromRelayError(relayErr);
    expect(nwc.kind).toBe('ProtocolError');
    expect(nwc.message).toContain('rejected');
  });

  it('fromNwcUriError', () => {
    const uriErr = new Error('Invalid scheme');
    const nwc = NwcError.fromNwcUriError(uriErr);
    expect(nwc.kind).toBe('ProtocolError');
    expect(nwc.message).toContain('Invalid scheme');
  });

  it('fromJsonError', () => {
    const jsonErr = new SyntaxError('Unexpected token');
    const nwc = NwcError.fromJsonError(jsonErr);
    expect(nwc.kind).toBe('ProtocolError');
  });

  it('name is NwcError', () => {
    const err = NwcError.timeout();
    expect(err.name).toBe('NwcError');
  });

  it('clone has same values', () => {
    const err = NwcError.walletNotFound();
    const cloned = NwcError.walletNotFound();
    expect(cloned.kind).toBe(err.kind);
    expect(cloned.message).toBe(err.message);
  });
});
