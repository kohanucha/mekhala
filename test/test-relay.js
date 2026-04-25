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
  if (data.name !== 'nwc-edge-relay' || !data.supported_nips.includes(47)) {
    throw new Error('NIP-11 failed: ' + JSON.stringify(data));
  }
  console.log('✅ NIP-11 JSON metadata passed.');

  console.log('Testing NIP-11 (Plain HTTP fallback)...');
  const responsePlain = await fetch(HTTP_URL);
  const text = await responsePlain.text();
  if (!text.includes('nwc-edge-relay: Nostr Wallet Connect Relay')) {
    throw new Error('Plain HTTP fallback failed: ' + text);
  }
  console.log('✅ NIP-11 Plain HTTP fallback passed.');
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

      // 4. Test Restricted Kinds (ensure kind not in [0, 1, 13194, 23194, 23195, 23196, 23197] is rejected)
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
            // App subscribes to:
            // 1. Responses (23195) and Notifications (23197) targeted to the App (#p)
            appWs.send(JSON.stringify(['REQ', 'app-subs', 
                { kinds: [23195, 23196, 23197], '#p': [appPk] }
            ]));
        });

        let requestReceived = false;
        let responseReceived = false;
        let notificationReceived = false;

        const startFlow = () => {
            console.log('Step 1: App sending NWC Request (kind 23194) to Wallet...');
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
                if (event.kind === 23195) {
                    console.log('✅ App received Response (23195).');
                    responseReceived = true;
                } else if (event.kind === 23196 || event.kind === 23197) {
                    console.log(`✅ App received Notification (${event.kind}).`);
                    notificationReceived = true;
                }

                if (requestReceived && responseReceived && notificationReceived) {
                    console.log('✅ All NWC Flow steps completed.');
                    walletWs.close();
                    appWs.close();
                    resolve();
                }
            }
        });

        setTimeout(() => {
            const status = `Eoses: W=${walletEose}, A=${appEose} | Req: ${requestReceived}, Res: ${responseReceived}, Notify: ${notificationReceived}`;
            reject(new Error(`NWC Flow Timeout. Status: ${status}`));
        }, 10000);
    });
}

async function testStatelessInfoEvent() {
    console.log('\n--- Testing NIP-47 Info Event (Stateless Routing) ---');
    
    const walletSk = generateSecretKey();
    const walletPk = getPublicKey(walletSk);

    const appWs = new WebSocket(RELAY_URL);
    const walletWs = new WebSocket(RELAY_URL);

    return new Promise((resolve, reject) => {
        let infoReceived = false;

        appWs.on('open', () => {
            console.log('App connected and subscribing to Info Event...');
            appWs.send(JSON.stringify(['REQ', 'info-sub', { kinds: [13194], authors: [walletPk] }]));
        });

        appWs.on('message', (data) => {
            const msg = JSON.parse(data.toString());
            if (msg[0] === 'EVENT' && msg[1] === 'info-sub') {
                if (msg[2].kind === 13194 && msg[2].pubkey === walletPk) {
                    console.log('✅ App received live Info Event (13194) routing.');
                    infoReceived = true;
                    appWs.close();
                    walletWs.close();
                    resolve();
                }
            }
        });

        walletWs.on('open', () => {
            // Wait a bit to ensure App's REQ is processed
            setTimeout(() => {
                console.log('Wallet publishing Info Event...');
                const infoEvent = finalizeEvent({
                    kind: 13194,
                    created_at: Math.floor(Date.now() / 1000),
                    tags: [],
                    content: 'stateless_routing_test'
                }, walletSk);
                walletWs.send(JSON.stringify(['EVENT', infoEvent]));
            }, 500);
        });

        appWs.on('error', reject);
        walletWs.on('error', reject);
        setTimeout(() => reject(new Error('Stateless Info Event Timeout')), 10000);
    });
}

async function testStrictValidation() {
    console.log('\n--- Testing Strict NIP-47 Validation ---');
    const ws = new WebSocket(RELAY_URL);

    return new Promise((resolve, reject) => {
        const sk = generateSecretKey();
        const pk = getPublicKey(sk);
        
        ws.on('open', () => {
            // 1. Test Broad REQ rejection
            console.log('Step 1: Testing rejection of broad NIP-47 REQ...');
            ws.send(JSON.stringify(['REQ', 'broad-sub', { kinds: [23194] }]));

            // 2. Test kind 23195 missing tags
            console.log('Step 2: Testing rejection of kind 23195 missing tags...');
            const invalid23195_noE = finalizeEvent({ kind: 23195, created_at: Math.floor(Date.now() / 1000), tags: [['p', pk]], content: '' }, sk);
            const invalid23195_noP = finalizeEvent({ kind: 23195, created_at: Math.floor(Date.now() / 1000), tags: [['e', '123']], content: '' }, sk);
            ws.send(JSON.stringify(['EVENT', invalid23195_noE]));
            ws.send(JSON.stringify(['EVENT', invalid23195_noP]));

            // 3. Test kind 23196/23197 missing p-tag
            console.log('Step 3: Testing rejection of kind 23196/23197 missing p-tag...');
            const invalid23196 = finalizeEvent({ kind: 23196, created_at: Math.floor(Date.now() / 1000), tags: [], content: '' }, sk);
            const invalid23197 = finalizeEvent({ kind: 23197, created_at: Math.floor(Date.now() / 1000), tags: [], content: '' }, sk);
            ws.send(JSON.stringify(['EVENT', invalid23196]));
            ws.send(JSON.stringify(['EVENT', invalid23197]));
        });

        let broadRejected = false;
        let rejectionsCount = 0;

        ws.on('message', (data) => {
            const msg = JSON.parse(data.toString());
            
            if (msg[0] === 'CLOSED' && msg[1] === 'broad-sub') {
                console.log('✅ Broad REQ rejected.');
                broadRejected = true;
            }

            if (msg[0] === 'OK' && msg[2] === false) {
                rejectionsCount++;
            }

            if (broadRejected && rejectionsCount === 4) {
                console.log('✅ All malformed NIP-47 events rejected.');
                ws.close();
                resolve();
            }
        });

        setTimeout(() => {
            reject(new Error(`Strict Validation Timeout. Broad: ${broadRejected}, Rejections: ${rejectionsCount}/4`));
        }, 5000);
    });
}

async function testMultiClientIsolation() {
    console.log('\n--- Testing Multi-Client Isolation (Cross-Talk Prevention) ---');
    
    const w1Ws = new WebSocket(RELAY_URL);
    const w2Ws = new WebSocket(RELAY_URL);
    const a1Ws = new WebSocket(RELAY_URL);

    const w1Sk = generateSecretKey(); const w1Pk = getPublicKey(w1Sk);
    const w2Sk = generateSecretKey(); const w2Pk = getPublicKey(w2Sk);
    const a1Sk = generateSecretKey(); const a1Pk = getPublicKey(a1Sk);

    return new Promise((resolve, reject) => {
        let w1Ready = false, w2Ready = false, a1Ready = false;
        
        const checkReady = () => {
            if (w1Ready && w2Ready && a1Ready) startTest();
        }

        w1Ws.on('open', () => { w1Ws.send(JSON.stringify(['REQ', 'w1', { kinds: [23194], '#p': [w1Pk] }])); });
        w2Ws.on('open', () => { w2Ws.send(JSON.stringify(['REQ', 'w2', { kinds: [23194], '#p': [w2Pk] }])); });
        a1Ws.on('open', () => { a1Ready = true; checkReady(); });

        let w1Received = false;
        let w2Received = false;

        w1Ws.on('message', (data) => { 
            const msg = JSON.parse(data.toString());
            if (msg[0] === 'EOSE') { w1Ready = true; checkReady(); } 
            if (msg[0] === 'EVENT') w1Received = true;
        });
        
        w2Ws.on('message', (data) => { 
            const msg = JSON.parse(data.toString());
            if (msg[0] === 'EOSE') { w2Ready = true; checkReady(); } 
            if (msg[0] === 'EVENT') w2Received = true;
        });

        const startTest = () => {
            console.log('App 1 sending Request to Wallet 1...');
            const req = finalizeEvent({
                kind: 23194,
                created_at: Math.floor(Date.now() / 1000),
                tags: [['p', w1Pk]],
                content: 'test'
            }, a1Sk);
            a1Ws.send(JSON.stringify(['EVENT', req]));

            setTimeout(() => {
                if (w1Received && !w2Received) {
                    console.log('✅ Multi-Client Isolation passed. Wallet 2 did not receive Wallet 1\'s request.');
                    w1Ws.close(); w2Ws.close(); a1Ws.close();
                    resolve();
                } else {
                    reject(new Error(`Isolation failed. W1: ${w1Received}, W2: ${w2Received}`));
                }
            }, 1000);
        };
        
        setTimeout(() => reject(new Error('Isolation test timeout')), 5000);
    });
}

async function testNip01EdgeCases() {
    console.log('\n--- Testing NIP-01 Edge Cases ---');
    const ws = new WebSocket(RELAY_URL);
    const sk = generateSecretKey();
    const pk = getPublicKey(sk);

    return new Promise((resolve, reject) => {
        ws.on('open', () => {
            // 1. ID Mismatch
            console.log('Step 1: Testing ID mismatch...');
            const event = finalizeEvent({ kind: 1, created_at: Math.floor(Date.now() / 1000), tags: [], content: 'test' }, sk);
            event.id = '0'.repeat(64); // Tamper with ID
            ws.send(JSON.stringify(['EVENT', event]));

            // 2. Malformed JSON
            console.log('Step 2: Testing malformed JSON...');
            ws.send('not a json array');

            // 3. Message too large
            console.log('Step 3: Testing message size limit...');
            const largeContent = 'a'.repeat(70000);
            ws.send(JSON.stringify(['EVENT', { content: largeContent }]));

            // 4. Multiple filters in REQ
            console.log('Step 4: Testing multiple filters in REQ...');
            ws.send(JSON.stringify(['REQ', 'multi-sub', { kinds: [1], authors: [pk] }, { kinds: [13194] }]));
        });

        let idMismatchRejected = false;
        let largeMessageNotice = false;
        let multiSubEose = false;
        let closeTestPassed = false;

        ws.on('message', (data) => {
            const msg = JSON.parse(data.toString());

            if (msg[0] === 'OK' && msg[2] === false && msg[3]?.includes('id')) {
                console.log('✅ ID mismatch rejected.');
                idMismatchRejected = true;
            }

            if (msg[0] === 'NOTICE' && msg[1]?.includes('too large')) {
                console.log('✅ Large message notice received.');
                largeMessageNotice = true;
            }

            if (msg[0] === 'EOSE' && msg[1] === 'multi-sub') {
                console.log('✅ Multiple filters REQ accepted.');
                multiSubEose = true;

                // 5. Test CLOSE
                console.log('Step 5: Testing CLOSE functionality...');
                ws.send(JSON.stringify(['CLOSE', 'multi-sub']));
                
                // Publish an event that would have matched
                const matchEvent = finalizeEvent({ kind: 1, created_at: Math.floor(Date.now() / 1000), tags: [], content: 'after close' }, sk);
                ws.send(JSON.stringify(['EVENT', matchEvent]));

                // Wait a bit to ensure NO event is received
                setTimeout(() => {
                    closeTestPassed = true;
                    checkDone();
                }, 1000);
            }

            if (msg[0] === 'EVENT' && msg[1] === 'multi-sub') {
                reject(new Error('Received event for closed subscription!'));
            }

            checkDone();
        });

        const checkDone = () => {
            if (idMismatchRejected && largeMessageNotice && multiSubEose && closeTestPassed) {
                ws.close();
                resolve();
            }
        };

        setTimeout(() => {
            reject(new Error(`NIP-01 Edge Cases Timeout. ID: ${idMismatchRejected}, Large: ${largeMessageNotice}, Multi: ${multiSubEose}, Close: ${closeTestPassed}`));
        }, 10000);
    });
}

async function runAll() {
    try {
        await testNip11();
        await testRelay();
        await testNip01EdgeCases();
        await testStrictValidation();
        await testNwcFlow();
        await testMultiClientIsolation();
        await testStatelessInfoEvent();
        console.log('\nAll tests passed successfully! 🚀');
        process.exit(0);
    } catch (err) {
        console.error('\n❌ Test failed:', err.message);
        process.exit(1);
    }
}

runAll();
