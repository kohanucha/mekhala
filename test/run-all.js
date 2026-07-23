import { isLocal } from "./env.js";

import { testWebSocketMessageDelivery } from "./ws-connect.test.js";
import { testInfoEventRetrieval, testInfoEventRetrievalViaPTag, testRealisticNwcFlow, testRealisticPaymentFlow, testInfoEventTagWireFormat } from "./info-event.test.js";
import { testAuth, testAuthHeaders, testNip11, testCorsAndHeaders } from "./basic.test.js";
import { testRelay, testNwcFlow, testStrictValidation, testMultiClientIsolation, testEdgeCases, testLastInWinsRouting, testMaxConnections } from "./nwc-flow.test.js";
import { testNip01EdgeCases, testFilterMatching, testFilterMatchingAdvanced, testMixedValidInvalidFilters, testFilterLimit } from "./nip01-filters.test.js";
import { testNip09Deletion, testNip09KTagDeletion, testNip09BlanketDeletion, testNip09UnauthorizedDeletion } from "./nip09-deletion.test.js";
import { testMalformedEventProducesOK, testOversizedEventProducesOK, testTimestampValidation, testProtocolErrors, testLimitEnforcement } from "./event-validation.test.js";
import { testNip44AndFallback, testInfoEventCaching, testKind13194OK } from "./nip44.test.js";
import { testLnAddressFlow, testLnAddressOffline, testLnAddressErrors } from "./lnaddress.test.js";
import { testCloseNonExistentSub, testReplaceSubscription, testDuplicateEventPublish } from "./subscription-edge-cases.test.js";

async function runAll() {
  try {
    await testWebSocketMessageDelivery();
    await testInfoEventRetrieval();
    await testInfoEventRetrievalViaPTag();
    await testInfoEventTagWireFormat();
    await testAuth();
    await testAuthHeaders();
    await testNip11();
    await testRelay();
    await testNip01EdgeCases();
    await testCloseNonExistentSub();
    await testStrictValidation();
    await testNwcFlow();
    await testMultiClientIsolation();
    await testInfoEventCaching();
    await testRealisticNwcFlow();
    await testNip09Deletion();
    await testNip09KTagDeletion();
    await testNip09BlanketDeletion();
    await testNip09UnauthorizedDeletion();
    await testReplaceSubscription();
    await testTimestampValidation();
    await testEdgeCases();
    await testDuplicateEventPublish();
    await testRealisticPaymentFlow();
    if (isLocal) {
      await testLnAddressFlow();
      await testLnAddressOffline();
      await testNip44AndFallback();
    } else {
      console.log("Skipping LN Address and NIP-44 tests (remote — no local KV)...");
    }
    await testLastInWinsRouting();
    await testProtocolErrors();
    await testLimitEnforcement();
    await testKind13194OK();
    await testMalformedEventProducesOK();
    await testOversizedEventProducesOK();
    if (isLocal) {
      await testLnAddressErrors();
    } else {
      console.log("Skipping LN Address Error tests (remote — no local KV)...");
    }
    await testFilterMatching();
    await testFilterMatchingAdvanced();
    await testMixedValidInvalidFilters();
    await testFilterLimit();
    await testCorsAndHeaders();
    if (isLocal) {
      await testMaxConnections();
    } else {
      console.log("Skipping Max Connections test (remote — spawns local wrangler)...");
    }
    console.log("\nAll tests passed successfully! 🚀");
    process.exit(0);
  } catch (err) {
    console.error("\n❌ Test failed:", err.message);
    process.exit(1);
  }
}

runAll();
