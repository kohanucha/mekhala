import type { Event } from './event.ts';
import type { Filter } from './filter.ts';
import type { Limits } from './limits.ts';
import { isNwcKind } from './limits.ts';
import type { ClientMessage, RelayMessage } from './nip01.ts';
import { verifyEvent } from './event.ts';
import { filterPubkeys, filterMatches, filterIsValid } from './filter.ts';
import { WalletRegistry } from './wallet_registry.ts';
import type { Storage } from './wallet_registry.ts';
import { RelayError } from './error.ts';
import { parseWalletInfo } from './nip47.ts';
import type { WalletInfo } from './nip47.ts';

export type EngineResponse =
  | { kind: 'send'; recipientId: number; message: RelayMessage }
  | { kind: 'wakeUp'; connectionId: number };

export class NostrEngine<S extends Storage> {
  private registry: WalletRegistry<S>;
  private limits: Limits;
  private clock: () => number;

  constructor(storage: S, limits: Limits, clock: () => number) {
    this.registry = new WalletRegistry(storage, limits);
    this.limits = limits;
    this.clock = clock;
  }

  async handleTyped(connectionId: number, message: ClientMessage): Promise<EngineResponse[]> {
    switch (message.type) {
      case 'EVENT':
        return this.handleEvent(connectionId, message.event);
      case 'REQ':
        return this.handleReq(connectionId, message.subscriptionId, message.filters);
      case 'CLOSE':
        return this.processClose(connectionId, message.subscriptionId);
    }
  }

  validateEvent(event: Event): { ok: true } | { ok: false; id: string; error: string } {
    const ts = this.clock();
    if (isNwcKind(event.kind)) {
      try {
        verifyEvent(event, ts, this.limits);
        return { ok: true };
      } catch (e) {
        return { ok: false, id: event.id, error: (e as RelayError).message };
      }
    } else {
      return { ok: false, id: event.id, error: 'blocked: event kind not allowed' };
    }
  }

  async routeVerifiedEvent(connectionId: number, event: Event): Promise<EngineResponse[]> {
    if (event.kind === 13194) {
      await this.processInfoEvent(event);
    } else if (event.kind === 5) {
      await this.processDeletionEvent(event);
    }
    return this.routeEvent(connectionId, event);
  }

  private async handleEvent(connectionId: number, event: Event): Promise<EngineResponse[]> {
    const ts = this.clock();

    if (isNwcKind(event.kind)) {
      try {
        verifyEvent(event, ts, this.limits);
      } catch (e) {
        return [
          {
            kind: 'send',
            recipientId: connectionId,
            message: { type: 'OK', id: event.id, ok: false, message: (e as RelayError).message },
          },
        ];
      }

      if (event.kind === 13194) {
        await this.processInfoEvent(event);
        return this.processEvent(connectionId, event);
      } else if (event.kind === 5) {
        await this.processDeletionEvent(event);
        return this.processEvent(connectionId, event);
      } else {
        return this.processEvent(connectionId, event);
      }
    } else {
      let message: RelayMessage;
      try {
        verifyEvent(event, ts, this.limits);
        message = { type: 'OK', id: event.id, ok: false, message: 'blocked: event kind not allowed' };
      } catch (e) {
        message = { type: 'OK', id: event.id, ok: false, message: (e as RelayError).message };
      }
      return [{ kind: 'send', recipientId: connectionId, message }];
    }
  }

  async handleReq(id: number, subId: string, filters: Filter[]): Promise<EngineResponse[]> {
    if (filters.some(f => !filterIsValid(f))) {
      return [
        { kind: 'send', recipientId: id, message: { type: 'CLOSED', subscriptionId: subId, message: 'filter too broad' } },
      ];
    }
    return this.processReq(id, subId, filters);
  }

  async handleReqInternal(id: number, subId: string, filters: Filter[]): Promise<EngineResponse[]> {
    return this.processReq(id, subId, filters);
  }

  async onConnect(id: number): Promise<EngineResponse[]> {
    await this.addConnection(id, new Map());
    return [];
  }

  private async processInfoEvent(event: Event): Promise<void> {
    await this.registry.cacheInfo(event);
  }

  private async processDeletionEvent(event: Event): Promise<void> {
    const author = event.pubkey;
    const eTagIds: string[] = [];
    for (const tag of event.tags) {
      const eid = tag.eventId();
      if (eid !== null) eTagIds.push(eid);
    }

    if (eTagIds.length > 0) {
      for (const eventId of eTagIds) {
        const infoPk = this.registry.findInfoPubkeyById(eventId);
        if (infoPk && infoPk === author) {
          await this.registry.deleteInfo(infoPk);
        }
      }
    } else {
      const kTags: number[] = [];
      for (const tag of event.tags) {
        const kv = tag.kindValue();
        if (kv !== null) kTags.push(kv);
      }

      if (kTags.length === 0 || kTags.includes(13194)) {
        await this.registry.deleteInfo(author);
      }
    }
  }

  private async processEvent(connectionId: number, event: Event): Promise<EngineResponse[]> {
    const responses: EngineResponse[] = [
      { kind: 'send', recipientId: connectionId, message: { type: 'OK', id: event.id, ok: true, message: '' } },
    ];

    const registryResponses = await this.registry.matchEvent(event);
    for (const resp of registryResponses) {
      switch (resp.kind) {
        case 'send':
          responses.push({
            kind: 'send',
            recipientId: resp.recipientId,
            message: { type: 'EVENT', subscriptionId: resp.subId, event: event },
          });
          break;
        case 'wakeUp':
          responses.push({ kind: 'wakeUp', connectionId: resp.connectionId });
          break;
      }
    }

    return responses;
  }

  private async routeEvent(_connectionId: number, event: Event): Promise<EngineResponse[]> {
    const responses: EngineResponse[] = [];

    const registryResponses = await this.registry.matchEvent(event);
    for (const resp of registryResponses) {
      switch (resp.kind) {
        case 'send':
          responses.push({
            kind: 'send',
            recipientId: resp.recipientId,
            message: { type: 'EVENT', subscriptionId: resp.subId, event },
          });
          break;
        case 'wakeUp':
          responses.push({ kind: 'wakeUp', connectionId: resp.connectionId });
          break;
      }
    }

    return responses;
  }

  private async processReq(id: number, subId: string, filters: Filter[]): Promise<EngineResponse[]> {
    const responses: EngineResponse[] = [];

    try {
      await this.registry.subscribe(id, subId, filters);
    } catch (e) {
      return [
        { kind: 'send', recipientId: id, message: { type: 'CLOSED', subscriptionId: subId, message: (e as Error).message } },
      ];
    }

    const globalLimit = Math.min(...filters.map(f => f.limit).filter((l): l is number => l !== undefined));

    for (const filter of filters) {
      for (const pk of filterPubkeys(filter)) {
        const infoEvent = await this.registry.getInfo(pk);
        if (infoEvent && filters.some(f => filterMatches(f, infoEvent))) {
          responses.push({
            kind: 'send',
            recipientId: id,
            message: { type: 'EVENT', subscriptionId: subId, event: infoEvent },
          });
        }
      }
    }

    if (globalLimit !== undefined && globalLimit < Infinity) {
      const eventCount = responses.filter(
        (r): r is { kind: 'send'; recipientId: number; message: RelayMessage & { type: 'EVENT' } } =>
          r.kind === 'send' && r.message.type === 'EVENT',
      ).length;
      if (eventCount >= globalLimit) {
        responses.push({
          kind: 'send',
          recipientId: id,
          message: { type: 'EOSE', subscriptionId: subId },
        });
        return responses;
      }
    }

    responses.push({
      kind: 'send',
      recipientId: id,
      message: { type: 'EOSE', subscriptionId: subId },
    });
    return responses;
  }

  async processClose(id: number, subId: string): Promise<EngineResponse[]> {
    await this.registry.unsubscribe(id, subId);
    return [];
  }

  async onDisconnect(id: number): Promise<EngineResponse[]> {
    await this.registry.onDisconnect(id);
    return [];
  }

  async onTerminate(id: number): Promise<EngineResponse[]> {
    await this.registry.onTerminate(id);
    return [];
  }

  async getWalletInfo(pubkey: string): Promise<WalletInfo | null> {
    const event = await this.registry.getInfo(pubkey);
    if (!event) return null;
    return parseWalletInfo(event);
  }

  async addConnection(id: number, subscriptions: Map<string, Filter[]>): Promise<void> {
    for (const [subId, filters] of subscriptions) {
      try {
        await this.registry.subscribe(id, subId, filters);
      } catch {
        // skip failed subscriptions
      }
    }
  }

  async load(connId: number): Promise<boolean> {
    return this.registry.load(connId);
  }

  async loadByPubkey(pubkey: string): Promise<number[]> {
    return this.registry.loadByPubkey(pubkey);
  }
}
