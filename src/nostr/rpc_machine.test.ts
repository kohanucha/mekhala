import { describe, it, expect } from 'vitest';
import { NwcRpcMachine } from './rpc_machine.ts';
import type { Event } from './event.ts';
import type { RelayMessage } from './nip01.ts';
import { Tag } from './tag.ts';

function mockEvent(id: string, pubkey: string): Event {
  return {
    id,
    pubkey,
    createdAt: 0,
    kind: 23194,
    tags: [],
    content: '',
    sig: '',
  };
}

describe('NwcRpcMachine', () => {
  it('full flow: start → EOSE ignored → response matched → success', () => {
    const req = mockEvent('req1', 'pk1');
    const machine = new NwcRpcMachine(req);

    const actions = machine.start();
    expect(actions.length).toBe(2);
    expect(actions[0].kind).toBe('subscribe');
    expect(actions[1].kind).toBe('publish');
    expect(machine.getState()).toEqual({ kind: 'awaitingResponse' });

    // EOSE should be ignored
    const eoseMsg: RelayMessage = { type: 'EOSE', subscriptionId: 'rpc_sub' };
    const eoseAction = machine.transition(eoseMsg);
    expect(eoseAction).toBeNull();
    expect(machine.getState()).toEqual({ kind: 'awaitingResponse' });

    // Matching response event
    const resp = mockEvent('resp1', 'pk2');
    resp.tags = [Tag.e('req1')];
    const eventMsg: RelayMessage = { type: 'EVENT', subscriptionId: 'rpc_sub', event: resp };
    const eventAction = machine.transition(eventMsg);

    expect(eventAction).toEqual({ kind: 'unsubscribe', subId: 'rpc_sub' });
    expect(machine.getState()).toEqual({ kind: 'success', event: resp });
  });

  it('ignores response with wrong e-tag', () => {
    const req = mockEvent('req1', 'pk1');
    const machine = new NwcRpcMachine(req);
    machine.start();

    const resp = mockEvent('resp1', 'pk2');
    resp.tags = [Tag.e('wrong_req')];
    const msg: RelayMessage = { type: 'EVENT', subscriptionId: 'rpc_sub', event: resp };
    const action = machine.transition(msg);

    expect(action).toBeNull();
    expect(machine.getState()).toEqual({ kind: 'awaitingResponse' });
  });

  it('ignores transitions before start', () => {
    const req = mockEvent('req1', 'pk1');
    const machine = new NwcRpcMachine(req);

    const msg: RelayMessage = { type: 'EOSE', subscriptionId: 'rpc_sub' };
    const action = machine.transition(msg);

    expect(action).toBeNull();
    expect(machine.getState()).toEqual({ kind: 'initial' });
  });

  it('transitions to failed on NOTICE', () => {
    const req = mockEvent('req1', 'pk1');
    const machine = new NwcRpcMachine(req);
    machine.start();

    const msg: RelayMessage = { type: 'NOTICE', message: 'rate limited' };
    const action = machine.transition(msg);

    expect(action).toEqual({ kind: 'unsubscribe', subId: 'rpc_sub' });
    expect(machine.getState()).toEqual({ kind: 'failed', reason: 'Relay notice: rate limited' });
  });
});
