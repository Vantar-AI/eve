# Eve Wire v0

Eve Wire carries conversation transitions between plan-backed endpoint machines. The runtime now has two encodings with identical semantics:

- `reference` sends a self-describing envelope containing the wire version, conversation, semantic identity, state, sequence, and complete frame;
- `compact` sends only a compiled transition ID, sequence, and optional data payload.

Use the compact path with a verified Eve Plan:

```bash
cargo run -- compile examples/generate.eveconv.json
cargo run -- run-plan build/generate.eveplan.json \
  --wire compact --transport quic --tokens 3
```

The same `--wire compact` option works with memory and TCP.

## Compiled transition dictionary

Compilation projects both endpoints, deduplicates their matching send/receive actions, sorts the resulting semantic transitions deterministically, and assigns dense `u16` IDs beginning at one. The dictionary is covered by `plan_identity` and stored in `wire.transitions`.

For the example conversation, transition `7` means:

```json
{
  "id": 7,
  "state": "start",
  "op": "data",
  "from": "client",
  "to": "server",
  "message": "prompt"
}
```

The reference envelope repeats those semantics on every prompt. Compact Eve Wire sends:

```json
{"t":7,"q":0,"p":{"text":"hello"}}
```

For that payload, compact JSON is 34 bytes versus 279 bytes for the reference envelope, an 88% reduction before transport framing. The ratio varies with payload size. Here `t` is the transition ID, `q` is the monotonic sequence, and optional `p` is present only for data payloads. Select and cancel transitions carry no payload. Local fault observations are not transmitted.

The receiver resolves `t` through its verified plan, reconstructs the complete semantic frame and state, then runs the same conversation, identity, sequence, and endpoint-action checks used by the reference path. Execution reports and traces therefore remain representation-independent. Tests require reference and compact sessions over memory, TCP, and QUIC to produce the same semantic trace.

## Plan-bound network session

TCP and QUIC exchange a versioned session preface before frame zero. Each side declares:

```json
{
  "eve_session": "0.1.0",
  "conversation": "example.generate",
  "conversation_identity": "sha256:…",
  "plan_identity": "sha256:…",
  "role": "client",
  "wire": "compact"
}
```

Both peers send before receiving, then independently require exact agreement on session version, conversation, semantic identity, plan identity, peer role, and wire encoding. A valid preface produces a one-byte acceptance and waits for peer acceptance; an invalid preface produces a rejection and closes. There is no automatic fallback: `compact` versus `reference` is a fatal mismatch. Compact TCP and QUIC transports reject semantic frames until both sides accept.

The example preface is 278 bytes plus a four-byte length prefix and one status byte in each direction. The two phases add one application-level validation round trip before the conversation. It is governed by [`eve-session-v0.schema.json`](../spec/eve-session-v0.schema.json).

Independent processes can now use the compact path directly:

```bash
# Server
cargo run -- serve-quic --wire compact \
  --certificate-out build/eve-quic-cert.der

# Client
cargo run -- connect-quic --wire compact \
  --certificate build/eve-quic-cert.der
```

## Authentication boundary

On QUIC, the client pins the server certificate before the preface travels inside the authenticated TLS channel. That binds the server's declared role, plan, and encoding to the pinned server key and prevents an on-path downgrade. The v0 QUIC server does not authenticate the client; any client that can reach it may claim the expected public plan and role. Mutual TLS, authorization, per-session nonces, and replay-resistant application proofs remain open.

TCP performs the same exact mismatch checks, but plaintext TCP does not authenticate either peer and cannot resist an active network attacker. Its preface is a correctness and interoperability guard, not a security boundary.

The encoding is still JSON and payload values are still dynamically represented. The short field names are an experiment, not a stable network standard. A future binary encoding can reuse the same transition dictionary without changing conversation semantics.

## Measured result

On the checked-in Apple M4 release run, compact encoding reduced the checked-transition median from 2.417 µs to 1.833 µs, a 1.32× speedup. The complete warm exchange improved from 92.5 µs to 83.6 µs, a 1.11× speedup. Relative to the hand-written baseline, median overhead fell from 1.68× to 1.51×, still above the provisional 1.25× target. That in-process memory benchmark does not include the network preface.

This shows that specialization helps, while also locating the next costs in state-machine work, allocation, thread creation, and channels. See [the benchmark](benchmark.md) for methodology and limitations.
