import { WebSocket } from 'ws';
import * as nostr from 'nostr-tools';
import { finalizeEvent, generateSecretKey, getPublicKey } from 'nostr-tools/pure';

// Get URL from command line args or use default
const args = process.argv.slice(2);
let baseURL = args[0] || 'localhost:8787';

// Clean up the input URL (remove protocol if user provided it)
baseURL = baseURL.replace(/^https?:\/\//, '').replace(/^wss?:\/\//, '').replace(/\/$/, '');

const isLocal = baseURL.includes('localhost') || baseURL.includes('127.0.0.1');
const wsProtocol = isLocal ? 'ws://' : 'wss://';
const httpProtocol = isLocal ? 'http://' : 'https://';

const RELAY_URL = `${wsProtocol}${baseURL}/`;
const HTTP_URL = `${httpProtocol}${baseURL}/`;

console.log(`Testing against:`);
console.log(`  WebSocket: ${RELAY_URL}`);
console.log(`  HTTP:      ${HTTP_URL}\n`);

async function testNip11() {
  console.log('Testing NIP-11 (Relay Information)...');
  const response = await fetch(HTTP_URL, {
    headers: { 'Accept': 'application/nostr+json' }
  });
  const data = await response.json();
  if (data.name !== 'nwc-worker' || !data.supported_nips.includes(47)) {
    throw new Error('NIP-11 failed: ' + JSON.stringify(data));
  }
  console.log('✅ NIP-11 passed.');
}

async function testRelay() {
  const ws = new WebSocket(RELAY_URL);

  const sk = generateSecretKey();
  const pk = getPublicKey(sk);
  let eventKind3;

  return new Promise((resolve, reject) => {
    ws.on('open', async () => {
      console.log('Connected to relay.');

      // 1. Test Stateless REQ -> EOSE
      console.log('Testing Stateless REQ...');
      ws.send(JSON.stringify(['REQ', 'sub-1', { kinds: [23194], '#p': [pk] }]));

      // 2. Test Invalid Signature
      console.log('Testing Signature Rejection...');
      ws.send(JSON.stringify(['EVENT', {
        id: '0'.repeat(64),
        pubkey: pk,
        created_at: Math.floor(Date.now() / 1000),
        kind: 1,
        tags: [],
        content: 'invalid',
        sig: '0'.repeat(128)
      }]));

      // 3. Test P-Tag Enforcement for Kind 23194
      console.log('Testing P-tag Enforcement...');
      const eventNoPTag = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [],
        content: ''
      }, sk);
      ws.send(JSON.stringify(['EVENT', eventNoPTag]));

      // 4. Test Restricted Kinds (ensure kind not in [0, 1, 13194, 23194, 23195] is rejected)
      console.log('Testing Restricted Kinds (Kind 3)...');
      eventKind3 = finalizeEvent({
        kind: 3, // Contacts, not in allowed list
        created_at: Math.floor(Date.now() / 1000),
        tags: [],
        content: ''
      }, sk);
      ws.send(JSON.stringify(['EVENT', eventKind3]));
    });

    let eoseReceived = false;
    let sigRejected = false;
    let kind3Rejected = false;

    ws.on('message', (data) => {
      const msg = JSON.parse(data.toString());
      console.log('Received:', msg[0], msg[1] || '', msg[2] !== undefined ? msg[2] : '');

      if (msg[0] === 'EOSE' && msg[1] === 'sub-1') {
        eoseReceived = true;
        console.log('✅ Stateless REQ passed.');
      }

      if (msg[0] === 'OK') {
        if (msg[1] === eventKind3.id && msg[2] === false) {
           kind3Rejected = true;
           console.log('✅ Restricted kind rejection passed.');
        }

        if (msg[2] === false) {
          if (msg[3].includes('signature')) {
            sigRejected = true;
            console.log('✅ Signature rejection passed.');
          }
        }
      }

      // Check if all initial tests finished
      if (eoseReceived && sigRejected && kind3Rejected) {
          // Now test routing
          testRouting(ws, sk, pk).then(resolve).catch(reject);
      }
    });

    ws.on('error', reject);
    setTimeout(() => reject(new Error('Timeout')), 5000);
  });
}

async function testRouting(ws, sk, pk) {
    console.log('Testing Event Routing...');
    const event = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [['p', pk]],
        content: 'test routing'
    }, sk);

    ws.send(JSON.stringify(['EVENT', event]));

    return new Promise((resolve) => {
        ws.on('message', (data) => {
            const msg = JSON.parse(data.toString());
            if (msg[0] === 'EVENT' && msg[1] === 'sub-1') {
                if (msg[2].id === event.id) {
                    console.log('✅ Event routing passed.');
                    ws.close();
                    resolve();
                }
            }
        });
    });
}

async function testNwcFlow() {
    console.log('\n--- Testing Full NWC Flow (2 Clients: App & Wallet) ---');
    
    const walletWs = new WebSocket(RELAY_URL);
    const appWs = new WebSocket(RELAY_URL);

    const walletSk = generateSecretKey();
    const walletPk = getPublicKey(walletSk);

    const appSk = generateSecretKey();
    const appPk = getPublicKey(appSk);

    return new Promise((resolve, reject) => {
        let walletEose = false;
        let appEose = false;

        const checkStart = () => {
            if (walletEose && appEose) {
                startFlow();
            }
        };

        walletWs.on('open', () => {
            console.log('Wallet connected.');
            // Wallet subscribes to NWC Requests meant for its pubkey
            walletWs.send(JSON.stringify(['REQ', 'wallet-requests', { kinds: [23194], '#p': [walletPk] }]));
        });

        appWs.on('open', () => {
            console.log('App connected.');
            // App subscribes with two filters:
            // 1. Wallet Info (13194) from the Wallet author
            // 2. Responses (23195) and Notifications (23197) targeted to the App (#p)
            appWs.send(JSON.stringify(['REQ', 'app-subs', 
                { kinds: [13194], authors: [walletPk] },
                { kinds: [23195, 23197], '#p': [appPk] }
            ]));
        });

        let infoReceived = false;
        let requestReceived = false;
        let responseReceived = false;
        let notificationReceived = false;

        const startFlow = () => {
            console.log('Step 1: Wallet sending NWC Info (kind 13194)...');
            const infoEvent = finalizeEvent({
                kind: 13194,
                created_at: Math.floor(Date.now() / 1000),
                tags: [],
                content: 'supported_methods=pay_invoice'
            }, walletSk);
            walletWs.send(JSON.stringify(['EVENT', infoEvent]));
        };

        const sendRequest = () => {
            console.log('Step 2: App sending NWC Request (kind 23194) to Wallet...');
            const reqEvent = finalizeEvent({
                kind: 23194,
                created_at: Math.floor(Date.now() / 1000),
                tags: [['p', walletPk]],
                content: '{"method":"pay_invoice","params":{"invoice":"..."}}'
            }, appSk);
            appWs.send(JSON.stringify(['EVENT', reqEvent]));
        };

        walletWs.on('message', (data) => {
            const msg = JSON.parse(data.toString());
            if (msg[0] === 'EOSE' && msg[1] === 'wallet-requests') {
                walletEose = true;
                checkStart();
            }
            if (msg[0] === 'EVENT' && msg[1] === 'wallet-requests') {
                const event = msg[2];
                if (event.kind === 23194) {
                    console.log('✅ Wallet received Request (23194) from App.');
                    requestReceived = true;

                    console.log('Step 3: Wallet sending Response (23195) & Notification (23197)...');
                    
                    // Send Response
                    const resEvent = finalizeEvent({
                        kind: 23195,
                        created_at: Math.floor(Date.now() / 1000),
                        tags: [['p', appPk], ['e', event.id]],
                        content: '{"result":{"preimage":"..."}}'
                    }, walletSk);
                    walletWs.send(JSON.stringify(['EVENT', resEvent]));

                    // Send Notification
                    const notifyEvent = finalizeEvent({
                        kind: 23197,
                        created_at: Math.floor(Date.now() / 1000),
                        tags: [['p', appPk]],
                        content: '{"type":"payment_sent","payload":{"invoice":"..."}}'
                    }, walletSk);
                    walletWs.send(JSON.stringify(['EVENT', notifyEvent]));
                }
            }
        });

        appWs.on('message', (data) => {
            const msg = JSON.parse(data.toString());
            if (msg[0] === 'EOSE' && msg[1] === 'app-subs') {
                appEose = true;
                checkStart();
            }
            if (msg[0] === 'EVENT' && msg[1] === 'app-subs') {
                const event = msg[2];
                if (event.kind === 13194 && !infoReceived) {
                    console.log('✅ App received Wallet Info (13194).');
                    infoReceived = true;
                    // Proceed to send request after info is received
                    sendRequest();
                } else if (event.kind === 23195) {
                    console.log('✅ App received Response (23195).');
                    responseReceived = true;
                } else if (event.kind === 23197) {
                    console.log('✅ App received Notification (23197).');
                    notificationReceived = true;
                }

                if (infoReceived && requestReceived && responseReceived && notificationReceived) {
                    console.log('✅ All NWC Flow steps completed.');
                    walletWs.close();
                    appWs.close();
                    resolve();
                }
            }
        });

        setTimeout(() => {
            const status = `Eoses: W=${walletEose}, A=${appEose} | Info: ${infoReceived}, Req: ${requestReceived}, Res: ${responseReceived}, Notify: ${notificationReceived}`;
            reject(new Error(`NWC Flow Timeout. Status: ${status}`));
        }, 10000);
    });
}

async function runAll() {
    try {
        await testNip11();
        await testRelay();
        await testNwcFlow();
        console.log('\nAll tests passed successfully! 🚀');
        process.exit(0);
    } catch (err) {
        console.error('\n❌ Test failed:', err.message);
        process.exit(1);
    }
}

runAll();
