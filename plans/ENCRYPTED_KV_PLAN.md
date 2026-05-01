# Implementation Plan: Internal Relay Key Management & Private Encryption

## Objective
Implement a "Zero-Touch" asymmetric encryption system where the relay manages its own Secp256k1 key pair within Durable Object storage. The public key is shared privately via KV for local encryption of NWC strings.

## Key Files & Context
- `Cargo.toml`: Add `chacha20poly1305` and `hkdf`.
- `src/lib.rs`: Update `NwcRelay` to manage keys in `state.storage()` and expose the public key via KV.
- `src/nwc_client.rs`: Implement NIP-44 (v2) for decryption.
- `src/utils.rs`: Add decryption helpers that interact with the Durable Object's internal key.
- `scripts/encrypt_nwc.js`: Update to fetch the public key from KV (via wrangler) and encrypt strings locally.

## Implementation Steps

### 1. Update Dependencies
- Add `chacha20poly1305 = "0.10"` and `hkdf = "0.12"` to `Cargo.toml`.

### 2. Internal Key Lifecycle (`src/lib.rs`)
- In `NwcRelay::new` or a dedicated initialization check:
    1. Check `state.storage().get::<String>("relay_privkey")`.
    2. If missing:
        - Generate a fresh 32-byte private key.
        - Store it in `state.storage()`.
        - Derive the public key.
        - Store the public key in the `MEKHALA_NWC_KV` namespace under the key `_relay_public_key`.
    3. Cache the private key in-memory in the `NwcRelay` struct.

### 3. NIP-44 Decryption (`src/nwc_client.rs`)
- Implement `decrypt_nip44` using the NIP-44 v2 specification.
- This function will be used by the Durable Object when processing LN Address requests.

### 4. Integration in LN Address Flow (`src/lnurl.rs` & `src/lib.rs`)
- Update the flow:
    1. `handle_lnurlp_callback` fetches the encrypted URI from KV.
    2. It sends this blob to the Durable Object.
    3. The Durable Object decrypts it using its internal private key.
    4. The payment request is processed.

### 5. Key Rotation Support
- Implement an internal "Migrate/Rotate" trigger:
    - On request, move `relay_privkey` to `relay_privkey_old`.
    - Generate a new `relay_privkey`.
    - Update `_relay_public_key` in KV.
    - The decryption logic will try the current key, then the old key.

### 6. Local Tooling (`scripts/encrypt_nwc.js`)
- Update the script to:
    1. Run `npx wrangler kv key get --binding MEKHALA_NWC_KV _relay_public_key` to get the key.
    2. Encrypt the NWC URI.
    3. Output the command to store it back in KV for a specific user.

## Verification & Testing
- Unit tests for NIP-44.
- Integration test for the full "Self-Initialization" flow.
- Verify that `_relay_public_key` is correctly populated in local KV during tests.
