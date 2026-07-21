import { describe, it, expect } from 'vitest';
import { NwcSession } from './wallet-connector.ts';
import { MockTransport, TEST_NWC_URI, TEST_WALLET_SK } from '../common/test-helpers.ts';
import { NwcClient, parseNwcUri, EncryptionMethod, type WalletInfo } from '../nostr/index.ts';

describe('NwcSession', () => {
  it('makeInvoice with NIP-04', async () => {
    const walletInfo: WalletInfo = { encryptionAlgorithms: [EncryptionMethod.Nip04] };

    const uriObj = parseNwcUri(TEST_NWC_URI);
    const appClient = new NwcClient(uriObj);
    const walletUri: import('../nostr/nip47.ts').NwcUri = {
      walletPubkey: appClient.myPubkey,
      secret: TEST_WALLET_SK,
    };

    const transport = new MockTransport();
    transport.walletInfo = walletInfo;
    transport.walletUri = walletUri;

    const session = new NwcSession(transport, TEST_NWC_URI);
    const result = await session.makeInvoice(1000, 'hash');

    expect(result).toBe('lnbc1test');
  });

  it('makeInvoice with NIP-44', async () => {
    const walletInfo: WalletInfo = { encryptionAlgorithms: [EncryptionMethod.Nip44] };

    const uriObj = parseNwcUri(TEST_NWC_URI);
    const appClient = new NwcClient(uriObj);
    const walletUri: import('../nostr/nip47.ts').NwcUri = {
      walletPubkey: appClient.myPubkey,
      secret: TEST_WALLET_SK,
    };

    const transport = new MockTransport();
    transport.walletInfo = walletInfo;
    transport.walletUri = walletUri;

    const session = new NwcSession(transport, TEST_NWC_URI);
    const result = await session.makeInvoice(1000, 'hash');

    expect(result).toBe('lnbc1test');
  });

  it('handles wallet error', async () => {
    const walletInfo: WalletInfo = { encryptionAlgorithms: [EncryptionMethod.Nip04] };

    const uriObj = parseNwcUri(TEST_NWC_URI);
    const appClient = new NwcClient(uriObj);
    const walletUri: import('../nostr/nip47.ts').NwcUri = {
      walletPubkey: appClient.myPubkey,
      secret: TEST_WALLET_SK,
    };

    const transport = new MockTransport();
    transport.walletInfo = walletInfo;
    transport.walletUri = walletUri;
    transport.errorCode = 'INSUFFICIENT_BALANCE';

    const session = new NwcSession(transport, TEST_NWC_URI);
    await expect(session.makeInvoice(1000, 'hash')).rejects.toThrow('INSUFFICIENT_BALANCE');
  });

  it('throws wallet not found', async () => {
    const transport = MockTransport.walletNotFound();
    const session = new NwcSession(transport, TEST_NWC_URI);
    await expect(session.makeInvoice(1000, 'hash')).rejects.toThrow('Wallet not connected');
  });
});
