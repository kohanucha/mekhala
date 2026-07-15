import type { Event } from './event.ts';
import type { Filter } from './filter.ts';
import type { RelayMessage } from './nip01.ts';

export type RpcState =
  | { kind: 'initial' }
  | { kind: 'awaitingResponse' }
  | { kind: 'success'; event: Event }
  | { kind: 'failed'; reason: string };

export type RpcAction =
  | { kind: 'subscribe'; subId: string; filter: Filter }
  | { kind: 'publish'; event: Event }
  | { kind: 'unsubscribe'; subId: string };

export class NwcRpcMachine {
  private request: Event;
  private state: RpcState = { kind: 'initial' };
  private subId = 'rpc_sub';

  constructor(request: Event) {
    this.request = request;
  }

  getState(): RpcState {
    return this.state;
  }

  start(): RpcAction[] {
    const filter: Filter = {
      eTags: [this.request.id],
      pTags: [this.request.pubkey],
    };

    this.state = { kind: 'awaitingResponse' };

    return [
      { kind: 'subscribe', subId: this.subId, filter },
      { kind: 'publish', event: this.request },
    ];
  }

  transition(message: RelayMessage): RpcAction | null {
    if (this.state.kind !== 'awaitingResponse') return null;

    switch (message.type) {
      case 'EVENT': {
        const referencesRequest = message.event.tags.some(
          t => t.eventId() === this.request.id,
        );
        if (referencesRequest) {
          this.state = { kind: 'success', event: message.event };
          return { kind: 'unsubscribe', subId: this.subId };
        }
        return null;
      }
      case 'EOSE':
        return null;
      case 'NOTICE':
        this.state = { kind: 'failed', reason: `Relay notice: ${message.message}` };
        return { kind: 'unsubscribe', subId: this.subId };
      default:
        return null;
    }
  }
}
