import { RELAY_URL, HTTP_URL, baseURL, httpProtocol, relaySecret, isLocal } from "./env.js";

export async function testAuth() {
  if (
    !relaySecret ||
    (relaySecret === "test-secret" && baseURL === "localhost:8787")
  ) {
    if (relaySecret === "") {
      console.log("Skipping Authentication tests (Public Relay mode)...");
      return;
    }
  }

  console.log("Testing Authentication (Unauthorized access)...");
  const rootURL = `${httpProtocol}${baseURL}/`;
  const response = await fetch(rootURL);
  if (response.status !== 404) {
    throw new Error(
      "Auth failed: Root path should return 404, but got " + response.status,
    );
  }

  const wrongURL = `${httpProtocol}${baseURL}/wrong-secret`;
  const responseWrong = await fetch(wrongURL);
  if (responseWrong.status !== 404) {
    throw new Error(
      "Auth failed: Wrong secret path should return 404, but got " +
        responseWrong.status,
    );
  }
  console.log("✅ Authentication rejection passed.");
}

export async function testNip11() {
  console.log("Testing NIP-11 (Relay Information)...");
  const response = await fetch(HTTP_URL, {
    headers: { Accept: "application/nostr+json" },
  });
  let data;
  try {
    const clonedResponse = response.clone();
    data = await clonedResponse.json();
  } catch (e) {
    const text = await response.text();
    console.error(
      `Failed to parse JSON. Status: ${response.status}. Body: ${text}`,
    );
    throw e;
  }
  if (!data.supported_nips.includes(47)) {
    throw new Error("NIP-11 failed: " + JSON.stringify(data));
  }
  console.log("✅ NIP-11 JSON metadata passed.");

  console.log("Testing NIP-11 (Plain HTTP fallback compatibility)...");
  const responsePlain = await fetch(HTTP_URL);
  if (responsePlain.status !== 200) {
    throw new Error(
      "Plain HTTP fallback should now return 200, but got: " +
        responsePlain.status,
    );
  }
  console.log("✅ NIP-11 Plain HTTP fallback compatibility passed.");
}

export async function testCorsAndHeaders() {
  console.log("\n--- Testing CORS and Response Headers ---");

  const response = await fetch(HTTP_URL, {
    headers: { Accept: "application/nostr+json" },
  });

  const contentType = response.headers.get("content-type");
  if (!contentType || !contentType.includes("application/nostr+json")) {
    throw new Error(`Expected Content-Type 'application/nostr+json', got: '${contentType}'`);
  }
  console.log("✅ NIP-11 Content-Type is application/nostr+json.");

  const corsOrigin = response.headers.get("access-control-allow-origin");
  if (!corsOrigin || corsOrigin !== "*") {
    throw new Error(`Expected Access-Control-Allow-Origin '*', got: '${corsOrigin}'`);
  }
  console.log("✅ CORS headers present on NIP-11 response.");

  const secHeaders = ["strict-transport-security", "x-content-type-options", "content-security-policy"];
  for (const header of secHeaders) {
    const val = response.headers.get(header);
    if (!val) {
      throw new Error(`Missing security header: ${header}`);
    }
  }
  console.log("✅ Security headers present on NIP-11 response.");

  if (isLocal) {
    const lnUrl = `${HTTP_URL.replace(/\/$/, "")}/.well-known/lnurlp/testuser_relay`;
    const lnResponse = await fetch(lnUrl);
    const lnCors = lnResponse.headers.get("access-control-allow-origin");
    if (lnCors !== "*") {
      throw new Error(`Expected LN address CORS '*', got: '${lnCors}'`);
    }
    console.log("✅ CORS headers present on LN address response.");
  } else {
    console.log("Skipping LN address CORS check (remote — no local KV).");
  }
}

export async function testAuthHeaders() {
  console.log("\n--- Testing Security Headers on Auth Rejection ---");

  if (!relaySecret || relaySecret === "") {
    console.log("Skipping Auth Headers test (Public Relay mode)...");
    return;
  }

  const wrongURL = `${httpProtocol}${baseURL}/wrong-secret`;
  const response = await fetch(wrongURL);

  if (response.status !== 404) {
    throw new Error(`Expected 404 for wrong secret, got ${response.status}`);
  }

  const secHeaders = ["strict-transport-security", "x-content-type-options", "content-security-policy"];
  for (const header of secHeaders) {
    const val = response.headers.get(header);
    if (!val) {
      throw new Error(`Missing security header on auth rejection: ${header}`);
    }
  }
  console.log("✅ Security headers present on 404 auth rejection.");
}
