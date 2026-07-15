export type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue };

export class Tag {
  private constructor(private raw: JsonValue[]) {}

  static p(pubkey: string, extras: JsonValue[] = []): Tag {
    return new Tag(['p', pubkey, ...extras]);
  }

  static e(eventId: string, extras: JsonValue[] = []): Tag {
    return new Tag(['e', eventId, ...extras]);
  }

  static encryption(scheme: string): Tag {
    return new Tag(['encryption', scheme]);
  }

  static expiration(ts: number | string): Tag {
    const val = typeof ts === 'number' ? String(ts) : ts;
    return new Tag(['expiration', val]);
  }

  static other(name: string, values: JsonValue[] = []): Tag {
    return new Tag([name, ...values]);
  }

  static fromJSON(arr: JsonValue[]): Tag {
    const name = arr[0];
    if (name === 'expiration' && typeof arr[1] === 'number') {
      const copy = [arr[0], String(arr[1]), ...arr.slice(2)];
      return new Tag(copy);
    }
    return new Tag(arr);
  }

  toJSON(): JsonValue[] {
    return this.raw;
  }

  equals(other: Tag): boolean {
    if (this.raw.length !== other.raw.length) return false;
    return JSON.stringify(this.raw) === JSON.stringify(other.raw);
  }

  isP(): boolean {
    return this.raw[0] === 'p';
  }

  isE(): boolean {
    return this.raw[0] === 'e';
  }

  pubkey(): string | null {
    if (this.raw[0] === 'p' && typeof this.raw[1] === 'string') {
      return this.raw[1] as string;
    }
    return null;
  }

  eventId(): string | null {
    if (this.raw[0] === 'e' && typeof this.raw[1] === 'string') {
      return this.raw[1] as string;
    }
    return null;
  }

  encryptionScheme(): string | null {
    if (this.raw[0] === 'encryption' && typeof this.raw[1] === 'string') {
      return this.raw[1] as string;
    }
    return null;
  }

  kindValue(): number | null {
    if (this.raw[0] === 'k') {
      const v = this.raw[1];
      if (typeof v === 'number') return v;
      if (typeof v === 'string') {
        const n = parseInt(v, 10);
        if (!isNaN(n)) return n;
      }
    }
    return null;
  }

  getRaw(): JsonValue[] {
    return this.raw;
  }
}

export function tagsArrayFromJSON(arrs: JsonValue[][]): Tag[] {
  return arrs.map(a => Tag.fromJSON(a));
}
