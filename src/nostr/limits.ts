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

export const NWC_KINDS = new Set([5, 13194, 23194, 23195, 23196, 23197]);

export function isNwcKind(k: number): boolean {
  return NWC_KINDS.has(k);
}
