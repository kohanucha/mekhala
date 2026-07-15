import type { UserStore } from '../common/mod.ts';

export class LnAddressHandler<S extends UserStore> {
  constructor(private store: S) {}

  async lookupUser(username: string): Promise<string | null> {
    return this.store.getNwcUri(username);
  }
}
