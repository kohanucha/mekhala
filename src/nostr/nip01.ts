import { Tag } from './tag.ts';
import { Event } from './event.ts';
import { Filter, filterFromJSON } from './filter.ts';

export type ClientMessage =
  | { type: 'EVENT'; event: Event }
  | { type: 'REQ'; subscriptionId: string; filters: Filter[] }
  | { type: 'CLOSE'; subscriptionId: string };

export type PartialClientMessage =
  | { type: 'EVENT'; id: string };

export function parsePartialClientMessage(text: string): PartialClientMessage | null {
  try {
    const arr = JSON.parse(text) as unknown[];
    if (arr.length < 2) return null;
    if (arr[0] === 'EVENT') {
      const event = arr[1] as Record<string, unknown>;
      if (typeof event?.id === 'string') {
        return { type: 'EVENT', id: event.id };
      }
    }
    return null;
  } catch {
    return null;
  }
}

export function parseClientMessage(text: string): ClientMessage {
  const arr = JSON.parse(text) as unknown[];
  if (arr.length === 0) throw new Error('empty message');

  const msgType = arr[0];
  if (typeof msgType !== 'string') throw new Error(`unknown message type: ${msgType}`);

  switch (msgType) {
    case 'EVENT': {
      if (arr.length < 2) throw new Error(`unknown message type: ${msgType}`);
      const event = parseEventJSON(arr[1] as Record<string, unknown>);
      return { type: 'EVENT', event };
    }
    case 'REQ': {
      if (arr.length < 3) throw new Error(`unknown message type: ${msgType}`);
      const subId = arr[1];
      if (typeof subId !== 'string') throw new Error(`unknown message type: ${msgType}`);
      const filters = (arr.slice(2) as Record<string, unknown>[]).map(f => filterFromJSON(f));
      return { type: 'REQ', subscriptionId: subId, filters };
    }
    case 'CLOSE': {
      if (arr.length < 2) throw new Error(`unknown message type: ${msgType}`);
      const subId = arr[1];
      if (typeof subId !== 'string') throw new Error(`unknown message type: ${msgType}`);
      return { type: 'CLOSE', subscriptionId: subId };
    }
    default:
      throw new Error(`unknown message type: ${msgType}`);
  }
}

export type RelayMessage =
  | { type: 'OK'; id: string; ok: boolean; message: string }
  | { type: 'EVENT'; subscriptionId: string; event: Event }
  | { type: 'EOSE'; subscriptionId: string }
  | { type: 'NOTICE'; message: string }
  | { type: 'CLOSED'; subscriptionId: string; message: string };

export function parseRelayMessage(text: string): RelayMessage {
  const arr = JSON.parse(text) as unknown[];
  if (arr.length === 0) throw new Error('empty message');

  const msgType = arr[0];
  if (typeof msgType !== 'string') throw new Error(`unknown message type: ${msgType}`);

  switch (msgType) {
    case 'OK': {
      if (arr.length < 4) throw new Error('OK requires id, bool, message');
      return {
        type: 'OK',
        id: arr[1] as string,
        ok: arr[2] as boolean,
        message: arr[3] as string,
      };
    }
    case 'EVENT': {
      if (arr.length < 3) throw new Error('EVENT requires sub_id and event');
      return {
        type: 'EVENT',
        subscriptionId: arr[1] as string,
        event: parseEventJSON(arr[2] as Record<string, unknown>),
      };
    }
    case 'EOSE': {
      if (arr.length < 2) throw new Error('EOSE requires sub_id');
      return { type: 'EOSE', subscriptionId: arr[1] as string };
    }
    case 'NOTICE': {
      if (arr.length < 2) throw new Error('NOTICE requires message');
      return { type: 'NOTICE', message: arr[1] as string };
    }
    case 'CLOSED': {
      if (arr.length < 3) throw new Error('CLOSED requires sub_id and message');
      return {
        type: 'CLOSED',
        subscriptionId: arr[1] as string,
        message: arr[2] as string,
      };
    }
    default:
      throw new Error(`unknown message type: ${msgType}`);
  }
}

export function relayMessageToJSON(msg: RelayMessage): string {
  switch (msg.type) {
    case 'OK':
      return JSON.stringify(['OK', msg.id, msg.ok, msg.message]);
    case 'EVENT':
      return JSON.stringify(['EVENT', msg.subscriptionId, msg.event]);
    case 'EOSE':
      return JSON.stringify(['EOSE', msg.subscriptionId]);
    case 'NOTICE':
      return JSON.stringify(['NOTICE', msg.message]);
    case 'CLOSED':
      return JSON.stringify(['CLOSED', msg.subscriptionId, msg.message]);
  }
}

function parseEventJSON(json: Record<string, unknown>): Event {
  return {
    id: json.id as string,
    pubkey: json.pubkey as string,
    createdAt: json.created_at as number,
    kind: json.kind as number,
    tags: ((json.tags as Array<Array<unknown>>) ?? []).map(t => Tag.fromJSON(t as never)),
    content: json.content as string,
    sig: json.sig as string,
  };
}
