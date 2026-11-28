import { isLocal } from "./env.js";

import { testWebSocketMessageDelivery } from "./ws-connect.test.js";
import { testInfoEventRetrieval, testAppAliasInfoNotReplayed, testRealisticNwcFlow, testRealisticPaymentFlow } from "./info-event.test.js";
import { testAuth, testAuthHeaders, testAuthModes, testNip11, testCorsAndHeaders } from "./basic.test.js";
import { testRelay, testNwcFlow, testStrictValidation, testMultiClientIsolation, testEdgeCases, testMultiConnectionRouting, testMaxConnections } from "./nwc-flow.test.js";
import { testNip01EdgeCases, testFilterMatching, testFilterMatchingAdvanced, testMixedValidInvalidFilters, testFilterLimit } from "./nip01-filters.test.js";
import { testNip09Deletion, testNip09KTagDeletion, testNip09BlanketDeletion, testNip09UnauthorizedDeletion } from "./nip09-deletion.test.js";
import { testMalformedEventProducesOK, testOversizedEventProducesOK, testOversizedInfoEventRejected, testTagsKeptAsSigned, testTimestampValidation, testProtocolErrors, testLimitEnforcement } from "./event-validation.test.js";
import { testInfoEventCaching, testKind13194OK, testInfoEventNotRolledBack } from "./info-event-publish.test.js";
import { testCloseNonExistentSub, testReplaceSubscription, testDuplicateEventPublish, testSubscriptionBeyondOneStorageBatch, testRefusedReqShapes } from "./subscription-edge-cases.test.js";
import { testPerAddressLimits, testIdleConnectionSweep, testConnectionEviction, testMessageBudget, testOrphanedConnectionCleanup } from "./connection-limits.test.js";
import { runSdkConformance } from "./conformance.test.js";
import { testRestartPersistence } from "./restart-persistence.test.js";
import { testInfoCacheCap, testInfoCacheCapAcrossPages, testInfoCacheExpiry } from "./info-cache.test.js";

async function runAll() {
  try {
    await testWebSocketMessageDelivery();
    await testInfoEventRetrieval();
    await testAppAliasInfoNotReplayed();
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
    await testSubscriptionBeyondOneStorageBatch();
    await testRefusedReqShapes();
    await testRealisticPaymentFlow();
    await runSdkConformance();
    await testMultiConnectionRouting();
    await testProtocolErrors();
    await testLimitEnforcement();
    await testKind13194OK();
    await testInfoEventNotRolledBack();
    await testMalformedEventProducesOK();
    await testOversizedEventProducesOK();
    await testOversizedInfoEventRejected();
    await testTagsKeptAsSigned();
    await testFilterMatching();
    await testFilterMatchingAdvanced();
    await testMixedValidInvalidFilters();
    await testFilterLimit();
    await testCorsAndHeaders();
    if (isLocal) {
      await testMaxConnections();
      await testPerAddressLimits();
      await testIdleConnectionSweep();
      await testConnectionEviction();
      await testMessageBudget();
      await testOrphanedConnectionCleanup();
      await testAuthModes();
      await testRestartPersistence();
      await testInfoCacheCap();
      await testInfoCacheCapAcrossPages();
      await testInfoCacheExpiry();
    } else {
      console.log("Skipping Max Connections test (remote — spawns local wrangler)...");
      console.log("Skipping connection limit tests (remote — spawn local wrangler)...");
      console.log("Skipping Restart Persistence test (remote — spawns local wrangler)...");
      console.log("Skipping Info Cache tests (remote — spawn local wrangler)...");
    }
    console.log("\nAll tests passed successfully! 🚀");
    process.exit(0);
  } catch (err) {
    console.error("\n❌ Test failed:", err.message);
    process.exit(1);
  }
}

runAll();
