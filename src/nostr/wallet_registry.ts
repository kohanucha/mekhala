import type { Event } from './event.ts';
import type { Filter } from './filter.ts';
import type { Limits } from './limits.ts';
import { filterMatches, filterPubkeys } from './filter.ts';
import { targetPubkeys } from './event.ts';
import { RelayError } from './error.ts';

export interface Storage {
  get(key: string): Promise<unknown | null>;
  putBatch(entries: Record<string, unknown>): Promise<void>;
  deleteBatch(keys: string[]): Promise<void>;
}

export interface SavedState {
  json: unknown;
  pubkeys: string[];
}

export type RegistryResponse =
  | { kind: 'send'; recipientId: number; subId: string }
  | { kind: 'wakeUp'; connectionId: number };

type PkEntry = { subs: Set<string>; info: Event | null };

function subKey(subId: string, filters: Filter[]): string {
  return subId + '::' + JSON.stringify(filters);
}

class WalletIndex {
  private subscriptionIndex = new Map<string, number[]>();
  private pkIndex = new Map<string, PkEntry>();
  private infoIdIndex = new Map<string, string>();
  private reverseIndex = new Map<number, Map<string, Filter[]>>();

  subscribe(connId: number, subId: string, filters: Filter[]): void {
    this.unsubscribe(connId, subId);

    const key = subKey(subId, filters);

    for (const filter of filters) {
      for (const pk of filterPubkeys(filter)) {
        let entry = this.pkIndex.get(pk);
        if (!entry) {
          entry = { subs: new Set(), info: null };
          this.pkIndex.set(pk, entry);
        }
        entry.subs.add(key);
      }
    }

    let conns = this.subscriptionIndex.get(key);
    if (!conns) {
      conns = [];
      this.subscriptionIndex.set(key, conns);
    }
    if (!conns.includes(connId)) {
      conns.push(connId);
    }

    let connSubs = this.reverseIndex.get(connId);
    if (!connSubs) {
      connSubs = new Map();
      this.reverseIndex.set(connId, connSubs);
    }
    connSubs.set(subId, filters);
  }

  unsubscribe(connId: number, subId: string): void {
    const connSubs = this.reverseIndex.get(connId);
    if (!connSubs) return;

    const filters = connSubs.get(subId);
    if (filters === undefined) return;
    connSubs.delete(subId);

    const key = subKey(subId, filters);
    const conns = this.subscriptionIndex.get(key);
    if (conns) {
      const idx = conns.indexOf(connId);
      if (idx !== -1) conns.splice(idx, 1);
      if (conns.length === 0) {
        this.subscriptionIndex.delete(key);
        for (const filter of filters) {
          for (const pk of filterPubkeys(filter)) {
            const entry = this.pkIndex.get(pk);
            if (entry) {
              entry.subs.delete(key);
              if (entry.subs.size === 0 && entry.info === null) {
                this.pkIndex.delete(pk);
              }
            }
          }
        }
      }
    }

    if (connSubs.size === 0) {
      this.reverseIndex.delete(connId);
    }
  }

  disconnect(connId: number): void {
    const connSubs = this.reverseIndex.get(connId);
    if (!connSubs) return;
    for (const subId of connSubs.keys()) {
      this.unsubscribe(connId, subId);
    }
  }

  getSubscriptions(connId: number): Map<string, Filter[]> {
    return new Map(this.reverseIndex.get(connId) ?? new Map());
  }

  subCount(connId: number): number {
    return this.reverseIndex.get(connId)?.size ?? 0;
  }

  cacheInfo(event: Event): void {
    let entry = this.pkIndex.get(event.pubkey);
    if (!entry) {
      entry = { subs: new Set(), info: null };
      this.pkIndex.set(event.pubkey, entry);
    }
    this.infoIdIndex.set(event.id, event.pubkey);
    entry.info = event;
  }

  getInfo(pubkey: string): Event | null {
    return this.pkIndex.get(pubkey)?.info ?? null;
  }

  deleteInfo(pubkey: string): Event | null {
    const entry = this.pkIndex.get(pubkey);
    if (!entry) return null;

    const old = entry.info;
    if (old) {
      this.infoIdIndex.delete(old.id);
    }
    entry.info = null;

    if (entry.subs.size === 0 && entry.info === null) {
      this.pkIndex.delete(pubkey);
    }

    return old;
  }

  findInfoPubkeyById(eventId: string): string | null {
    return this.infoIdIndex.get(eventId) ?? null;
  }

  matchEvent(event: Event): Map<string, number[]> {
    const targetPks = targetPubkeys(event);
    const subToConns = new Map<string, number[]>();

    for (const pk of targetPks) {
      const entry = this.pkIndex.get(pk);
      if (!entry) continue;

      const matching: Array<{ subId: string; connId: number }> = [];
      for (const key of entry.subs) {
        const sepIdx = key.indexOf('::');
        const subId = key.substring(0, sepIdx);
        const filtersJson = key.substring(sepIdx + 2);
        try {
          const filters = JSON.parse(filtersJson) as Filter[];
          if (filters.some(f => filterMatches(f, event))) {
            const conns = this.subscriptionIndex.get(key);
            if (conns && conns.length > 0) {
              const latestConn = conns[conns.length - 1];
              matching.push({ subId, connId: latestConn });
            }
          }
        } catch {
          // skip malformed filter JSON
        }
      }

      if (matching.length === 0) continue;
      const latestId = Math.max(...matching.map(m => m.connId));
      for (const m of matching) {
        if (m.connId === latestId) {
          subToConns.set(m.subId, [m.connId]);
        }
      }
    }

    return subToConns;
  }

  getConnectionId(pubkey: string): number | null {
    const entry = this.pkIndex.get(pubkey);
    if (!entry) return null;

    let latest: number | null = null;
    for (const key of entry.subs) {
      const conns = this.subscriptionIndex.get(key);
      if (conns && conns.length > 0) {
        const id = conns[conns.length - 1];
        if (latest === null || id > latest) {
          latest = id;
        }
      }
    }
    return latest;
  }

  save(connId: number): SavedState | null {
    const subscriptions = this.getSubscriptions(connId);
    if (subscriptions.size === 0) return null;

    let infoEvent: Event | null = null;
    const pubkeys: string[] = [];

    for (const filters of subscriptions.values()) {
      for (const filter of filters) {
        for (const pk of filterPubkeys(filter)) {
          if (!pubkeys.includes(pk)) pubkeys.push(pk);
          if (!infoEvent) {
            infoEvent = this.getInfo(pk);
          }
        }
      }
    }

    return {
      json: {
        subscriptions: Object.fromEntries(subscriptions),
        info_event: infoEvent,
      },
      pubkeys,
    };
  }

  restore(connId: number, data: Record<string, unknown>): void {
    const subsData = data.subscriptions;
    if (subsData && typeof subsData === 'object') {
      for (const [subId, filters] of Object.entries(subsData)) {
        if (Array.isArray(filters)) {
          this.subscribe(connId, subId, filters as Filter[]);
        }
      }
    }
    const infoData = data.info_event;
    if (infoData && typeof infoData === 'object') {
      this.cacheInfo(infoData as unknown as Event);
    }
  }
}

export class WalletRegistry<S extends Storage> {
  storage: S;
  private index = new WalletIndex();
  private limits: Limits;

  constructor(storage: S, limits: Limits) {
    this.storage = storage;
    this.limits = limits;
  }

  async subscribe(connId: number, subId: string, filters: Filter[]): Promise<void> {
    const count = this.index.subCount(connId);
    if (count >= this.limits.maxSubscriptionsPerConnection) {
      throw RelayError.Generic(
        `too many subscriptions: ${count} (max ${this.limits.maxSubscriptionsPerConnection})`,
      );
    }
    this.index.subscribe(connId, subId, filters);
    try {
      await this.sync(connId);
    } catch (e) {
      throw RelayError.Generic(`persist failed: ${(e as Error).message}`);
    }
  }

  async unsubscribe(connId: number, subId: string): Promise<void> {
    this.index.unsubscribe(connId, subId);
    try {
      await this.sync(connId);
    } catch (e) {
      // log error but don't propagate (graceful on storage failure)
    }
  }

  async matchEvent(event: Event): Promise<RegistryResponse[]> {
    const responses: RegistryResponse[] = [];
    const targetPks = targetPubkeys(event);

    for (const pk of targetPks) {
      for (const id of await this.loadByPubkey(pk)) {
        responses.push({ kind: 'wakeUp', connectionId: id });
      }
    }

    for (const [subId, conns] of this.index.matchEvent(event)) {
      for (const id of conns) {
        responses.push({ kind: 'send', recipientId: id, subId });
      }
    }

    return responses;
  }

  async load(connId: number): Promise<boolean> {
    if (this.index.getSubscriptions(connId).size > 0) return true;

    const key = `conn:${connId}`;
    const data = await this.storage.get(key);
    if (data && typeof data === 'object') {
      this.index.restore(connId, data as Record<string, unknown>);
      return true;
    }
    return false;
  }

  async loadByPubkey(pubkey: string): Promise<number[]> {
    const key = `pk:${pubkey}`;
    const val = await this.storage.get(key);

    let storageIds: number[];
    if (val !== null) {
      if (Array.isArray(val)) {
        storageIds = val.map(v => Number(v));
      } else if (typeof val === 'number') {
        storageIds = [val];
      } else {
        storageIds = [];
      }
    } else {
      const id = this.index.getConnectionId(pubkey);
      return id !== null ? [id] : [];
    }

    const loaded: number[] = [];
    const stale: number[] = [];
    for (const id of storageIds) {
      if (await this.load(id)) {
        loaded.push(id);
      } else {
        stale.push(id);
      }
    }

    if (loaded.length === 0) {
      await this.storage.deleteBatch([key]);
    } else if (stale.length > 0) {
      await this.storage.putBatch({ [key]: loaded });
    }

    return loaded;
  }

  async onDisconnect(id: number): Promise<void> {
    this.index.disconnect(id);
  }

  async onTerminate(id: number): Promise<void> {
    const subs = this.index.getSubscriptions(id);
    const pubkeys = new Set<string>();
    for (const filters of subs.values()) {
      for (const filter of filters) {
        for (const pk of filterPubkeys(filter)) {
          pubkeys.add(pk);
        }
      }
    }

    this.index.disconnect(id);

    for (const pk of pubkeys) {
      const key = `pk:${pk}`;
      const val = await this.storage.get(key);
      if (val !== null) {
        let ids: number[];
        if (Array.isArray(val)) {
          ids = val.map(v => Number(v));
        } else if (typeof val === 'number') {
          ids = [val];
        } else {
          ids = [];
        }
        const newIds = ids.filter(x => x !== id);
        if (newIds.length === 0) {
          await this.storage.deleteBatch([key]);
        } else {
          await this.storage.putBatch({ [key]: newIds });
        }
      }
    }

    await this.storage.deleteBatch([`conn:${id}`]);
  }

  async cacheInfo(event: Event): Promise<void> {
    const key = `info:${event.pubkey}`;
    await this.storage.putBatch({ [key]: event });
    this.index.cacheInfo(event);
  }

  async getInfo(pubkey: string): Promise<Event | null> {
    const cached = this.index.getInfo(pubkey);
    if (cached) return cached;

    const key = `info:${pubkey}`;
    const val = await this.storage.get(key);
    if (val && typeof val === 'object') {
      const event = val as Event;
      this.index.cacheInfo(event);
      return event;
    }
    return null;
  }

  async deleteInfo(pubkey: string): Promise<void> {
    const removed = this.index.deleteInfo(pubkey);
    if (removed) {
      const key = `info:${pubkey}`;
      await this.storage.deleteBatch([key]);
    }
  }

  findInfoPubkeyById(eventId: string): string | null {
    return this.index.findInfoPubkeyById(eventId);
  }

  // test helper: expose subscriptions for a connection
  getConnSubscriptions(connId: number): Map<string, Filter[]> {
    return this.index.getSubscriptions(connId);
  }

  private async sync(connId: number): Promise<void> {
    const state = this.index.save(connId);
    if (state) {
      const entries: Record<string, unknown> = {
        [`conn:${connId}`]: state.json,
      };
      for (const pk of state.pubkeys) {
        const key = `pk:${pk}`;
        const ids = await this.readPkList(key);
        if (!ids.includes(connId)) {
          ids.push(connId);
        }
        entries[key] = ids;
      }
      await this.storage.putBatch(entries);
    } else {
      await this.storage.deleteBatch([`conn:${connId}`]);
    }
  }

  private async readPkList(key: string): Promise<number[]> {
    const val = await this.storage.get(key);
    if (val !== null) {
      if (Array.isArray(val)) {
        return val.map(v => Number(v));
      } else if (typeof val === 'number') {
        return [val];
      }
    }
    return [];
  }
}
