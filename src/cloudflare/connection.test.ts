import { describe, it, expect } from 'vitest';
import { ConnectionRegistry } from './connection.ts';
import type { WebSocketHandle } from './connection.ts';

function mockWs(): WebSocketHandle {
  let attachment: number | null = null;
  return {
    send(_data: string) { void _data; },
    serializeAttachment(id: number) { attachment = id; },
    deserializeAttachment() { return attachment; },
  };
}

describe('ConnectionRegistry', () => {
  it('sends to internal and delivers', async () => {
    const reg = new ConnectionRegistry();
    const promise = reg.addInternal(1);
    const sent = reg.send(1, 'hello');
    expect(sent).toBe(true);
    expect(await promise).toBe('hello');
  });

  it('returns false for unknown id', () => {
    const reg = new ConnectionRegistry();
    expect(reg.send(42, 'msg')).toBe(false);
  });

  it('remove drops internal connection', async () => {
    const reg = new ConnectionRegistry();
    const promise = reg.addInternal(1);
    reg.remove(1);
    expect(reg.send(1, 'msg')).toBe(false);
    // The internal promise should never resolve
    let resolved = false;
    await Promise.race([
      promise.then(v => { resolved = true; return v; }),
      new Promise<string>(r => setTimeout(() => { r('timeout'); }, 10)),
    ]);
    expect(resolved).toBe(false);
  });

  it('internal send consumes entry', () => {
    const reg = new ConnectionRegistry();
    void reg.addInternal(1);
    expect(reg.send(1, 'first')).toBe(true);
    expect(reg.send(1, 'second')).toBe(false);
  });

  it('new registry has size 0', () => {
    const reg = new ConnectionRegistry();
    expect(reg.size).toBe(0);
  });

  it('external send does not remove entry', () => {
    const reg = new ConnectionRegistry();
    reg.addExternal(1, mockWs());
    expect(reg.send(1, 'msg')).toBe(true);
    expect(reg.size).toBe(1);
  });

  it('identify by reference', () => {
    const reg = new ConnectionRegistry();
    const ws = mockWs();
    reg.addExternal(1, ws);
    expect(reg.identify(ws)).toBe(1);
  });

  it('identify recovers from hibernation attachment', () => {
    const reg = new ConnectionRegistry();
    const ws = mockWs();
    ws.serializeAttachment(42);
    expect(reg.identify(ws)).toBe(42);
  });

  it('identify returns null for unknown ws', () => {
    const reg = new ConnectionRegistry();
    expect(reg.identify(mockWs())).toBeNull();
  });

  it('findWsById returns ws for external', () => {
    const reg = new ConnectionRegistry();
    const ws = mockWs();
    reg.addExternal(7, ws);
    expect(reg.findWsById(7)).toBe(ws);
  });

  it('findWsById returns null for internal', () => {
    const reg = new ConnectionRegistry();
    void reg.addInternal(5);
    expect(reg.findWsById(5)).toBeNull();
  });

  it('findWsById returns null for unknown', () => {
    const reg = new ConnectionRegistry();
    expect(reg.findWsById(99)).toBeNull();
  });
});
