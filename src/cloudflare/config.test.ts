import { describe, it, expect } from 'vitest';
import { parseOpt, fromEnv } from './config.ts';

describe('parseOpt', () => {
  it('returns default when missing', () => {
    expect(parseOpt(undefined, 42)).toBe(42);
  });

  it('parses valid number', () => {
    expect(parseOpt('100', 42)).toBe(100);
  });

  it('returns default for non-numeric string', () => {
    expect(parseOpt('not-a-number', 42)).toBe(42);
  });

  it('parses zero', () => {
    expect(parseOpt('0', 42)).toBe(0);
  });

  it('returns default for empty string', () => {
    expect(parseOpt('', 42)).toBe(42);
  });

  it('returns default for negative number', () => {
    expect(parseOpt('-5', 42)).toBe(42);
  });

  it('returns default for float', () => {
    expect(parseOpt('1.5', 42)).toBe(42);
  });
});

describe('fromEnv', () => {
  it('uses defaults when env is empty', () => {
    const cfg = fromEnv({});
    expect(cfg.maxContentLength).toBe(65536);
    expect(cfg.maxSubscriptionsPerConnection).toBe(100);
    expect(cfg.maxConnections).toBe(100);
    expect(cfg.walletRegion).toBeNull();
    expect(cfg.relaySecret).toBeNull();
  });

  it('reads values from env', () => {
    const cfg = fromEnv({
      MAX_CONTENT_LENGTH: '8192',
      MAX_SUBSCRIPTIONS_PER_CONNECTION: '50',
      MAX_CONNECTIONS: '10',
      WALLET_REGION: 'weur',
      RELAY_SECRET: 'hunter2',
    });
    expect(cfg.maxContentLength).toBe(8192);
    expect(cfg.maxSubscriptionsPerConnection).toBe(50);
    expect(cfg.maxConnections).toBe(10);
    expect(cfg.walletRegion).toBe('weur');
    expect(cfg.relaySecret).toBe('hunter2');
  });

  it('skips empty wallet region', () => {
    const cfg = fromEnv({ WALLET_REGION: '' });
    expect(cfg.walletRegion).toBeNull();
  });
});
