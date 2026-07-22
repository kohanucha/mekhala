import { describe, it, expect } from 'vitest';
import { DEFAULT_LIMITS, createLimits } from './limits.ts';

describe('Limits', () => {
  it('default values', () => {
    expect(DEFAULT_LIMITS.maxContentLength).toBe(65536);
    expect(DEFAULT_LIMITS.maxSubscriptionsPerConnection).toBe(100);
  });

  it('createLimits', () => {
    const limits = createLimits(16384, 50);
    expect(limits.maxContentLength).toBe(16384);
    expect(limits.maxSubscriptionsPerConnection).toBe(50);
  });
});
