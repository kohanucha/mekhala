export interface WebSocketHandle {
  send(data: string): void;
  serializeAttachment(id: number): void;
  deserializeAttachment(): number | null;
}

type ConnectionKind =
  | { kind: 'external'; ws: WebSocketHandle }
  | { kind: 'internal'; resolve: (value: string) => void };

export class ConnectionRegistry {
  private connections = new Map<number, ConnectionKind>();

  addExternal(id: number, ws: WebSocketHandle): void {
    ws.serializeAttachment(id);
    this.connections.set(id, { kind: 'external', ws });
  }

  addInternal(id: number): Promise<string> {
    return new Promise(resolve => {
      this.connections.set(id, { kind: 'internal', resolve });
    });
  }

  identify(ws: WebSocketHandle): number | null {
    for (const [id, conn] of this.connections) {
      if (conn.kind === 'external' && conn.ws === ws) {
        return id;
      }
    }
    const id = ws.deserializeAttachment();
    if (id != null) {
      this.connections.set(id, { kind: 'external', ws });
    }
    return id;
  }

  findWsById(id: number): WebSocketHandle | null {
    const conn = this.connections.get(id);
    if (conn?.kind === 'external') return conn.ws;
    return null;
  }

  send(id: number, message: string): boolean {
    const conn = this.connections.get(id);
    if (!conn) return false;

    switch (conn.kind) {
      case 'external':
        try {
          conn.ws.send(message);
        } catch {
          // send error ignored — connection might be closed
        }
        return true;
      case 'internal':
        conn.resolve(message);
        this.connections.delete(id);
        return true;
    }
  }

  remove(id: number): void {
    this.connections.delete(id);
  }

  get size(): number {
    return this.connections.size;
  }
}
