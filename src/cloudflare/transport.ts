import { tagsArrayFromJSON, type JsonValue } from '../nostr/tag.ts';
import type { Event } from '../nostr/event.ts';
import type { NwcUri } from '../nostr/nip47.ts';
import type { ClientMessage, RelayMessage } from '../nostr/nip01.ts';
import { parseClientMessage, relayMessageToJSON } from '../nostr/nip01.ts';
import { NostrEngine } from '../nostr/engine.ts';
import type { EngineResponse } from '../nostr/engine.ts';
import { CloudflareStorage } from './storage.ts';
import { fromEnv } from './config.ts';
import type { CloudflareConfig } from './config.ts';
import { ConnectionRegistry } from './connection.ts';
import type { WebSocketHandle } from './connection.ts';
import { CloudflareKvStore } from './kv.ts';
import { NwcRpcMachine } from '../nostr/rpc_machine.ts';
import type { RpcAction } from '../nostr/rpc_machine.ts';
import { NwcError } from '../common/mod.ts';
import type { WalletInfo } from '../nostr/nip47.ts';
import { NwcClient, parseNwcUri, EncryptionMethod } from '../nostr/nip47.ts';
import { DEFAULT_LIMITS } from '../nostr/limits.ts';

const RPC_TIMEOUT_MS = 10_000;
const MAX_MESSAGE_LENGTH = 131_072;

function now(): number {
  return Math.floor(Date.now() / 1000);
}

export class CloudflareTransport implements DurableObject {
  private config: CloudflareConfig;
  private engine: NostrEngine<CloudflareStorage>;
  private connections = new ConnectionRegistry();
  private idCounter = 0;
  private kv: CloudflareKvStore;
  private ctx: DurableObjectState;
  // Pending LNURL callback: { resolve, requestId, deadline }
  private pendingCallback: { resolve: (val: unknown) => void; requestId: string; deadline: number } | null = null;

  constructor(ctx: DurableObjectState, env: Record<string, unknown>) {
    this.ctx = ctx;
    this.config = fromEnv(env as unknown as Record<string, string | undefined>);
    const storage = new CloudflareStorage(ctx.storage);
    const limits = {
      ...DEFAULT_LIMITS,
      maxContentLength: this.config.maxContentLength,
      maxSubscriptionsPerConnection: this.config.maxSubscriptionsPerConnection,
    };
    this.engine = new NostrEngine(storage, limits, now);
    this.kv = new CloudflareKvStore(
      (env as Record<string, unknown>).MEKHALA_NWC_KV as KVNamespace,
    );
  }

  // ── DurableObject lifecycle ──

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;

    if (path.includes('/lnaddress/') && path.endsWith('/callback')) {
      return this.handleLnaddressCallback(request, path);
    }

    return this.acceptNewConnection();
  }

  async webSocketMessage(ws: WebSocket, message: ArrayBuffer | string): Promise<void> {
    if (typeof message !== 'string') {
      ws.send(JSON.stringify(['NOTICE', 'binary not supported']));
      return;
    }

    if (message.length > MAX_MESSAGE_LENGTH) {
      ws.send(JSON.stringify(['NOTICE', 'message too large']));
      return;
    }

    // Before processing, try to detect NWC responses for pending callbacks
    if (message.includes('"kind":23195') || message.includes('"kind":23196') || message.includes('"kind":23197')) {
      try {
        const parsed = JSON.parse(message);
        if (Array.isArray(parsed) && parsed[0] === 'EVENT') {
          const rawEvent = parsed[1] as { tags?: string[][]; id?: string };
          if (rawEvent?.tags) {
            // Check pending callback first (non-blocking, no storage I/O)
            for (const tag of rawEvent.tags) {
              if (tag[0] === 'e' && tag[1]) {
                const eid = tag[1];
                if (this.pendingCallback && this.pendingCallback.requestId === eid) {
                  this.pendingCallback.resolve(rawEvent);
                  this.pendingCallback = null;
                  break;
                }
              }
            }
            // Also store in DO storage as fallback
            for (const tag of rawEvent.tags) {
              if (tag[0] === 'e' && tag[1]) {
                const eid = tag[1];
                const rpcKey = `lnrpc:${eid}`;
                const pending = await this.ctx.storage.get(rpcKey);
                if (pending) {
                  await this.ctx.storage.put(`lnrpc_result:${eid}`, parsed[1]);
                  break;
                }
              }
            }
          }
        }
      } catch { /* ignore parse errors */ }
    }

    await processMessage(message, ws, this.engine, this.connections);
  }

  async webSocketClose(ws: WebSocket, _code: number, _reason: string, _wasClean: boolean): Promise<void> {
    await this.handleDisconnect(ws);
  }

  async webSocketError(ws: WebSocket, _error: unknown): Promise<void> {
    await this.handleDisconnect(ws);
  }

  // ── NwcTransport methods ──

  async getWalletInfo(pubkey: string): Promise<WalletInfo | null> {
    await this.loadConnectionByPubkey(pubkey);
    return this.engine.getWalletInfo(pubkey);
  }

  async executeNwcRpc(request: Event): Promise<Event> {
    const id = this.allocateId();
    const machine = new NwcRpcMachine(request);

    for (const action of machine.start()) {
      await this.executeRpcActionInner(id, action);
    }

    const result = this.receiveRpcResponse(id, machine);

    await this.engine.onDisconnect(id);
    this.connections.remove(id);

    return result;
  }

  // ── ID allocation ──

  private allocateId(): number {
    this.idCounter++;
    return this.idCounter;
  }

  // ── Internal RPC ──

  private async receiveRpcResponse(id: number, machine: NwcRpcMachine): Promise<Event> {
    const start = Date.now();

    while (true) {
      const elapsed = Date.now() - start;
      const remaining = RPC_TIMEOUT_MS - elapsed;
      if (remaining <= 0) {
        throw NwcError.timeout();
      }

      let text: string;
      try {
        text = await Promise.race([
          this.connections.addInternal(id),
          new Promise<string>((_, reject) =>
            setTimeout(() => reject(NwcError.timeout()), remaining),
          ),
        ]);
      } catch (e) {
        if (e instanceof NwcError) throw e;
        throw NwcError.timeout();
      }

      let msg: RelayMessage;
      try {
        msg = JSON.parse(text) as RelayMessage;
      } catch {
        throw NwcError.protocolError('malformed relay response');
      }

      const action = machine.transition(msg);
      if (action) {
        await this.executeRpcActionInner(id, action);
      }

      const state = machine.getState();
      if (state.kind === 'success') {
        return state.event;
      }
      if (state.kind === 'failed') {
        throw NwcError.protocolError(state.reason);
      }
    }
  }

  private async executeRpcActionInner(id: number, action: RpcAction): Promise<void> {
    switch (action.kind) {
      case 'subscribe': {
        const responses = await this.engine.handleReqInternal(id, action.subId, [action.filter]);
        if (responses.some(r => r.kind === 'send' && r.message.type === 'CLOSED')) {
          throw NwcError.protocolError('subscribe rejected: storage unavailable');
        }
        this.sendToWsOnly(responses);
        break;
      }
      case 'publish': {
        const msg: ClientMessage = { type: 'EVENT', event: action.event };
        const responses = await this.engine.handleTyped(id, msg);
        this.sendToWsOnly(responses);
        break;
      }
      case 'unsubscribe': {
        const responses = await this.engine.processClose(id, action.subId);
        this.sendToWsOnly(responses);
        break;
      }
    }
  }

  // ── Response dispatch ──

  private recoverConnections(): void {
    const websockets = this.ctx.getWebSockets();
    for (const ws of websockets) {
      this.connections.identify(ws);
    }
  }

  private sendToWsOnly(responses: EngineResponse[]): void {
    for (const resp of responses) {
      if (resp.kind === 'send') {
        this.connections.send(resp.recipientId, relayMessageToJSON(resp.message));
      }
    }
  }

  // ── Connection management ──

  private async loadConnectionByPubkey(pubkey: string): Promise<void> {
    const ids = await this.engine.loadByPubkey(pubkey);
    if (ids.length > 0) {
      await this.engine.load(ids[0]);
    }
  }

  private async acceptNewConnection(): Promise<Response> {
    if (this.connections.size >= this.config.maxConnections) {
      return new Response(JSON.stringify({ status: 'ERROR', reason: 'Too Many Connections' }), {
        status: 429,
        headers: { 'Content-Type': 'application/json' },
      });
    }

    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];

    const connectionId = this.allocateId();

    this.ctx.acceptWebSocket(server);
    this.connections.addExternal(connectionId, server);

    const responses = await this.engine.onConnect(connectionId);
    this.sendToWsOnly(responses);

    return new Response(null, { status: 101, webSocket: client });
  }

  private async handleDisconnect(ws: WebSocket): Promise<void> {
    const id = this.connections.identify(ws);
    if (id == null) return;

    const responses = await this.engine.onTerminate(id);
    this.sendToWsOnly(responses);
    this.connections.remove(id);
  }

  private async handleLnaddressCallback(request: Request, path: string): Promise<Response> {
    const username = path.substring(path.indexOf('/lnaddress/') + '/lnaddress/'.length, path.lastIndexOf('/callback'));
    const url = new URL(request.url);
    const amountMsat = parseInt(url.searchParams.get('amount') || '', 10);
    if (isNaN(amountMsat) || amountMsat <= 0) {
      return jsonResponse({ status: 'ERROR', reason: 'Missing amount' }, 200);
    }

    const nwcUriStr = await this.kv.getNwcUri(username);
    if (nwcUriStr == null) {
      return jsonResponse({ status: 'ERROR', reason: 'User not found' }, 200);
    }

    let nwcUri: NwcUri;
    try {
      nwcUri = parseNwcUri(nwcUriStr);
    } catch {
      return jsonResponse({ status: 'ERROR', reason: 'Invalid NWC URI' }, 200);
    }

    const client = new NwcClient(nwcUri);

    // Detect wallet's preferred encryption method from its cached info event
    const walletInfo = await this.engine.getWalletInfo(nwcUri.walletPubkey);
    if (walletInfo?.encryptionAlgorithms.includes(EncryptionMethod.Nip44)) {
      client.encryptionMethod = EncryptionMethod.Nip44;
    }

    const { event } = await client.createRequestEvent('make_invoice', { amount: amountMsat }, []);

    // Recover WebSocket connections — DO may have hibernated since last handler
    this.recoverConnections();

    const rpcId = this.allocateId();
    const subId = 'lnrpc';

    const subResponses = await this.engine.handleReqInternal(rpcId, subId, [
      { eTags: [event.id], pTags: [event.pubkey] },
    ]);
    this.sendToWsOnly(subResponses);

    const pubResponses = await this.engine.handleTyped(rpcId, { type: 'EVENT', event });
    this.sendToWsOnly(pubResponses);

    const rpcKey = `lnrpc:${event.id}`;
    await this.ctx.storage.put(rpcKey, { walletPubkey: nwcUri.walletPubkey, clientPubkey: event.pubkey, createdAt: Date.now() });

    // Wait for the wallet's response via a Promise resolved by webSocketMessage.
    // This avoids busy-polling and lets the DO runtime process WebSocket messages.
    const rawEvent = await new Promise<unknown>((resolve) => {
      this.pendingCallback = { resolve, requestId: event.id, deadline: Date.now() + RPC_TIMEOUT_MS };
      // Fallback: resolve via timeout
      setTimeout(() => {
        if (this.pendingCallback?.requestId === event.id) {
          this.pendingCallback = null;
          resolve(null);
        }
      }, RPC_TIMEOUT_MS);
    });

    if (rawEvent) {
      await this.ctx.storage.delete(rpcKey);
      await this.ctx.storage.delete(`lnrpc_result:${event.id}`);
      await this.engine.processClose(rpcId, subId);
      try {
        const raw = rawEvent as Record<string, unknown>;
        const parsedEvent: Event = {
          id: raw.id as string,
          pubkey: raw.pubkey as string,
          createdAt: (raw.created_at ?? raw.createdAt) as number,
          kind: raw.kind as number,
          tags: tagsArrayFromJSON((raw.tags as JsonValue[][] | undefined) ?? []),
          content: raw.content as string,
          sig: raw.sig as string,
        };
        // Manually decrypt and parse the response to avoid verifyEvent issues
        const decryptedContent = await client.decrypt(parsedEvent.content);
        const responseJson = JSON.parse(decryptedContent) as { result?: Record<string, unknown> };
        const invoice = responseJson?.result?.invoice;
        if (invoice) {
          return jsonResponse({ pr: invoice, routes: [] }, 200);
        }
      } catch {
        // fall through to error
      }
      return jsonResponse({ status: 'ERROR', reason: 'Failed to parse wallet response' }, 200);
    }

    await this.ctx.storage.delete(rpcKey);
    await this.engine.processClose(rpcId, subId);
    return jsonResponse({ status: 'ERROR', reason: 'Wallet not connected' }, 200);
  }
}

// ── Helpers ──

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      'Content-Type': 'application/json',
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'GET, OPTIONS',
      'Access-Control-Allow-Headers': '*',
      'Strict-Transport-Security': 'max-age=31536000; includeSubDomains',
      'X-Content-Type-Options': 'nosniff',
    },
  });
}

// ── Response dispatch helper ──

function sendToConnection(
  connections: ConnectionRegistry,
  fallbackWs: WebSocketHandle,
  responses: EngineResponse[],
): void {
  for (const resp of responses) {
    if (resp.kind === 'send') {
      const sent = connections.send(resp.recipientId, relayMessageToJSON(resp.message));
      if (!sent) {
        fallbackWs.send(relayMessageToJSON(resp.message));
      }
    }
  }
}

// ── Extracted message processing ──

export async function processMessage(
  text: string,
  ws: WebSocketHandle,
  engine: NostrEngine<CloudflareStorage>,
  connections: ConnectionRegistry,
): Promise<void> {
  let parsed: ClientMessage;
  try {
    parsed = parseClientMessage(text);
  } catch (e) {
    const errMsg = e instanceof Error ? e.message : String(e);
    try {
      const partial = JSON.parse(text);
      if (Array.isArray(partial) && partial[0] === 'EVENT' && partial[1]?.id) {
        ws.send(JSON.stringify(['OK', partial[1].id, false, `parse failed: ${errMsg}`]));
      } else {
        ws.send(JSON.stringify(['NOTICE', `parse failed: ${errMsg}`]));
      }
    } catch {
      ws.send(JSON.stringify(['NOTICE', `parse failed: ${errMsg}`]));
    }
    return;
  }

  const connectionId = connections.identify(ws);
  if (connectionId == null) {
    ws.send(JSON.stringify(['NOTICE', 'connection lost: please reconnect']));
    return;
  }
  await engine.load(connectionId);
  connections.addExternal(connectionId, ws);

    if (parsed.type === 'EVENT') {
      const event = parsed.event;
      const result = engine.validateEvent(event);
      if (result.ok) {
        ws.send(relayMessageToJSON({ type: 'OK', id: event.id, ok: true, message: '' }));
        const responses = await engine.routeVerifiedEvent(connectionId, event);
        sendToConnection(connections, ws, responses);
      } else {
        ws.send(relayMessageToJSON({ type: 'OK', id: result.id, ok: false, message: result.error }));
      }
    } else {
      const responses = await handleNonEventMessage(parsed, connectionId, engine);
      sendToConnection(connections, ws, responses);
    }
}

async function handleNonEventMessage(
  msg: ClientMessage,
  connectionId: number,
  engine: NostrEngine<CloudflareStorage>,
): Promise<EngineResponse[]> {
  switch (msg.type) {
    case 'REQ':
      return engine.handleReq(connectionId, msg.subscriptionId, msg.filters);
    case 'CLOSE':
      return engine.processClose(connectionId, msg.subscriptionId);
    default:
      return [];
  }
}
