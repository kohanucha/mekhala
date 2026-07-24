import {
  type ClientMessage, type EngineResponse,
  parseClientMessage, relayMessageToJSON,
} from '../nostr/index.ts';
import { ConnectionRegistry, type WebSocketHandle } from './connection.ts';
import { NostrEngine } from '../nostr/index.ts';
import { CloudflareStorage } from './storage.ts';

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
      const partial = JSON.parse(text) as unknown[];
      if (Array.isArray(partial) && partial[0] === 'EVENT' && (partial[1] as Record<string, unknown> | undefined)?.id) {
        ws.send(JSON.stringify(['OK', (partial[1] as Record<string, unknown>).id, false, `parse failed: ${errMsg}`]));
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
  console.log('[mekhala] ws-handler conn=%d type=%s', connectionId, parsed.type);
  await engine.load(connectionId);
  connections.addExternal(connectionId, ws);

  if (parsed.type === 'EVENT') {
    const event = parsed.event;
    console.log('[mekhala] ws-handler EVENT kind=%d id=%s', event.kind, event.id);
    const result = engine.validateEvent(event);
    if (result.ok) {
      console.log('[mekhala] ws-handler validate OK id=%s kind=%d', event.id, event.kind);
      ws.send(relayMessageToJSON({ type: 'OK', id: event.id, ok: true, message: '' }));
      const responses = await engine.routeVerifiedEvent(connectionId, event);
      console.log('[mekhala] ws-handler routeVerifiedEvent responses=%d', responses.length);
      sendToConnection(connections, ws, responses);
    } else {
      console.log('[mekhala] ws-handler validate FAIL id=%s kind=%d err=%s', result.id, event.kind, result.error);
      ws.send(relayMessageToJSON({ type: 'OK', id: result.id, ok: false, message: result.error }));
    }
  } else {
    const responses = await handleNonEventMessage(parsed, connectionId, engine);
    console.log('[mekhala] ws-handler nonEvent type=%s conn=%d responses=%d', parsed.type, connectionId, responses.length);
    sendToConnection(connections, ws, responses);
  }
}
