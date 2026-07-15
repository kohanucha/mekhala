export interface Limits {
  maxContentLength: number;
  maxSubscriptionsPerConnection: number;
}

export const DEFAULT_LIMITS: Limits = {
  maxContentLength: 65536,
  maxSubscriptionsPerConnection: 100,
};

export function createLimits(maxContentLength: number, maxSubscriptionsPerConnection: number): Limits {
  return { maxContentLength, maxSubscriptionsPerConnection };
}
