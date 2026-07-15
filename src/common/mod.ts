import type { Event } from '../nostr/event.ts';
import type { WalletInfo } from '../nostr/nip47.ts';

export class NwcError extends Error {
  readonly kind: string;

  private constructor(kind: string, message: string) {
    super(message);
    this.name = 'NwcError';
    this.kind = kind;
  }

  static walletNotFound(): NwcError {
    return new NwcError('WalletNotFound', 'Wallet not connected');
  }

  static timeout(): NwcError {
    return new NwcError('Timeout', 'NWC RPC timeout');
  }

  static protocolError(msg: string): NwcError {
    return new NwcError('ProtocolError', msg);
  }

  static rpcError(code: string, message: string): NwcError {
    return new NwcError('RpcError', `NWC Error (${code}): ${message}`);
  }

  static fromRelayError(e: Error): NwcError {
    return NwcError.protocolError(e.message);
  }

  static fromNwcUriError(e: Error): NwcError {
    return NwcError.protocolError(e.message);
  }

  static fromJsonError(e: unknown): NwcError {
    const msg = e instanceof Error ? e.message : String(e);
    return NwcError.protocolError(msg);
  }
}

export interface NwcTransport {
  getWalletInfo(pubkey: string): Promise<WalletInfo | null>;
  executeNwcRpc(request: Event): Promise<Event>;
}

export interface UserStore {
  getNwcUri(username: string): Promise<string | null>;
}
