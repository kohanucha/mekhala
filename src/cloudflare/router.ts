import { fromEnv } from './config.ts';
import { getDurableStub } from './durable_object.ts';
import { AccessPolicy } from './auth.ts';
import { CloudflareKvStore } from './kv.ts';
import { isValidUsername } from '../lnaddress/index.ts';

export async function handleRequest(
  request: Request,
  env: Record<string, unknown>,
): Promise<Response> {
  const url = new URL(request.url);
  const path = url.pathname;

  if (request.method === 'OPTIONS') {
    return corsResponse(null, 204);
  }

  const lnurlMatch = /\/\.well-known\/lnurlp\/([^/]+)$/.exec(path);
  if (lnurlMatch) {
    return handleLnurlp(lnurlMatch[1], request, env);
  }

  if (path.includes('/lnaddress/') && path.endsWith('/callback')) {
    return forwardToDo(request, env);
  }

  const secret = path === '/' ? '' : path.slice(1);
  return handleRelay(request, env, secret);
}

function corsResponse(body: unknown, status: number): Response {
  const headers = new Headers({
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'GET, OPTIONS',
    'Access-Control-Allow-Headers': '*',
    'Content-Type': 'application/json',
  });
  securityHeaders(headers);
  return new Response(body != null ? JSON.stringify(body) : null, { status, headers });
}

function securityHeaders(headers: Headers): Headers {
  headers.set('Strict-Transport-Security', 'max-age=31536000; includeSubDomains');
  headers.set('X-Content-Type-Options', 'nosniff');
  headers.set('Content-Security-Policy', "default-src 'self'");
  return headers;
}

function authResponse(): Response {
  const body = JSON.stringify({ status: 'ERROR', reason: 'Not Found' });
  const headers = new Headers({
    'Content-Type': 'application/json',
  });
  securityHeaders(headers);
  return new Response(body, { status: 404, headers });
}

function nip11Response(): Response {
  const body = JSON.stringify({ supported_nips: [1, 9, 11, 47] });
  const headers = new Headers({
    'Content-Type': 'application/nostr+json',
    'Access-Control-Allow-Origin': '*',
  });
  securityHeaders(headers);
  return new Response(body, { status: 200, headers });
}

function isWebSocketUpgrade(request: Request): boolean {
  const upgrade = request.headers.get('Upgrade');
  return upgrade?.toLowerCase() === 'websocket';
}

function handleRelay(
  request: Request,
  env: Record<string, unknown>,
  secret: string,
): Response | Promise<Response> {
  const config = fromEnv(env as Record<string, string | undefined>);
  const policy = new AccessPolicy(config.relaySecret);

  try {
    policy.checkAccess(secret);
  } catch {
    return authResponse();
  }

  if (isWebSocketUpgrade(request)) {
    return forwardToDo(request, env);
  }

  return nip11Response();
}

async function handleLnurlp(
  username: string,
  request: Request,
  env: Record<string, unknown>,
): Promise<Response> {
  if (!isValidUsername(username)) {
    return corsResponse({ status: 'ERROR', reason: 'Not Found' }, 404);
  }

  const kv = env.MEKHALA_NWC_KV as KVNamespace;
  const store = new CloudflareKvStore(kv);
  const nwcUri = await store.getNwcUri(username);

  if (nwcUri == null) {
    return corsResponse({ status: 'ERROR', reason: 'User not found' }, 200);
  }

  const url = new URL(request.url);
  const host = url.hostname;
  const port = url.port ? `:${url.port}` : '';
  const protocol = host === 'localhost' || host === '127.0.0.1' ? 'http' : 'https';
  const callbackUrl = `${protocol}://${host}${port}/lnaddress/${username}/callback`;
  const metadata = JSON.stringify([['text/plain', `Payment to ${username}`]]);
  const body = {
    callback: callbackUrl,
    maxSendable: 100_000_000,
    minSendable: 1_000,
    metadata,
    tag: 'payRequest',
  };
  return corsResponse(body, 200);
}

function forwardToDo(request: Request, env: Record<string, unknown>): Promise<Response> {
  const config = fromEnv(env as Record<string, string | undefined>);
  const stub = getDurableStub(env, config.walletRegion ?? undefined);
  return stub.fetch(request);
}
