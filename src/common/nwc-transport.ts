import type { Event, WalletInfo } from '../nostr/index.ts';

export interface NwcTransport {
  getWalletInfo(pubkey: string): Promise<WalletInfo | null>;
  executeNwcRpc(request: Event): Promise<Event>;
}

export interface UserStore {
  getNwcUri(username: string): Promise<string | null>;
}
