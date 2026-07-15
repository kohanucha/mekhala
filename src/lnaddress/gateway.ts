import type { NwcTransport } from '../common/mod.ts';
import { sha256 } from '@noble/hashes/sha2.js';
import { hexEncode } from '../util.ts';
import { NwcSession } from './wallet_connector.ts';

export function payRequestInfo(username: string, requestUrl: URL): Record<string, unknown> {
  const callbackUrl = buildCallbackUrl(username, requestUrl);

  return {
    callback: callbackUrl,
    maxSendable: 100_000_000,
    minSendable: 1000,
    metadata: generateMetadata(username),
    tag: 'payRequest',
  };
}

export async function createInvoice(
  transport: NwcTransport,
  nwcUri: string,
  username: string,
  amountMsat: number,
): Promise<string> {
  const descriptionHash = getDescriptionHash(username);
  const session = new NwcSession(transport, nwcUri);
  return session.makeInvoice(amountMsat, descriptionHash);
}

export function generateMetadata(username: string): string {
  return `[["text/plain","Payment to ${username}"]]`;
}

export function getDescriptionHash(username: string): string {
  const metadata = generateMetadata(username);
  const hash = sha256(new TextEncoder().encode(metadata));
  return hexEncode(hash);
}

export function buildCallbackUrl(username: string, requestUrl: URL): string {
  const isLocal = requestUrl.hostname === 'localhost' || requestUrl.hostname === '127.0.0.1';
  const host = requestUrl.hostname;
  const port = requestUrl.port ? `:${requestUrl.port}` : '';
  const protocol = isLocal ? 'http' : 'https';

  return `${protocol}://${host}${port}/lnaddress/${username}/callback`;
}
