import type { NwcTransport } from '../common/index.ts';
import { NwcError } from '../common/index.ts';
import { parseNwcUri, NwcClient, EncryptionMethod, type NwcResponse, type NwcMethod } from '../nostr/index.ts';

export class NwcSession {
  private transport: NwcTransport;
  private client: NwcClient;

  constructor(transport: NwcTransport, nwcUri: string) {
    const uri = parseNwcUri(nwcUri);
    this.client = new NwcClient(uri);
    this.transport = transport;
  }

  async makeInvoice(amountMsat: number, descriptionHash: string): Promise<string> {
    const params = { amount: amountMsat, description_hash: descriptionHash };
    const result = await this.call('make_invoice', params);
    const invoice = result.invoice;
    if (typeof invoice !== 'string') {
      throw NwcError.protocolError('Missing invoice in response');
    }
    return invoice;
  }

  async call(method: NwcMethod, params: Record<string, unknown>): Promise<Record<string, unknown>> {
    const client = await this.negotiateEncryption();
    const { event, requestId } = await client.createRequestEvent(method, params, []);

    const respEvent = await this.transport.executeNwcRpc(event);

    const respJson = await client.parseResponseEvent(respEvent, requestId) as Record<string, unknown>;

    const response = respJson as unknown as NwcResponse;

    if (response.error) {
      throw NwcError.rpcError(response.error.code, response.error.message);
    }

    if (response.result == null) {
      throw NwcError.protocolError('NWC response missing result and error');
    }

    return response.result as Record<string, unknown>;
  }

  private async negotiateEncryption(): Promise<NwcClient> {
    const info = await this.transport.getWalletInfo(this.client.walletPubkey);
    if (info == null) {
      throw NwcError.walletNotFound();
    }

    if (info.encryptionAlgorithms.includes(EncryptionMethod.Nip44)) {
      this.client.encryptionMethod = EncryptionMethod.Nip44;
    } else {
      this.client.encryptionMethod = EncryptionMethod.Nip04;
    }

    return this.client;
  }
}
