import type { Event } from '../nostr/event.ts';
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

    if (path.startsWith('/lnaddress/') && path.endsWith('/callback')) {
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
    const username = path.replace('/lnaddress/', '').replace('/callback', '');
    const url = new URL(request.url);
    const amountMsat = parseInt(url.searchParams.get('amount') || '', 10);
    if (isNaN(amountMsat)) {
      return jsonResponse({ status: 'ERROR', reason: 'Missing amount' }, 200);
    }

    const nwcUri = await this.kv.getNwcUri(username);
    if (nwcUri == null) {
      return jsonResponse({ status: 'ERROR', reason: 'User not found' }, 200);
    }

    return jsonResponse({ pr: 'placeholder_invoice', routes: [] }, 200);
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
  } catch {
    try {
      const partial = JSON.parse(text);
      if (Array.isArray(partial) && partial[0] === 'EVENT' && partial[1]?.id) {
        ws.send(JSON.stringify(['OK', partial[1].id, false, 'parse failed']));
      } else {
        ws.send(JSON.stringify(['NOTICE', 'parse failed']));
      }
    } catch {
      ws.send(JSON.stringify(['NOTICE', 'parse failed']));
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
      for (const resp of responses) {
        if (resp.kind === 'send') {
          ws.send(relayMessageToJSON(resp.message));
        }
      }
    } else {
      ws.send(relayMessageToJSON({ type: 'OK', id: result.id, ok: false, message: result.error }));
    }
  } else {
    const responses = await handleNonEventMessage(parsed, connectionId, engine);
    for (const resp of responses) {
      if (resp.kind === 'send') {
        ws.send(relayMessageToJSON(resp.message));
      }
    }
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
