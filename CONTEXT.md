# Mekhala Domain Context

A high-performance, stateless Nostr relay optimized for **NIP-47 (Nostr Wallet Connect)** on Cloudflare Workers.

## Core Concepts

### Nostr Wallet Connect (NWC)
A protocol (NIP-47) that allows applications to communicate with Lightning wallets via Nostr. It uses specific event kinds for requests, responses, and notifications.

### Relay
The ephemeral routing engine. Mekhala is a specialized relay that only processes NWC-related events and does not persist them.

### WebSocket Hibernation
A Cloudflare Workers feature where Durable Objects can release memory while keeping a WebSocket connection "alive" in a low-power state. Mekhala uses this to maintain NWC subscriptions efficiently.

### Connection State
The data attached to a WebSocket connection, including active subscriptions and security limits. This state is serialized and restored during hibernation/wake-up.

### LN Address Bridging
Mekhala provides an LN Address bridge, allowing users to pay to an LN Address (e.g., `user@relay.com`) which then triggers an NWC request to the user's wallet.

## Domain Language

- **NWC Event**: A Nostr event with a kind in the 13194 or 23194-23197 range.
- **Filter**: A Nostr subscription filter (NIP-01).
- **Subscription**: A persistent request for events matching specific filters.
- **Relay Secret**: An optional path parameter used to restrict access to the relay.
- **Hibernation Tag**: The metadata (serialized `ConnectionState`) attached to a hibernating WebSocket.
