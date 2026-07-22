export interface CloudflareConfig {
  maxContentLength: number;
  maxSubscriptionsPerConnection: number;
  maxConnections: number;
  walletRegion: string | null;
  relaySecret: string | null;
}

const DEFAULT_MAX_CONTENT_LENGTH = 65536;
const DEFAULT_MAX_SUBSCRIPTIONS = 100;
const DEFAULT_MAX_CONNECTIONS = 100;

export function fromEnv(env: Record<string, string | undefined>): CloudflareConfig {
  return {
    maxContentLength: parseOpt(env.MAX_CONTENT_LENGTH, DEFAULT_MAX_CONTENT_LENGTH),
    maxSubscriptionsPerConnection: parseOpt(env.MAX_SUBSCRIPTIONS_PER_CONNECTION, DEFAULT_MAX_SUBSCRIPTIONS),
    maxConnections: parseOpt(env.MAX_CONNECTIONS, DEFAULT_MAX_CONNECTIONS),
    walletRegion: env.WALLET_REGION != null && env.WALLET_REGION !== '' ? env.WALLET_REGION : null,
    relaySecret: env.RELAY_SECRET ?? null,
  };
}

export function parseOpt(value: string | undefined, defaultVal: number): number {
  if (value == null) return defaultVal;
  const trimmed = value.trim();
  if (trimmed === '') return defaultVal;
  const n = Number(trimmed);
  return Number.isInteger(n) && n >= 0 ? n : defaultVal;
}
